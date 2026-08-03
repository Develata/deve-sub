#![allow(clippy::expect_used, reason = "spike binary, bind failure is fatal")]

use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
    let addr = "0.0.0.0:8080";
    let listener = TcpListener::bind(addr).expect("bind");
    eprintln!("deve-sub docker spike listening on {addr}");

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept error: {e}");
                continue;
            }
        };

        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            r#"{"app":"deve-sub","spike":"docker-build","version":"0.0.0"}"#,
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
    }
}
