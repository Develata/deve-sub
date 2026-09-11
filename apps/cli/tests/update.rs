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
async fn start_mock_server(
    new_binary: Vec<u8>,
    health_ok: bool,
    version: &str,
    signature_assets: usize,
) -> String {
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

    let mut manifest = serde_json::json!({
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
    for name in ["deve-sub-manifest.json", "deve-sub-manifest.json.sig"]
        .into_iter()
        .take(signature_assets)
    {
        manifest["assets"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name": name,
                "browser_download_url": format!("http://{addr}/{name}")
            }));
    }
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
            } else if req.starts_with("GET /deve-sub-manifest.json.sig") {
                (200, vec![0; 64], "application/octet-stream")
            } else if req.starts_with("GET /deve-sub-manifest.json") {
                (200, b"untrusted manifest".to_vec(), "application/json")
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
    std::fs::copy(BIN, &dest).expect("copy");
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
    let base_url = start_mock_server(new_binary.clone(), true, current_version, 0).await;

    let output = Command::new(BIN)
        .args([
            "update",
            "--binary-only",
            "--manifest-url",
            &format!("{base_url}/manifest"),
            "--binary-path",
            binary_path.to_str().unwrap(),
            "--health-url",
            &format!("{base_url}/health"),
            "--no-restart",
            "--force",
            "--allow-unsigned",
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
    let base_url = start_mock_server(new_binary.clone(), false, current_version, 0).await;

    let status = Command::new(BIN)
        .args([
            "update",
            "--binary-only",
            "--manifest-url",
            &format!("{base_url}/manifest"),
            "--binary-path",
            binary_path.to_str().unwrap(),
            "--health-url",
            &format!("{base_url}/health"),
            "--no-restart",
            "--force",
            "--allow-unsigned",
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

#[tokio::test(flavor = "multi_thread")]
async fn update001_unsigned_release_rejected_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary_path = copy_current_binary(dir.path());
    let original_bytes = read_file(&binary_path);
    let base_url = start_mock_server(b"untrusted binary".to_vec(), true, "0.1.0", 0).await;
    let output = Command::new(BIN)
        .args([
            "update",
            "--binary-only",
            "--manifest-url",
            &format!("{base_url}/manifest"),
            "--binary-path",
            binary_path.to_str().unwrap(),
            "--force",
            "--no-restart",
        ])
        .output()
        .expect("spawn");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("release is unsigned"), "{error}");
    assert_eq!(read_file(&binary_path), original_bytes);
    assert!(!dir.path().join("deve-sub.bak").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn update001_unsigned_opt_in_never_bypasses_invalid_signature() {
    for (assets, expected) in [
        (1, "partial signed manifest"),
        (2, "signature verification failed"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary_path = copy_current_binary(dir.path());
        let original_bytes = read_file(&binary_path);
        let base_url = start_mock_server(b"untrusted binary".to_vec(), true, "0.1.0", assets).await;
        let output = Command::new(BIN)
            .args([
                "update",
                "--binary-only",
                "--manifest-url",
                &format!("{base_url}/manifest"),
                "--binary-path",
                binary_path.to_str().unwrap(),
                "--force",
                "--no-restart",
                "--allow-unsigned",
            ])
            .output()
            .expect("spawn");
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert_eq!(read_file(&binary_path), original_bytes);
    }
}

#[test]
fn native_web_update_refuses_before_network_or_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("deve-sub");
    std::fs::write(&binary, b"unchanged").unwrap();
    let output = Command::new(BIN)
        .args([
            "update",
            "--binary-path",
            binary.to_str().unwrap(),
            "--manifest-url",
            "http://127.0.0.1:1/unreachable",
            "--force",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("--binary-only"), "{error}");
    assert_eq!(std::fs::read(&binary).unwrap(), b"unchanged");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn broken_config_never_falls_back_to_headless_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("invalid.toml");
    std::fs::write(&config, "[invalid").unwrap();
    let output = Command::new(BIN)
        .args([
            "update",
            "--config",
            config.to_str().unwrap(),
            "--manifest-url",
            "http://127.0.0.1:1/unreachable",
            "--force",
        ])
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("config"), "{error}");
    assert!(!error.contains("manifest fetch"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn force_is_not_implicit_downgrade_permission() {
    let base = start_mock_server(Vec::new(), true, "0.0.0", 0).await;
    let output = Command::new(BIN)
        .args([
            "update",
            "--binary-only",
            "--force",
            "--allow-unsigned",
            "--manifest-url",
            &format!("{base}/manifest"),
        ])
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("--allow-downgrade"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_headless_config_reaches_authentication_without_binary_only() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    std::fs::write(&config, r#"{"server":{"serve_web":false}}"#).unwrap();
    let base = start_mock_server(Vec::new(), true, env!("CARGO_PKG_VERSION"), 0).await;
    let output = Command::new(BIN)
        .args([
            "update",
            "--config",
            config.to_str().unwrap(),
            "--force",
            "--binary-path",
            dir.path().join("binary").to_str().unwrap(),
            "--manifest-url",
            &format!("{base}/manifest"),
        ])
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("release is unsigned"), "{error}");
}
