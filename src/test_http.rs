//! Minimal one-shot HTTP/1.1 server used by tests.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct TestServer {
    pub base_url: String,
}

/// Serve exactly one request with the given raw HTTP response.
pub async fn serve_once(response: Vec<u8>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    drop(tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();

        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        socket.write_all(&response).await.unwrap();
        let _ = socket.shutdown().await;
    }));

    TestServer {
        base_url: format!("http://{}", addr),
    }
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
