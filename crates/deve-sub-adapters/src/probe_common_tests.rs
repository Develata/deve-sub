use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn response(raw: &'static [u8]) -> reqwest::Response {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut request = [0; 1024];
        assert!(stream.read(&mut request).await.expect("request") > 0);
        stream.write_all(raw).await.expect("response");
    });
    let response = reqwest::get(format!("http://{address}"))
        .await
        .expect("get");
    server.await.expect("server");
    response
}

#[tokio::test]
async fn success_body_requires_complete_bounded_utf8() {
    let full = response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]").await;
    assert_eq!(read_body_capped(full, 2).await.expect("exact limit"), "[]");
    let oversized = response(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n[] ").await;
    assert!(read_body_capped(oversized, 2).await.is_err());
    // A valid JSON prefix is insufficient if the HTTP body was truncated.
    let partial = response(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n[]").await;
    assert!(read_body_capped(partial, 20).await.is_err());
}
