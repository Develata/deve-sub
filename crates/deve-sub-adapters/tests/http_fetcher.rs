#![allow(clippy::expect_used)]

//! Adapter tests for `HttpFetcher` (SRC-012, SEC-003, SEC-004).
//!
//! SRC-012: gzip/deflate/brotli/zstd compressed responses are decompressed.
//! SEC-003: the SSRF checker is called before every connection, and the
//!          resolved IP is pinned for the actual connection (DNS pinning).
//! SEC-004: redirect to an internal address is rejected at each hop.

use std::future::Future;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deve_sub_adapters::{HttpFetcher, SsrfChecker};
use deve_sub_application::{FetchError, FetchResult, SubscriptionFetcher};
use deve_sub_security::SsrfError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Mock SSRF checkers
// ---------------------------------------------------------------------------

struct AllowAllChecker;

impl SsrfChecker for AllowAllChecker {
    fn check(
        &self,
        _url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, SsrfError>> + Send>> {
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]) })
    }
}

struct WrongIpChecker;

impl SsrfChecker for WrongIpChecker {
    fn check(
        &self,
        _url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, SsrfError>> + Send>> {
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))]) })
    }
}

struct RecordingChecker {
    calls: Arc<Mutex<Vec<String>>>,
}

impl SsrfChecker for RecordingChecker {
    fn check(
        &self,
        url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, SsrfError>> + Send>> {
        self.calls.lock().expect("mutex").push(url.to_owned());
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]) })
    }
}

struct RecordingSelectiveChecker {
    calls: Arc<Mutex<Vec<String>>>,
    blocked: Vec<IpAddr>,
}

impl SsrfChecker for RecordingSelectiveChecker {
    fn check(
        &self,
        url: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, SsrfError>> + Send>> {
        self.calls.lock().expect("mutex").push(url.to_owned());
        let url = url.to_owned();
        let blocked = self.blocked.clone();
        Box::pin(async move {
            let parsed = url::Url::parse(&url).expect("valid URL");
            if let Some(host) = parsed.host_str()
                && let Ok(ip) = host.parse::<IpAddr>()
            {
                if blocked.contains(&ip) {
                    return Err(SsrfError::Blocked("blocked by test checker".to_owned()));
                }
                return Ok(vec![ip]);
            }
            Ok(vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))])
        })
    }
}

// ---------------------------------------------------------------------------
// Mock HTTP server
// ---------------------------------------------------------------------------

fn http_response(status_line: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status_line}\r\n");
    for (key, value) in headers {
        response.push_str(&format!("{key}: {value}\r\n"));
    }
    response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    response.push_str("\r\n");
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

async fn start_mock_server(response: Vec<u8>) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await;
            let _ = stream.write_all(&response).await;
            let _ = stream.flush().await;
        }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    addr
}

const ORIGINAL_BODY: &[u8] = b"trojan://PASS@example.com:443?sni=example.com&type=tcp#Node";

// ---------------------------------------------------------------------------
// SRC-012: compressed responses are decompressed
// ---------------------------------------------------------------------------

/// SRC-012: gzip-compressed response is decompressed to the original body.
#[tokio::test]
async fn gzip_response_decompressed() {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(ORIGINAL_BODY).expect("gzip encode");
    let compressed = encoder.finish().expect("gzip finish");

    let response = http_response(
        "200 OK",
        &[("Content-Type", "text/plain"), ("Content-Encoding", "gzip")],
        &compressed,
    );
    let addr = start_mock_server(response).await;

    let fetcher = HttpFetcher::with_checker(AllowAllChecker).timeout(5);
    let url = format!("http://127.0.0.1:{}/sub", addr.port());
    let result = fetcher.fetch(&url, None).await.expect("fetch");

    match result {
        FetchResult::Ok { body, .. } => {
            assert_eq!(body, ORIGINAL_BODY, "decompressed gzip body should match");
        }
        _ => panic!("expected Ok, got {result:?}"),
    }
}

/// SRC-012: brotli-compressed response is decompressed to the original body.
#[tokio::test]
async fn brotli_response_decompressed() {
    let mut compressed = Vec::new();
    let mut reader = brotli::CompressorReader::new(ORIGINAL_BODY, 4096, 11, 22);
    reader.read_to_end(&mut compressed).expect("brotli encode");

    let response = http_response(
        "200 OK",
        &[("Content-Type", "text/plain"), ("Content-Encoding", "br")],
        &compressed,
    );
    let addr = start_mock_server(response).await;

    let fetcher = HttpFetcher::with_checker(AllowAllChecker).timeout(5);
    let url = format!("http://127.0.0.1:{}/sub", addr.port());
    let result = fetcher.fetch(&url, None).await.expect("fetch");

    match result {
        FetchResult::Ok { body, .. } => {
            assert_eq!(body, ORIGINAL_BODY, "decompressed brotli body should match");
        }
        _ => panic!("expected Ok, got {result:?}"),
    }
}

/// SRC-012: zstd-compressed response is decompressed to the original body.
#[tokio::test]
async fn zstd_response_decompressed() {
    let compressed = zstd::encode_all(ORIGINAL_BODY, 3).expect("zstd encode");

    let response = http_response(
        "200 OK",
        &[("Content-Type", "text/plain"), ("Content-Encoding", "zstd")],
        &compressed,
    );
    let addr = start_mock_server(response).await;

    let fetcher = HttpFetcher::with_checker(AllowAllChecker).timeout(5);
    let url = format!("http://127.0.0.1:{}/sub", addr.port());
    let result = fetcher.fetch(&url, None).await.expect("fetch");

    match result {
        FetchResult::Ok { body, .. } => {
            assert_eq!(body, ORIGINAL_BODY, "decompressed zstd body should match");
        }
        _ => panic!("expected Ok, got {result:?}"),
    }
}

// ---------------------------------------------------------------------------
// SEC-003: SSRF checker called before connect + DNS pinning
// ---------------------------------------------------------------------------

/// SEC-003: The SSRF checker is called with the URL before connecting.
#[tokio::test]
async fn ssrf_checker_called_before_connect() {
    let response = http_response("200 OK", &[("Content-Type", "text/plain")], b"ok");
    let addr = start_mock_server(response).await;

    let calls = Arc::new(Mutex::new(Vec::new()));
    let checker = RecordingChecker {
        calls: calls.clone(),
    };
    let fetcher = HttpFetcher::with_checker(checker).timeout(5);
    let url = format!("http://127.0.0.1:{}/sub", addr.port());
    let _ = fetcher.fetch(&url, None).await;

    let recorded = calls.lock().expect("mutex").clone();
    assert!(
        !recorded.is_empty(),
        "SSRF checker should have been called at least once"
    );
    assert!(
        recorded[0].contains("127.0.0.1"),
        "first check should be for the request URL"
    );
}

/// SEC-003: The fetcher connects to the IP returned by the SSRF checker
/// (DNS pinning), not a re-resolved IP. When the checker pins a domain
/// name to a wrong IP, the fetch fails to connect — proving the pinned IP
/// is used instead of real DNS resolution.
#[tokio::test]
async fn fetcher_pins_ssrf_checker_ip() {
    let response = http_response("200 OK", &[("Content-Type", "text/plain")], b"ok");
    let addr = start_mock_server(response).await;

    // WHY: use a domain name so reqwest applies resolve_to_addrs (DNS
    // pinning). IP literals skip pinning because there is no DNS to rebind.
    let fetcher = HttpFetcher::with_checker(WrongIpChecker).timeout(2);
    let url = format!("http://test.local:{}/sub", addr.port());
    let result = fetcher.fetch(&url, None).await;

    assert!(
        result.is_err(),
        "fetch should fail when checker pins domain to wrong IP (10.0.0.1)"
    );
}

// ---------------------------------------------------------------------------
// SEC-004: redirect to internal address rejected at each hop
// ---------------------------------------------------------------------------

/// SEC-004: A redirect to an internal address is rejected by the SSRF
/// checker on the redirect hop, before any connection to the internal
/// target is attempted.
#[tokio::test]
async fn redirect_to_internal_rejected() {
    let redirect_response = b"HTTP/1.1 301 Moved Permanently\r\n\
         Location: http://10.0.0.1/internal\r\n\
         Content-Length: 0\r\n\
         \r\n"
        .to_vec();
    let addr = start_mock_server(redirect_response).await;

    let calls = Arc::new(Mutex::new(Vec::new()));
    let checker = RecordingSelectiveChecker {
        calls: calls.clone(),
        blocked: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))],
    };
    let fetcher = HttpFetcher::with_checker(checker).timeout(5);
    let url = format!("http://127.0.0.1:{}/sub", addr.port());
    let result = fetcher.fetch(&url, None).await;

    assert!(
        matches!(result, Err(FetchError::Ssrf(_))),
        "redirect to internal should be rejected by SSRF, got {result:?}"
    );

    let recorded = calls.lock().expect("mutex").clone();
    assert_eq!(
        recorded.len(),
        2,
        "SSRF checker should be called for initial + redirect hop"
    );
    assert!(
        recorded[0].contains("127.0.0.1"),
        "first check is for the initial URL"
    );
    assert!(
        recorded[1].contains("10.0.0.1"),
        "second check is for the redirect target"
    );
}
