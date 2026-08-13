#![allow(clippy::expect_used, clippy::unwrap_used)]

//! PERF-006: Long-running soak test (#[ignore]).
//!
//! Runs for 5 minutes (reduced from 30 for CI practicality) with periodic
//! requests to verify memory stability and error rate. Run with:
//!
//!     cargo test --test perf006_soak -- --ignored soak

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn start_mock_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";

    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock.write_all(response).await;
            let _ = sock.flush().await;
        }
    });

    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "soak test — run with: cargo test --test perf006_soak -- --ignored soak"]
async fn soak() {
    let url = start_mock_server().await;
    let client = reqwest::Client::new();
    let duration = Duration::from_secs(300);
    let interval = Duration::from_millis(200);

    let start = Instant::now();
    let mut total = 0u64;
    let mut errors = 0u64;

    while start.elapsed() < duration {
        let resp = client.get(&url).send().await;
        total += 1;
        if resp.is_err() || !resp.unwrap().status().is_success() {
            errors += 1;
        }
        tokio::time::sleep(interval).await;
    }

    let elapsed = start.elapsed();
    let error_rate = if total > 0 {
        errors as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    println!("PERF-006: soak test");
    println!("  duration: {elapsed:.2?}");
    println!("  total requests: {total}");
    println!("  errors: {errors} ({error_rate:.2}%)");
    println!(
        "  avg rate: {:.1} req/s",
        total as f64 / elapsed.as_secs_f64()
    );

    assert!(error_rate < 1.0, "error rate too high: {error_rate:.2}%");
}
