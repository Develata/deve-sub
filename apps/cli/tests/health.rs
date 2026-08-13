#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Integration tests for `deve-sub health` subcommands.
//!
//! Uses the subprocess strategy: spawns the compiled `deve-sub` binary and
//! asserts exit codes. A minimal HTTP server (tokio TcpListener with
//! hand-written HTTP/1.1 responses) provides the health endpoint to probe.

use std::process::Command;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Hand-written 200 OK response.
fn http_200() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"
}

/// Hand-written 503 Service Unavailable response.
fn http_503() -> &'static [u8] {
    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 9\r\n\r\nunhealthy"
}

/// Start a minimal HTTP server that responds with the given status for all
/// requests. Returns the bound address with the given path appended.
async fn start_mock_server(response: &'static [u8], path: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind failed");
    let addr = listener.local_addr().expect("local_addr failed");
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock.write_all(response).await;
            let _ = sock.flush().await;
        }
    });
    format!("http://{addr}{path}")
}

#[tokio::test(flavor = "multi_thread")]
async fn health_live_2xx_exits_0() {
    let url = start_mock_server(http_200(), "/health/live").await;
    let status = Command::new(env!("CARGO_BIN_EXE_deve-sub"))
        .args(["health", "live", "--url", &url])
        .status()
        .expect("failed to spawn deve-sub");
    assert!(status.success(), "expected exit 0 for 2xx, got {status:?}");
    assert_eq!(status.code(), Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn health_live_5xx_exits_1() {
    let url = start_mock_server(http_503(), "/health/live").await;
    let status = Command::new(env!("CARGO_BIN_EXE_deve-sub"))
        .args(["health", "live", "--url", &url])
        .status()
        .expect("failed to spawn deve-sub");
    assert!(!status.success(), "expected exit 1 for 5xx, got {status:?}");
    assert_eq!(status.code(), Some(1));
}

#[tokio::test]
async fn health_live_connection_refused_exits_1() {
    // Use a port that's almost certainly not listening.
    let url = "http://127.0.0.1:1/health/live";
    let status = Command::new(env!("CARGO_BIN_EXE_deve-sub"))
        .args(["health", "live", "--url", url, "--timeout", "1"])
        .status()
        .expect("failed to spawn deve-sub");
    assert_eq!(status.code(), Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn health_ready_2xx_exits_0() {
    let url = start_mock_server(http_200(), "/health/ready").await;
    let status = Command::new(env!("CARGO_BIN_EXE_deve-sub"))
        .args(["health", "ready", "--url", &url])
        .status()
        .expect("failed to spawn deve-sub");
    assert!(status.success(), "expected exit 0 for 2xx, got {status:?}");
    assert_eq!(status.code(), Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn health_ready_5xx_exits_1() {
    let url = start_mock_server(http_503(), "/health/ready").await;
    let status = Command::new(env!("CARGO_BIN_EXE_deve-sub"))
        .args(["health", "ready", "--url", &url])
        .status()
        .expect("failed to spawn deve-sub");
    assert!(!status.success(), "expected exit 1 for 5xx, got {status:?}");
    assert_eq!(status.code(), Some(1));
}

#[tokio::test]
async fn health_ready_connection_refused_exits_1() {
    let url = "http://127.0.0.1:1/health/ready";
    let status = Command::new(env!("CARGO_BIN_EXE_deve-sub"))
        .args(["health", "ready", "--url", url, "--timeout", "1"])
        .status()
        .expect("failed to spawn deve-sub");
    assert!(
        !status.success(),
        "expected exit 1 for connection refused, got {status:?}"
    );
    assert_eq!(status.code(), Some(1));
}
