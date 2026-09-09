#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Mock HTTP transport concurrency smoke (not application throughput).
//!
//! Starts a mock HTTP server and measures throughput of 500 concurrent
//! requests. This benchmarks the HTTP client and connection pool under
//! load, It does not launch or measure Deve Sub.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn start_mock_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    // WHY `Connection: close`: the mock closes the socket after one response.
    // Without this header, reqwest's connection pool keeps the socket in the
    // idle pool and reuses it for the next request, hitting a reset. Telling
    // the client to close after each response matches the server's behavior.
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            // WHY spawn per-connection: under 500 concurrent requests, a
            // sequential accept→read→write loop overflows the TCP backlog
            // and resets queued connections. Per-connection tasks let the
            // server handle them concurrently.
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock.write_all(response).await;
                let _ = sock.flush().await;
            });
        }
    });

    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_http_transport_concurrency_smoke() {
    let base_url = start_mock_server().await;
    let client = reqwest::Client::new();
    let concurrency = 500;
    let total = 500;

    let start = std::time::Instant::now();
    let mut handles = Vec::with_capacity(total);

    for _ in 0..total {
        let client = client.clone();
        let url = base_url.clone();
        handles.push(tokio::spawn(async move {
            let resp = client.get(&url).send().await.expect("request");
            assert!(resp.status().is_success());
        }));
    }

    for handle in handles {
        handle.await.expect("task");
    }

    let elapsed = start.elapsed();
    let rps = total as f64 / elapsed.as_secs_f64();

    println!("Mock transport smoke: {total} requests at concurrency {concurrency}");
    println!("  elapsed: {elapsed:.2?}");
    println!("  throughput: {rps:.0} req/s");
    println!("  per-request: {:.2?}", elapsed / total as u32);

    // Completion is the smoke invariant; transport timing is diagnostic only.
}
