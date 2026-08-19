#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Integration tests for P0 CLI acceptance cases.
//!
//! CLI-001: Headless startup — server runs without web UI, API accessible.
//! CLI-002: stdin import — nodes imported from stdin.
//! CLI-003: stdout URI export — `node list --format uri` outputs one URI per line.
//! CLI-004: JSON output — `node list --format json` outputs valid JSON.
//! CLI-005: doctor — checks database, directories, network, version.
//! NODE-002: file import — nodes imported from a file path.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use serde::Deserialize;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

const TROJAN_URI: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#Test";

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Create a fully-migrated SQLite database at `db_path` and a master key at
/// `key_path`, then insert one row into `users` so row counts are non-
/// trivial. Node import/list tests require a master key because node
/// credentials are encrypted at rest (migration 0015, ADR-0007).
async fn setup_db(db_path: &std::path::Path, key_path: &std::path::Path) {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePool::connect(&url).await.expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrations");
    pool.close().await;
    std::fs::write(key_path, [0x42u8; 32]).expect("write master key");
}

#[tokio::test(flavor = "multi_thread")]
async fn cli005_doctor_checks_all_sections() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;

    let output = Command::new(BIN)
        .args(["doctor", "--db-path", db_path.to_str().unwrap()])
        .output()
        .expect("spawn");

    assert!(
        output.status.success(),
        "doctor failed: {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[1/4] Version"), "missing version section");
    assert!(
        stdout.contains("[2/4] Database"),
        "missing database section"
    );
    assert!(
        stdout.contains("[3/4] Directories"),
        "missing directories section"
    );
    assert!(stdout.contains("[4/4] Network"), "missing network section");
    assert!(stdout.contains("schema version:"), "missing schema version");
    assert!(
        stdout.contains("Diagnostics complete."),
        "missing completion line"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cli002_stdin_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;

    let mut child = Command::new(BIN)
        .args([
            "node",
            "import",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(TROJAN_URI.as_bytes()).expect("write");
    }

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "import failed: {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Import completed:"),
        "missing import summary"
    );
    assert!(
        stdout.contains("new:       1"),
        "expected 1 new node, got: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn node002_file_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;
    let input_path = dir.path().join("nodes.txt");
    std::fs::write(&input_path, TROJAN_URI).expect("write input");

    let output = Command::new(BIN)
        .args([
            "node",
            "import",
            "--input",
            input_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn");

    assert!(
        output.status.success(),
        "import failed: {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Import completed:"),
        "missing import summary"
    );
    assert!(
        stdout.contains("new:       1"),
        "expected 1 new node, got: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cli003_stdout_uri_export() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;

    let mut import = Command::new(BIN)
        .args([
            "node",
            "import",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = import.stdin.take().expect("stdin");
        stdin.write_all(TROJAN_URI.as_bytes()).expect("write");
    }
    let import_output = import.wait_with_output().expect("wait");
    assert!(
        import_output.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&import_output.stderr)
    );

    let output = Command::new(BIN)
        .args([
            "node",
            "list",
            "--format",
            "uri",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn");

    assert!(output.status.success(), "list failed: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "expected at least one URI line");
    assert!(
        lines[0].starts_with("trojan://"),
        "expected trojan URI, got: {}",
        lines[0]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cli004_json_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;

    let mut import = Command::new(BIN)
        .args([
            "node",
            "import",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = import.stdin.take().expect("stdin");
        stdin.write_all(TROJAN_URI.as_bytes()).expect("write");
    }
    let import_output = import.wait_with_output().expect("wait");
    assert!(
        import_output.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&import_output.stderr)
    );

    let output = Command::new(BIN)
        .args([
            "node",
            "list",
            "--format",
            "json",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn");

    assert!(output.status.success(), "list failed: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);

    #[derive(Debug, Deserialize)]
    struct NodeJson {
        #[allow(dead_code)]
        id: String,
        protocol: String,
        host: String,
        port: u16,
        #[allow(dead_code)]
        region: Option<String>,
        active: bool,
        missing_from_source: bool,
    }

    let nodes: Vec<NodeJson> = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(nodes.len(), 1, "expected 1 node, got: {stdout}");
    assert_eq!(nodes[0].protocol, "Trojan");
    assert_eq!(nodes[0].host, "example.com");
    assert_eq!(nodes[0].port, 443);
    assert!(nodes[0].active);
    assert!(!nodes[0].missing_from_source);
}

#[tokio::test(flavor = "multi_thread")]
async fn cli001_headless_startup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let config = serde_json::json!({
        "security": {
            "master_key_path": key_path.to_str().unwrap(),
            "allow_master_key_generation": true,
            "cookie_secure": false
        }
    });
    let config_path = dir.path().join("config.json");
    std::fs::write(&config_path, config.to_string()).expect("write config");

    let child = Command::new(BIN)
        .args([
            "serve",
            "--config",
            config_path.to_str().unwrap(),
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--db-path",
            db_path.to_str().unwrap(),
            "--headless",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let _guard = ChildGuard(child);

    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut health_ok = false;
    while std::time::Instant::now() < deadline {
        if let Ok(mut stream) =
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500))
        {
            let _ = stream.write_all(b"GET /health/live HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let mut buf = [0u8; 256];
            if let Ok(n) = stream.read(&mut buf) {
                let response = String::from_utf8_lossy(&buf[..n]);
                if response.starts_with("HTTP/1.1 200") {
                    health_ok = true;
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert!(health_ok, "server did not become healthy within 10s");

    if let Ok(mut stream) =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500))
    {
        let _ = stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let mut buf = [0u8; 256];
        if let Ok(n) = stream.read(&mut buf) {
            let response = String::from_utf8_lossy(&buf[..n]);
            assert!(
                response.starts_with("HTTP/1.1 404"),
                "expected 404 for / in headless mode, got: {}",
                response.lines().next().unwrap_or("")
            );
        }
    }
}

/// CLI-006: a second `serve` started against an already-locked database
/// must fail fast (bounded timeout, not hang forever) with a clear error
/// naming the lock and the holder. Regression guard for DS-AUD-B05.
#[tokio::test(flavor = "multi_thread")]
async fn cli006_second_serve_fails_fast_when_one_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_path = dir.path().join("master.key");
    setup_db(&db_path, &key_path).await;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port1 = listener.local_addr().expect("addr").port();
    drop(listener);

    let config = serde_json::json!({
        "security": {
            "master_key_path": key_path.to_str().unwrap(),
            "allow_master_key_generation": true,
            "cookie_secure": false
        }
    });
    let config_path = dir.path().join("config.json");
    std::fs::write(&config_path, config.to_string()).expect("write config");

    let first = Command::new(BIN)
        .args([
            "serve",
            "--config",
            config_path.to_str().unwrap(),
            "--bind",
            &format!("127.0.0.1:{port1}"),
            "--db-path",
            db_path.to_str().unwrap(),
            "--headless",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first serve");
    let _first_guard = ChildGuard(first);

    let addr1: std::net::SocketAddr = format!("127.0.0.1:{port1}").parse().expect("addr");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut healthy = false;
    while std::time::Instant::now() < deadline {
        if let Ok(mut stream) =
            std::net::TcpStream::connect_timeout(&addr1, std::time::Duration::from_millis(500))
        {
            let _ = stream.write_all(b"GET /health/live HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let mut buf = [0u8; 256];
            if let Ok(n) = stream.read(&mut buf)
                && String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200")
            {
                healthy = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert!(healthy, "first serve did not become healthy within 10s");

    // Second serve on a different port (so a bind failure cannot confound
    // the test) but the SAME db_path — must fail on the lock, not hang.
    let listener2 = std::net::TcpListener::bind("127.0.0.1:0").expect("bind2");
    let port2 = listener2.local_addr().expect("addr").port();
    drop(listener2);

    let start = std::time::Instant::now();
    let output = Command::new(BIN)
        .args([
            "serve",
            "--config",
            config_path.to_str().unwrap(),
            "--bind",
            &format!("127.0.0.1:{port2}"),
            "--db-path",
            db_path.to_str().unwrap(),
            "--headless",
        ])
        // Bounded wait — the old blocking `lock_exclusive` would hang here
        // until the test harness killed the process.
        .output()
        .expect("spawn second serve");
    let elapsed = start.elapsed();

    assert!(
        !output.status.success(),
        "second serve must fail, got status {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("another deve-sub process holds the lock"),
        "stderr must name the held lock, got: {stderr}"
    );
    // B-05 contract: must fail within 5s (the serve-side timeout).
    // Generous upper bound to absorb scheduling jitter on CI.
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "second serve took {elapsed:?} — lock acquisition is not bounded"
    );
}

/// DS-AUD-B01: `key init` creates a fresh key when neither key nor DB exists.
#[test]
fn key_init_creates_key_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let key_path = dir.path().join("master.key");
    let db_path = dir.path().join("deve-sub.db");

    let output = Command::new(BIN)
        .args([
            "key",
            "init",
            "--key-path",
            key_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn key init");

    assert!(
        output.status.success(),
        "key init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(key_path.exists(), "key file must be created");
    let bytes = std::fs::read(&key_path).expect("read key");
    assert_eq!(bytes.len(), 32, "key must be exactly 32 bytes");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("master key initialized"),
        "stdout must confirm init, got: {stdout}"
    );
}

/// DS-AUD-B01: `key init` refuses to overwrite an existing key file.
#[test]
fn key_init_refuses_when_key_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let key_path = dir.path().join("master.key");
    let db_path = dir.path().join("deve-sub.db");
    std::fs::write(&key_path, [0xAAu8; 32]).expect("pre-create key");

    let output = Command::new(BIN)
        .args([
            "key",
            "init",
            "--key-path",
            key_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn key init");

    assert!(
        !output.status.success(),
        "key init must fail when key exists, got status {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists") && stderr.contains("refusing to overwrite"),
        "stderr must refuse overwrite, got: {stderr}"
    );
    // Original key must be untouched.
    let bytes = std::fs::read(&key_path).expect("read key");
    assert_eq!(bytes, [0xAAu8; 32], "existing key must not be modified");
}

/// DS-AUD-B01: `key init` refuses when DB exists but key is missing —
/// generating a new key would silently invalidate all encrypted columns.
#[test]
fn key_init_refuses_when_db_exists_without_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let key_path = dir.path().join("master.key");
    let db_path = dir.path().join("deve-sub.db");
    // Create a non-empty "DB" file — the check is Path::exists(), not
    // schema validation, so any file triggers the guard.
    std::fs::write(&db_path, b"sqlite3 fake db content").expect("pre-create db");

    let output = Command::new(BIN)
        .args([
            "key",
            "init",
            "--key-path",
            key_path.to_str().unwrap(),
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn key init");

    assert!(
        !output.status.success(),
        "key init must fail when DB exists without key"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("database already exists") && stderr.contains("silently invalidate"),
        "stderr must explain the fail-closed reason, got: {stderr}"
    );
    // Key file must NOT be created.
    assert!(!key_path.exists(), "key must not be created when DB exists");
}

/// DS-AUD-B01: `key init` uses the default `data/deve-sub.db` for the
/// existence check when `--db-path` is omitted.
#[test]
fn key_init_default_db_path_when_omitted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let key_path = dir.path().join("master.key");
    // Create the default-relative DB at the cwd so the guard fires.
    let default_db_dir = dir.path().join("data");
    std::fs::create_dir_all(&default_db_dir).expect("mkdir");
    std::fs::write(default_db_dir.join("deve-sub.db"), b"fake").expect("db");

    let output = Command::new(BIN)
        .args(["key", "init", "--key-path", key_path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("spawn key init");

    assert!(
        !output.status.success(),
        "key init must detect the default-path DB and refuse"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("database already exists"),
        "stderr must report the DB-without-key guard, got: {stderr}"
    );
}

/// DS-AUD-B07: a CLI command must NOT silently generate a new key on a host
/// with an existing DB. The DB is bound to the first key that opens it; a
/// subsequent command with a different key must fail closed with a
/// fingerprint mismatch, not generate a fresh key (which would split the key
/// epoch and make old ciphertext unreadable).
#[tokio::test(flavor = "multi_thread")]
async fn b07_wrong_key_fails_closed_not_silently_generated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let key_a = dir.path().join("master.key");
    let key_b = dir.path().join("other.key");
    setup_db(&db_path, &key_a).await;

    // Bind key A to the DB via the first keyed command (node import).
    let mut child = Command::new(BIN)
        .args([
            "node",
            "import",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_a.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first import");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(TROJAN_URI.as_bytes()).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "first import must succeed and bind key A, got {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // Write a different key B and retry the same command. Must fail closed.
    std::fs::write(&key_b, [0x99u8; 32]).expect("write key B");
    let mut child = Command::new(BIN)
        .args([
            "node",
            "import",
            "--db-path",
            db_path.to_str().unwrap(),
            "--key-path",
            key_b.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn second import");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(TROJAN_URI.as_bytes()).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    assert!(
        !output.status.success(),
        "second import with a different key must fail, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fingerprint mismatch"),
        "stderr must report the fingerprint mismatch, got: {stderr}"
    );
}
