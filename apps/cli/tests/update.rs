#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Integration tests for `deve-sub update` (UPDATE-001/002).
//!
//! UPDATE-001: successful update — binary swapped, health check passes.
//! UPDATE-002: failed update — health check fails, binary rolled back.

use std::io::Read;
use std::process::{Command, Stdio};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

/// Start a mock HTTP server that serves a manifest, binary, checksums, and
/// a health endpoint. Returns (base_url, health_status).
async fn start_mock_server(new_binary: Vec<u8>, health_ok: bool, version: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    let binary_hash: String = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&new_binary);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };

    let asset_name = if std::env::consts::ARCH == "x86_64" {
        "deve-sub-linux-amd64"
    } else {
        "deve-sub-linux-arm64"
    };

    let manifest = serde_json::json!({
        "tag_name": format!("v{version}"),
        "assets": [
            {
                "name": asset_name,
                "browser_download_url": format!("http://{addr}/{asset_name}")
            },
            {
                "name": "checksums.txt",
                "browser_download_url": format!("http://{addr}/checksums.txt")
            }
        ]
    });
    let manifest_bytes = serde_json::to_vec(&manifest).expect("manifest");
    let checksums = format!("{binary_hash}  {asset_name}\n");
    let checksums_bytes = checksums.into_bytes();

    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let req = String::from_utf8_lossy(&buf);

            let (status, body, content_type) = if req.starts_with("GET /manifest") {
                (200, manifest_bytes.clone(), "application/json")
            } else if req.starts_with(&format!("GET /{asset_name}")) {
                (200, new_binary.clone(), "application/octet-stream")
            } else if req.starts_with("GET /checksums.txt") {
                (200, checksums_bytes.clone(), "text/plain")
            } else if req.starts_with("GET /health") {
                if health_ok {
                    (200, b"ok".to_vec(), "text/plain")
                } else {
                    (503, b"unhealthy".to_vec(), "text/plain")
                }
            } else {
                (404, b"not found".to_vec(), "text/plain")
            };

            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.write_all(&body).await;
            let _ = sock.flush().await;
        }
    });

    format!("http://{addr}")
}

/// Copy the current deve-sub binary to a temp path for testing.
fn copy_current_binary(dir: &std::path::Path) -> std::path::PathBuf {
    let dest = dir.join("deve-sub");
    std::fs::copy(std::env::current_exe().expect("exe"), &dest).expect("copy");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .expect("permissions");
    }
    dest
}

/// Read file contents for comparison.
fn read_file(path: &std::path::Path) -> Vec<u8> {
    let mut f = std::fs::File::open(path).expect("open");
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).expect("read");
    buf
}

#[tokio::test(flavor = "multi_thread")]
async fn update001_successful_update_no_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary_path = copy_current_binary(dir.path());

    let current_version = env!("CARGO_PKG_VERSION");
    let new_binary = format!("#!/bin/sh\necho deve-sub {current_version}\n").into_bytes();
    let base_url = start_mock_server(new_binary.clone(), true, current_version).await;

    let output = Command::new(BIN)
        .args([
            "update",
            "--manifest-url",
            &format!("{base_url}/manifest"),
            "--binary-path",
            binary_path.to_str().unwrap(),
            "--health-url",
            &format!("{base_url}/health"),
            "--no-restart",
            "--force",
            "--timeout",
            "5",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "update should succeed: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status
    );

    let updated_bytes = read_file(&binary_path);
    assert_eq!(
        updated_bytes, new_binary,
        "binary should be replaced with the new version"
    );
    assert!(
        dir.path().join("deve-sub.bak").exists(),
        "backup must be KEPT under --no-restart (liveness not confirmed)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn update002_bad_binary_rejected_before_swap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary_path = copy_current_binary(dir.path());
    let original_bytes = read_file(&binary_path);

    let new_binary = b"#!/bin/sh\necho fake-new-binary\n".to_vec();
    let current_version = env!("CARGO_PKG_VERSION");
    let base_url = start_mock_server(new_binary.clone(), false, current_version).await;

    let status = Command::new(BIN)
        .args([
            "update",
            "--manifest-url",
            &format!("{base_url}/manifest"),
            "--binary-path",
            binary_path.to_str().unwrap(),
            "--health-url",
            &format!("{base_url}/health"),
            "--no-restart",
            "--force",
            "--timeout",
            "5",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .expect("spawn");

    assert!(
        !status.success(),
        "update must fail when the binary reports the wrong version: {status:?}"
    );

    let restored_bytes = read_file(&binary_path);
    assert_eq!(
        restored_bytes, original_bytes,
        "live binary must be untouched — bad binary rejected before swap"
    );
    assert!(
        !dir.path().join("deve-sub.bak").exists(),
        "no backup should exist — swap never happened"
    );
}
