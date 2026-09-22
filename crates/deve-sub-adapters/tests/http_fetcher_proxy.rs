#![allow(clippy::expect_used)]

//! SEC-003: inherited proxy settings cannot replace the checked destination.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use deve_sub_adapters::{DStatusProbeAdapter, HttpFetcher, SsrfChecker};
use deve_sub_application::{FetchResult, SubscriptionFetcher};
use deve_sub_domain::{ProbeSource, ProbeSourceAdapter, ProbeSourceKind};
use deve_sub_kernel::{ProbeSourceId, Timestamp};
use deve_sub_security::SsrfError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct LoopbackChecker;

impl SsrfChecker for LoopbackChecker {
    fn check(
        &self,
        _url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, SsrfError>> + Send>> {
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]) })
    }
}

#[test]
fn sec003_environment_proxy_cannot_override_pinned_address() {
    const CHILD: &str = "DEVE_SUB_PROXY_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(async {
                fetch_pinned_destination().await;
                probe_pinned_destination().await;
            });
        return;
    }

    // WHY: a subprocess isolates environment changes from concurrently running
    // tests, without unsafe process-global set_var or dependence on the host's
    // proxy configuration. Port 1 is a synthetic unavailable proxy endpoint.
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "sec003_environment_proxy_cannot_override_pinned_address",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("NO_PROXY", "")
        .env("no_proxy", "");
    for name in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env(name, "http://127.0.0.1:1");
    }
    let mut child = command.spawn().expect("spawn isolated regression");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            assert!(status.success(), "pinned fetch failed with inherited proxy");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("proxy regression exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

async fn probe_pinned_destination() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("probe listener");
    let port = listener.local_addr().expect("probe address").port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept pinned probe");
        let mut request = [0; 4096];
        let count = stream.read(&mut request).await.expect("read probe");
        assert!(
            String::from_utf8_lossy(&request[..count])
                .starts_with("GET /api/allnode_status HTTP/1.1")
        );
        let body = r#"{"success":true,"data":{}}"#;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .expect("write probe response");
    });
    let source = ProbeSource {
        revision: 0,
        id: ProbeSourceId::new(),
        kind: ProbeSourceKind::DStatus,
        name: "fixture".into(),
        endpoint_url: format!("http://proxy-fixture.invalid:{port}"),
        auth_config: String::new(),
        subscription_id: None,
        enabled: true,
        last_sync_at: None,
        last_sync_status: None,
        last_counter_snapshot: None,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    };
    let adapter = DStatusProbeAdapter::with_checker(Arc::new(LoopbackChecker));
    let result = tokio::time::timeout(Duration::from_secs(2), adapter.sync_traffic(&source))
        .await
        .expect("bounded probe sync")
        .expect("probe must reach the checked IP");
    assert!(result.samples.is_empty());
    assert!(result.new_counter_snapshot.is_some());
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("probe fixture finished")
        .expect("probe fixture succeeded");
}

async fn fetch_pinned_destination() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture listener");
    let port = listener.local_addr().expect("fixture address").port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept pinned request");
        let mut request = [0; 4096];
        let count = stream.read(&mut request).await.expect("read request");
        assert!(String::from_utf8_lossy(&request[..count]).starts_with("GET /sub HTTP/1.1"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\npinned")
            .await
            .expect("write fixture response");
    });
    // .invalid never resolves via normal DNS: only the checker's pinned
    // loopback result can reach this server; an environment proxy must not.
    let result = HttpFetcher::with_checker(LoopbackChecker)
        .timeout(2)
        .fetch(&format!("http://proxy-fixture.invalid:{port}/sub"), None)
        .await;
    assert!(
        matches!(result, Ok(FetchResult::Ok { ref body, .. }) if body == b"pinned"),
        "fetch must reach the checked IP: {result:?}"
    );
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("fixture finished")
        .expect("fixture succeeded");
}
