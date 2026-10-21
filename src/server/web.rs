//! Local HTTP/WebSocket bridge for browser-based frontends.
//!
//! The browser never talks to `mclc json` directly. This module spawns the
//! JSON server, owns its stdio, multiplexes requests from one or more
//! WebSocket clients, and keeps the launcher protocol unchanged on the wire.

use crate::net::download::DownloadSource;
use anyhow::Context;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::IpAddr;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot, watch};
use url::Url;

#[derive(Debug, Clone)]
pub struct WebOptions {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    pub allow_origins: Vec<String>,
}

struct WebState {
    bridge: Arc<Bridge>,
    bind_host: String,
    token: Option<String>,
    allow_origins: Vec<String>,
}

struct Bridge {
    commands: mpsc::Sender<BridgeRequest>,
    pending: StdMutex<HashMap<u64, Pending>>,
    ready: watch::Sender<Option<Value>>,
    clients: StdMutex<Vec<mpsc::UnboundedSender<Value>>>,
    alive: AtomicBool,
    next_id: AtomicU64,
    stdin: AsyncMutex<ChildStdin>,
}

struct BridgeRequest {
    id: Value,
    command: String,
    params: Value,
    owner: mpsc::UnboundedSender<Value>,
}

struct Pending {
    owner: mpsc::UnboundedSender<Value>,
    original_id: Value,
    result_tx: oneshot::Sender<Result<Value, String>>,
}

#[derive(Debug, Deserialize)]
struct WebRequest {
    #[serde(default)]
    id: Value,
    command: String,
    #[serde(default)]
    params: Value,
}

pub async fn run(
    options: WebOptions,
    source: DownloadSource,
    java_path: Option<String>,
) -> anyhow::Result<()> {
    if options.token.as_deref() == Some("") {
        anyhow::bail!("--token cannot be empty");
    }
    if !is_loopback_host(&options.host) && options.token.is_none() {
        anyhow::bail!("--token is required when binding to a non-loopback address");
    }

    let listener = TcpListener::bind((options.host.as_str(), options.port))
        .await
        .with_context(|| {
            format!(
                "Failed to bind web server to {}:{}",
                options.host, options.port
            )
        })?;
    let address = listener.local_addr()?;

    let bridge = spawn_launcher(source, java_path)?;
    let state = Arc::new(WebState {
        bridge,
        bind_host: options.host.clone(),
        token: options.token.clone(),
        allow_origins: options.allow_origins.clone(),
    });

    let app = Router::new()
        .route("/", get(info))
        .route("/health", get(info))
        .route("/ws", get(ws_handler))
        .with_state(state.clone());

    print_startup(&address, &options);

    tokio::select! {
        result = axum::serve(listener, app) => {
            result.context("Web server failed")?;
        }
        signal = tokio::signal::ctrl_c() => {
            signal.context("Failed to listen for Ctrl+C")?;
            println!("\nShutting down...");
        }
    }

    state.bridge.request_shutdown().await;
    Ok(())
}

fn spawn_launcher(
    source: DownloadSource,
    java_path: Option<String>,
) -> anyhow::Result<Arc<Bridge>> {
    let executable = std::env::current_exe().context("Failed to locate the mclc executable")?;
    let mut command = Command::new(executable);
    command.arg("json");
    match source {
        DownloadSource::Auto => {}
        DownloadSource::Official => {
            command.arg("--download-source").arg("official");
        }
        DownloadSource::Bmclapi => {
            command.arg("--download-source").arg("bmclapi");
        }
    }
    if let Some(java_path) = java_path.as_deref() {
        command.arg("--runtime").arg(java_path);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let mut child = command
        .spawn()
        .context("Failed to start the mclc json subprocess")?;
    let stdin = child
        .stdin
        .take()
        .context("mclc json did not expose stdin")?;
    let stdout = child
        .stdout
        .take()
        .context("mclc json did not expose stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("mclc json did not expose stderr")?;

    let (commands, command_rx) = mpsc::channel(64);
    let (ready, _) = watch::channel(None);
    let bridge = Arc::new(Bridge {
        commands,
        pending: StdMutex::new(HashMap::new()),
        ready,
        clients: StdMutex::new(Vec::new()),
        alive: AtomicBool::new(true),
        next_id: AtomicU64::new(1),
        stdin: AsyncMutex::new(stdin),
    });

    tokio::spawn(read_launcher_output(bridge.clone(), stdout));
    tokio::spawn(read_launcher_stderr(stderr));
    tokio::spawn(dispatch_requests(bridge.clone(), command_rx));

    let wait_bridge = bridge.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        eprintln!("mclc json exited: {:?}", status);
        wait_bridge.launcher_gone();
    });

    Ok(bridge)
}

async fn read_launcher_output(bridge: Arc<Bridge>, stdout: ChildStdout) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<Value>(line) {
                    bridge.handle_launcher_event(event);
                }
            }
            Ok(None) => break,
            Err(error) => {
                eprintln!("Failed to read mclc json output: {}", error);
                break;
            }
        }
    }
    bridge.launcher_gone();
}

async fn read_launcher_stderr(stderr: ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("[mclc] {}", line);
    }
}

async fn dispatch_requests(bridge: Arc<Bridge>, mut queue: mpsc::Receiver<BridgeRequest>) {
    while let Some(request) = queue.recv().await {
        if request.owner.is_closed() {
            continue;
        }
        if !bridge.alive.load(Ordering::SeqCst) {
            send_error(&request.owner, request.id, "launcher process exited");
            continue;
        }

        let internal_id = bridge.next_id.fetch_add(1, Ordering::SeqCst);
        let (result_tx, result_rx) = oneshot::channel();
        bridge.pending.lock().unwrap().insert(
            internal_id,
            Pending {
                owner: request.owner.clone(),
                original_id: request.id.clone(),
                result_tx,
            },
        );

        let message = json!({
            "id": internal_id,
            "command": request.command,
            "params": request.params,
        });
        let mut line = match serde_json::to_vec(&message) {
            Ok(line) => line,
            Err(error) => {
                bridge.pending.lock().unwrap().remove(&internal_id);
                send_error(
                    &request.owner,
                    request.id,
                    format!("Failed to encode request: {}", error),
                );
                continue;
            }
        };
        line.push(b'\n');

        let write_result = {
            let mut stdin = bridge.stdin.lock().await;
            async {
                stdin.write_all(&line).await?;
                stdin.flush().await?;
                Ok::<(), std::io::Error>(())
            }
            .await
        };
        if let Err(error) = write_result {
            bridge.pending.lock().unwrap().remove(&internal_id);
            send_error(
                &request.owner,
                request.id,
                format!("Failed to write to launcher: {}", error),
            );
            continue;
        }

        match result_rx.await {
            Ok(Ok(data)) => send_message(
                &request.owner,
                json!({"type": "result", "id": request.id, "ok": true, "data": data}),
            ),
            Ok(Err(error)) => send_error(&request.owner, request.id, error),
            Err(_) => send_error(&request.owner, request.id, "launcher process exited"),
        }
    }
}

impl Bridge {
    fn subscribe_ready(&self) -> watch::Receiver<Option<Value>> {
        self.ready.subscribe()
    }

    fn register(&self, sender: mpsc::UnboundedSender<Value>) {
        self.clients.lock().unwrap().push(sender);
    }

    fn unregister(&self, sender: &mpsc::UnboundedSender<Value>) {
        self.clients
            .lock()
            .unwrap()
            .retain(|client| !client.same_channel(sender));
    }

    fn enqueue(&self, request: WebRequest, owner: mpsc::UnboundedSender<Value>) {
        let id = request.id.clone();
        match self.commands.try_send(BridgeRequest {
            id: request.id,
            command: request.command,
            params: request.params,
            owner: owner.clone(),
        }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                send_error(&owner, id, "launcher request queue is full");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                send_error(&owner, id, "launcher is not available");
            }
        }
    }

    fn handle_launcher_event(&self, mut event: Value) {
        let kind = event.get("type").and_then(Value::as_str).map(str::to_owned);

        match kind.as_deref() {
            Some("result") | Some("error") => {
                let Some(internal_id) = event.get("id").and_then(Value::as_u64) else {
                    return;
                };
                let pending = self.pending.lock().unwrap().remove(&internal_id);
                if let Some(pending) = pending {
                    let outcome = if kind.as_deref() == Some("result") {
                        Ok(event.get("data").cloned().unwrap_or(Value::Null))
                    } else {
                        Err(event
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error")
                            .to_string())
                    };
                    let _ = pending.result_tx.send(outcome);
                }
            }
            Some("ready") => {
                // New clients receive the current value from the watch channel;
                // existing clients observe the change in their writer task.
                self.ready.send_replace(Some(event));
            }
            _ => {
                if let Some(internal_id) = event.get("id").and_then(Value::as_u64) {
                    let target = self
                        .pending
                        .lock()
                        .unwrap()
                        .get(&internal_id)
                        .map(|pending| (pending.owner.clone(), pending.original_id.clone()));
                    if let Some((owner, original_id)) = target {
                        event["id"] = original_id;
                        let _ = owner.send(event);
                        return;
                    }
                }
                event["id"] = Value::Null;
                self.broadcast(event);
            }
        }
    }

    fn broadcast(&self, event: Value) {
        let mut clients = self.clients.lock().unwrap();
        clients.retain(|client| !client.is_closed());
        for client in clients.iter() {
            let _ = client.send(event.clone());
        }
    }

    fn launcher_gone(&self) {
        if !self.alive.swap(false, Ordering::SeqCst) {
            return;
        }
        let pending: Vec<Pending> = {
            let mut pending = self.pending.lock().unwrap();
            pending.drain().map(|(_, pending)| pending).collect()
        };
        for pending in pending {
            let _ = pending
                .result_tx
                .send(Err("launcher process exited".into()));
        }
        self.broadcast(json!({"type": "launcher_exit", "id": null}));
    }

    async fn request_shutdown(&self) {
        let line = b"{\"id\":0,\"command\":\"shutdown\"}\n";
        let mut stdin = self.stdin.lock().await;
        let _ = stdin.write_all(line).await;
        let _ = stdin.flush().await;
    }
}

async fn info(State(state): State<Arc<WebState>>) -> Response {
    let ready = {
        let ready = state.bridge.ready.borrow();
        (*ready).clone()
    };
    let protocol = ready
        .as_ref()
        .and_then(|ready| ready.get("protocol"))
        .cloned()
        .unwrap_or_else(|| json!(1));
    let version = ready
        .as_ref()
        .and_then(|ready| ready.get("version"))
        .cloned()
        .unwrap_or_else(|| json!(env!("CARGO_PKG_VERSION")));
    let alive = state.bridge.alive.load(Ordering::SeqCst);
    let body = json!({
        "name": "mclc web",
        "protocol": protocol,
        "version": version,
        "websocket": "/ws",
        "launcher_alive": alive,
        "auth_required": state.token.is_some(),
    })
    .to_string();

    let mut response = (
        if alive {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        body,
    )
        .into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    match state.authorize(&headers, uri.query().unwrap_or("")) {
        Ok(()) => ws
            .max_message_size(1024 * 1024)
            .on_upgrade(move |socket| handle_socket(socket, state)),
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn handle_socket(socket: WebSocket, state: Arc<WebState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    state.bridge.register(tx.clone());

    let mut ready_rx = state.bridge.subscribe_ready();
    let initial_ready = {
        let ready = ready_rx.borrow();
        (*ready).clone()
    };
    let mut ready_sent = false;
    if let Some(ready) = initial_ready {
        if sender
            .send(Message::Text(ready.to_string().into()))
            .await
            .is_err()
        {
            state.bridge.unregister(&tx);
            return;
        }
        ready_sent = true;
    }
    if !state.bridge.alive.load(Ordering::SeqCst) {
        let _ = tx.send(json!({"type": "launcher_exit", "id": null}));
    }

    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                message = rx.recv() => {
                    let Some(message) = message else {
                        break;
                    };
                    if sender
                        .send(Message::Text(message.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                changed = ready_rx.changed(), if !ready_sent => {
                    if changed.is_err() {
                        break;
                    }
                    let ready = {
                        let ready = ready_rx.borrow();
                        (*ready).clone()
                    };
                    if let Some(ready) = ready {
                        if sender
                            .send(Message::Text(ready.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        ready_sent = true;
                    }
                }
            }
        }
    });

    println!("WebSocket client connected");

    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Text(text)) => match serde_json::from_str::<WebRequest>(text.as_str()) {
                Ok(request) => state.bridge.enqueue(request, tx.clone()),
                Err(error) => send_error(&tx, Value::Null, format!("Invalid request: {}", error)),
            },
            Ok(Message::Binary(_)) => {
                send_error(&tx, Value::Null, "Binary messages are not supported");
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("WebSocket error: {}", error);
                break;
            }
        }
    }

    state.bridge.unregister(&tx);
    send_task.abort();
    println!("WebSocket client disconnected");
}

impl WebState {
    fn authorize(
        &self,
        headers: &HeaderMap,
        query: &str,
    ) -> Result<(), (StatusCode, &'static str)> {
        if !host_allowed(header_value(headers, header::HOST), &self.bind_host) {
            return Err((StatusCode::FORBIDDEN, "invalid Host header"));
        }

        if let Some(expected) = self.token.as_deref() {
            let header_token = header_value(headers, header::AUTHORIZATION)
                .and_then(|value| value.strip_prefix("Bearer "));
            let query_token = query_param(query, "token");
            let supplied = header_token.map(str::to_owned).or(query_token);
            if supplied.as_deref() != Some(expected) {
                return Err((StatusCode::UNAUTHORIZED, "invalid or missing token"));
            }
        }

        if let Some(origin) = header_value(headers, header::ORIGIN)
            && !origin_allowed(origin, &self.allow_origins)
        {
            return Err((StatusCode::FORBIDDEN, "origin not allowed"));
        }

        Ok(())
    }
}

fn header_value(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn query_param(query: &str, name: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn host_allowed(host_header: Option<&str>, bind_host: &str) -> bool {
    if !is_loopback_host(bind_host) {
        return true;
    }
    let Some(host_header) = host_header else {
        return true;
    };
    let host = hostname_only(host_header);
    is_loopback_host(host) || host.eq_ignore_ascii_case(bind_host)
}

fn hostname_only(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn origin_allowed(origin: &str, allow_origins: &[String]) -> bool {
    let origin = origin.trim_end_matches('/');
    if allow_origins
        .iter()
        .any(|allowed| allowed.trim_end_matches('/').eq_ignore_ascii_case(origin))
    {
        return true;
    }

    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn send_message(owner: &mpsc::UnboundedSender<Value>, message: Value) {
    if !owner.is_closed() {
        let _ = owner.send(message);
    }
}

fn send_error(owner: &mpsc::UnboundedSender<Value>, id: Value, error: impl Into<String>) {
    send_message(
        owner,
        json!({"type": "error", "id": id, "ok": false, "error": error.into()}),
    );
}

fn print_startup(address: &std::net::SocketAddr, options: &WebOptions) {
    let host = display_host(address.ip());
    println!(
        "LauncherCore web backend listening on http://{}:{}",
        host,
        address.port()
    );
    println!("WebSocket endpoint: ws://{}:{}/ws", host, address.port());
    println!(
        "Health endpoint:    http://{}:{}/health",
        host,
        address.port()
    );
    if let Some(token) = &options.token {
        println!("Token required:     {}", token);
    } else {
        println!("Token:              not required (loopback origins only)");
    }
    if options.allow_origins.is_empty() {
        println!("Allowed origins:    loopback (localhost, 127.0.0.1, ::1)");
    } else {
        println!(
            "Allowed origins:    loopback + {}",
            options.allow_origins.join(", ")
        );
    }
    if !is_loopback_host(&options.host) {
        println!(
            "Warning: the web backend is bound to a non-loopback address. Use --token and --allow-origin, and do not expose it to untrusted networks."
        );
    }
}

fn display_host(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{}]", address),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_origins_are_allowed() {
        assert!(origin_allowed("http://localhost:5173", &[]));
        assert!(origin_allowed("https://localhost", &[]));
        assert!(origin_allowed("http://127.0.0.1:8080", &[]));
        assert!(origin_allowed("http://[::1]:3000", &[]));
        assert!(!origin_allowed("https://example.com", &[]));
        assert!(!origin_allowed("http://localhost.example.com", &[]));
        assert!(!origin_allowed("null", &[]));
    }

    #[test]
    fn explicit_origins_are_supported() {
        let allowed = vec![
            "https://launcher.example.com/".to_string(),
            "null".to_string(),
        ];
        assert!(origin_allowed("https://launcher.example.com", &allowed));
        assert!(origin_allowed("null", &allowed));
        assert!(!origin_allowed("https://other.example.com", &allowed));
    }

    #[test]
    fn host_headers_are_checked_for_loopback_binds() {
        assert!(host_allowed(Some("localhost:7420"), "127.0.0.1"));
        assert!(host_allowed(Some("127.0.0.1:7420"), "127.0.0.1"));
        assert!(host_allowed(Some("[::1]:7420"), "127.0.0.1"));
        assert!(!host_allowed(Some("evil.example.com:7420"), "127.0.0.1"));
        assert!(host_allowed(Some("evil.example.com:7420"), "0.0.0.0"));
    }

    #[test]
    fn request_defaults_are_null() {
        let request: WebRequest = serde_json::from_str(r#"{"command":"ping"}"#).unwrap();
        assert_eq!(request.id, Value::Null);
        assert_eq!(request.params, Value::Null);
    }

    #[test]
    fn query_tokens_are_decoded() {
        assert_eq!(query_param("token=abc", "token"), Some("abc".to_string()));
        assert_eq!(
            query_param("x=1&token=a%20b", "token"),
            Some("a b".to_string())
        );
        assert_eq!(query_param("x=1", "token"), None);
    }
}
