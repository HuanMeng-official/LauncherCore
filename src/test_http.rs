//! Minimal HTTP/1.1 server used by tests.

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub body: String,
}

pub struct TestServer {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

/// Serve exactly one request with the given raw HTTP response.
pub async fn serve_once(response: Vec<u8>) -> TestServer {
    serve_sequence(vec![response]).await
}

/// Serve the given responses in order, recording every request.
pub async fn serve_sequence(responses: Vec<Vec<u8>>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);

    drop(tokio::spawn(async move {
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            recorded.lock().unwrap().push(request);

            socket.write_all(&response).await.unwrap();
            let _ = socket.shutdown().await;
        }
    }));

    TestServer {
        base_url: format!("http://{}", addr),
        requests,
    }
}

async fn read_request(socket: &mut TcpStream) -> RecordedRequest {
    let mut buffer = Vec::new();
    let mut header_end = None;
    let mut chunk = [0u8; 4096];

    while header_end.is_none() {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = Some(index + 4);
        }
    }

    let header_end = header_end.unwrap_or(buffer.len());
    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    while buffer.len() < header_end + content_length {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }

    let request_line = header.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let body = String::from_utf8_lossy(&buffer[header_end..]).to_string();

    RecordedRequest { method, path, body }
}

pub fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {}\r\n", status);
    for (name, value) in headers {
        out.push_str(&format!("{}: {}\r\n", name, value));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));

    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
