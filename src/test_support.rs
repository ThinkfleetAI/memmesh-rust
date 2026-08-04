//! Shared test scaffolding: a dependency-free HTTP mock server used by the
//! resource test modules. Compiled only under `cfg(test)`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use crate::MemMesh;

/// A captured request: method, path (with query), and raw body.
#[derive(Debug)]
pub struct Captured {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// Spin up a one-shot-per-connection mock server. Answers `responses.len()`
/// sequential requests, replying with the matching canned body and recording
/// each request. `Connection: close` forces reqwest to reconnect per call.
pub fn mock_server(responses: Vec<String>) -> (String, mpsc::Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for body_out in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&data);
                if let Some(idx) = text.find("\r\n\r\n") {
                    let header = text[..idx].to_string();
                    let content_len = header
                        .lines()
                        .find_map(|l| {
                            let low = l.to_ascii_lowercase();
                            low.strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let body_start = idx + 4;
                    if data.len() >= body_start + content_len {
                        let first = header.lines().next().unwrap_or("");
                        let mut it = first.split_whitespace();
                        let method = it.next().unwrap_or("").to_string();
                        let path = it.next().unwrap_or("").to_string();
                        let req_body =
                            String::from_utf8_lossy(&data[body_start..body_start + content_len])
                                .to_string();
                        tx.send(Captured { method, path, body: req_body }).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body_out.len(),
                            body_out
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                        stream.flush().unwrap();
                        break;
                    }
                }
            }
        }
    });
    (format!("http://{addr}"), rx)
}

/// A client pointed at the mock server's base URL.
pub fn client(base: &str) -> MemMesh {
    MemMesh::with_base_url("sk-test", "proj", base)
}
