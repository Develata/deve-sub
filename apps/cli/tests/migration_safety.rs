#![allow(clippy::expect_used)]
//! BACKUP-004: preflight and durable rollback precede all schema/payload writes.
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Output,
};
use tokio::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");
const CANARY: &str = "TEST_LEGACY_CACHE_PASSWORD";

struct Fixture {
    directory: tempfile::TempDir,
    db: PathBuf,
    key: PathBuf,
    wrong: PathBuf,
}
impl Fixture {
    async fn new(version: i64, cache: bool) -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let f = Self {
            db: directory.path().join("live.sqlite"),
            key: directory.path().join("original.key"),
            wrong: directory.path().join("wrong.key"),
            directory,
        };
        std::fs::write(&f.key, [0x42; 32]).expect("key");
        std::fs::write(&f.wrong, [0x99; 32]).expect("wrong key");
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", f.db.display()))
            .await
            .expect("pool");
        let mut migrator = sqlx::migrate!("../../migrations");
        migrator.migrations = migrator
            .migrations
            .iter()
            .filter(|m| m.version <= version)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        migrator.run(&pool).await.expect("fixture schema");
        sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES ('owner', 'reserved-owner', 'test', 'admin')").execute(&pool).await.expect("user");
        sqlx::query("INSERT INTO templates (id, name) VALUES ('01KZAAAAAAAAAAAAAAAAAAAAAA', 'reserved-template')")
            .execute(&pool)
            .await
            .expect("template");
        if cache {
            let fingerprint = deve_sub_security::MasterKey::load(&f.key)
                .expect("key")
                .fingerprint()
                .expect("fingerprint");
            sqlx::query("INSERT INTO key_metadata (id, current_key_fingerprint) VALUES (1, ?)")
                .bind(fingerprint)
                .execute(&pool)
                .await
                .expect("binding");
            sqlx::query("INSERT INTO generation_cache (id, template_id, template_version, profile, mode, selection_mode, selection_payload, pool_revision, cache_key, content, is_active) VALUES ('cache', '01KZAAAAAAAAAAAAAAAAAAAAAA', 1, 'mihomo', 'lenient', 'fixed', '{}', 0, 'legacy-key', ?, 1)").bind(CANARY).execute(&pool).await.expect("legacy cache");
        }
        pool.close().await;
        f
    }
    async fn pool(&self) -> sqlx::SqlitePool {
        sqlx::SqlitePool::connect(&format!("sqlite://{}", self.db.display()))
            .await
            .expect("pool")
    }
    async fn run(&self, command: &[&str], key: Option<&Path>, extra: &[&str]) -> Output {
        // Set only the child umask: parallel tests must not change process-wide
        // state, and a restrictive developer shell must not hide privacy bugs.
        #[cfg(unix)]
        let mut c = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "umask 000; exec \"$@\"", "migration-safety"])
                .arg(BIN);
            c
        };
        #[cfg(not(unix))]
        let mut c = Command::new(BIN);
        c.current_dir(self.directory.path())
            .env_remove("DEVE_SUB_KEY_PATH")
            .env_remove("DEVE_SUB_CONFIG")
            .env_remove("DEVE_SUB_DB_PATH")
            .args(command)
            .arg("--db-path")
            .arg(&self.db);
        if let Some(key) = key {
            c.arg("--key-path").arg(key);
        }
        c.args(extra).output().await.expect("CLI")
    }
    async fn assert_legacy(&self, version: i64) {
        let pool = self.pool().await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("version"),
            version
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT content FROM generation_cache WHERE id='cache'"
            )
            .fetch_one(&pool)
            .await
            .expect("original content"),
            CANARY
        );
        pool.close().await;
    }
    fn rollbacks(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.directory.path())
            .expect("entries")
            .map(|e| e.expect("entry").path())
            .filter(|p| {
                p.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .starts_with(".deve-sub-pre-migrate-")
            })
            .collect()
    }
}
fn success(out: &Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_wrong_key_and_backup_io_failure_leave_schema_unchanged() {
    let f = Fixture::new(28, true).await;
    for key in [None, Some(f.wrong.as_path())] {
        assert!(!f.run(&["migrate"], key, &[]).await.status.success());
        f.assert_legacy(28).await;
        assert!(f.rollbacks().is_empty());
    }
    let not_a_directory = f.directory.path().join("not-a-directory");
    std::fs::write(&not_a_directory, b"fixture").expect("file");
    assert!(
        !f.run(
            &["migrate"],
            Some(&f.key),
            &["--rollback-dir", not_a_directory.to_str().expect("path")]
        )
        .await
        .status
        .success()
    );
    f.assert_legacy(28).await;
    success(&f.run(&["migrate"], Some(&f.key), &[]).await);
    let recovery = f.rollbacks();
    assert_eq!(recovery.len(), 1);
    let recovery = &recovery[0];
    assert_eq!(
        std::fs::read(recovery.join("master.key")).expect("recovered original key"),
        vec![0x42; 32]
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(recovery.join("manifest.json")).expect("manifest"))
            .expect("JSON");
    let snapshot = std::fs::read(recovery.join("database.sqlite")).expect("snapshot");
    let digest: String = Sha256::digest(&snapshot)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(manifest["database_sha256"], digest);
    assert_eq!(manifest["schema_version"], 28);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(recovery)
                .expect("dir mode")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for n in ["database.sqlite", "master.key", "manifest.json"] {
            assert_eq!(
                std::fs::metadata(recovery.join(n))
                    .expect("mode")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    let rollback = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        recovery.join("database.sqlite").display()
    ))
    .await
    .expect("rollback open");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT content FROM generation_cache")
            .fetch_one(&rollback)
            .await
            .expect("recover old schema"),
        CANARY
    );
    rollback.close().await;
    success(&f.run(&["migrate"], Some(&f.key), &[]).await);
    assert_eq!(
        f.rollbacks().len(),
        1,
        "current-schema retry must not accumulate snapshots"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_staged_payload_conversion_keeps_live_database_and_rollback_pair() {
    let f = Fixture::new(28, true).await;
    let pool = f.pool().await;
    sqlx::query("CREATE TRIGGER injected_failure BEFORE UPDATE ON generation_cache BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END").execute(&pool).await.expect("fault");
    pool.close().await;
    assert!(
        !f.run(&["migrate"], Some(&f.key), &[])
            .await
            .status
            .success()
    );
    f.assert_legacy(28).await;
    let recovery = f.rollbacks();
    assert_eq!(recovery.len(), 1);
    assert_eq!(
        std::fs::read(recovery[0].join("master.key")).expect("original key retained"),
        vec![0x42; 32]
    );
    let pool = f.pool().await;
    sqlx::query("DROP TRIGGER injected_failure")
        .execute(&pool)
        .await
        .expect("repair write fault");
    pool.close().await;
    success(&f.run(&["migrate"], Some(&f.key), &[]).await);
    assert_eq!(
        f.rollbacks().len(),
        2,
        "previous rollback evidence is preserved"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn populated_pre0015_and_management_implicit_upgrade_preserve_original_fields() {
    let f = Fixture::new(14, false).await;
    let pool = f.pool().await;
    sqlx::query("INSERT INTO sources (id,name,url,headers_encrypted) VALUES ('source','source','https://source.example.test/TEST_TOKEN','TEST_HEADERS')").execute(&pool).await.expect("source");
    sqlx::query("INSERT INTO nodes (id,protocol_kind,host,port,authentication_json,tls_json,chain_json) VALUES ('old-node','trojan','node.example.test',443,'{\"password\":\"TEST_PASSWORD\"}','{\"pin\":\"TEST_PIN\"}','[\"old-node\"]')").execute(&pool).await.expect("node");
    sqlx::query("INSERT INTO node_overrides (id,node_id,enabled,display_name) VALUES ('override','old-node',0,'kept-name')").execute(&pool).await.expect("disabled override");
    pool.close().await;
    for command in [&["migrate"][..], &["template", "list"][..]] {
        assert!(!f.run(command, None, &[]).await.status.success());
        let pool = f.pool().await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("schema"),
            14
        );
        let row: (String, String) = sqlx::query_as("SELECT url,headers_encrypted FROM sources")
            .fetch_one(&pool)
            .await
            .expect("source fields");
        assert_eq!(
            row,
            (
                "https://source.example.test/TEST_TOKEN".into(),
                "TEST_HEADERS".into()
            )
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT authentication_json FROM nodes")
                .fetch_one(&pool)
                .await
                .expect("credential"),
            r#"{"password":"TEST_PASSWORD"}"#
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT enabled FROM node_overrides")
                .fetch_one(&pool)
                .await
                .expect("override"),
            0
        );
        pool.close().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn supplied_key_never_first_binds_unverified_existing_ciphertext() {
    let f = Fixture::new(28, false).await;
    let pool = f.pool().await;
    sqlx::query("INSERT INTO sources (id,name,url_encrypted) VALUES ('source','source','v2:unknown-existing:fixture')").execute(&pool).await.expect("unknown existing ciphertext");
    pool.close().await;
    let out = f.run(&["migrate"], Some(&f.key), &[]).await;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unbound legacy ciphertext"));
    let pool = f.pool().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM key_metadata")
            .fetch_one(&pool)
            .await
            .expect("binding"),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .expect("schema"),
        28
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_counter_and_pre_binding_hmac_state_require_original_key_verification() {
    for version in [17, 28] {
        let f = Fixture::new(version, false).await;
        let pool = f.pool().await;
        if version == 28 {
            sqlx::query("INSERT INTO probe_sources (id,kind,name,endpoint_url,auth_config,last_counter_snapshot) VALUES ('probe','D','reserved-probe','https://probe.example.test','', 'v2:existing-counter:fixture')").execute(&pool).await.expect("encrypted counter without auth");
        } else {
            sqlx::query("INSERT INTO sessions (id,user_id,token_hash,expires_at) VALUES ('session','owner','TEST_HMAC_DIGEST','2099-01-01T00:00:00Z')").execute(&pool).await.expect("legacy HMAC session");
        }
        pool.close().await;
        let out = f.run(&["migrate"], Some(&f.key), &[]).await;
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("original-key verification"));
        let pool = f.pool().await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("schema"),
            version
        );
        pool.close().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn traffic_projection_backfill_is_verified_without_rejecting_valid_new_rows() {
    let f = Fixture::new(23, false).await;
    let pool = f.pool().await;
    sqlx::query("INSERT INTO subscriptions (id,name,slug,owner_id,template_id,profile,node_selection,token_id) VALUES ('sub','sub','sub','owner','01KZAAAAAAAAAAAAAAAAAAAAAA','mihomo','{}','test-token')").execute(&pool).await.expect("subscription");
    sqlx::query("INSERT INTO subscription_traffic (id,subscription_id,source_kind,source_ref,upload,download,recorded_at) VALUES ('sample','sub','M','reserved-manual',13,29,'2026-01-01T00:00:00Z')").execute(&pool).await.expect("old sample");
    pool.close().await;
    success(&f.run(&["migrate"], None, &[]).await);
    let pool = f.pool().await;
    assert_eq!(sqlx::query_as::<_,(i64,i64)>("SELECT total_upload,total_download FROM traffic_daily_snapshots WHERE subscription_id='sub'").fetch_one(&pool).await.expect("daily backfill"),(13,29));
    assert_eq!(
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT upload,download FROM traffic_totals WHERE subscription_id='sub'"
        )
        .fetch_one(&pool)
        .await
        .expect("lifetime backfill"),
        (13, 29)
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn management_waiter_opens_published_database_after_acquiring_lock() {
    use fs2::FileExt;
    let old = Fixture::new(i64::MAX, false).await;
    let new = Fixture::new(i64::MAX, false).await;
    let pool = new.pool().await;
    sqlx::query("UPDATE templates SET name='newly-published-template'")
        .execute(&pool)
        .await
        .expect("replacement marker");
    pool.close().await;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(format!("{}.deve-sub.lock", old.db.display()))
        .expect("lock file");
    lock.lock_exclusive().expect("hold before connect");
    let mut child = Command::new(BIN)
        .args(["template", "list", "--db-path"])
        .arg(&old.db)
        .env_remove("DEVE_SUB_CONFIG")
        .env_remove("DEVE_SUB_KEY_PATH")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("waiting CLI");
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(
        child.try_wait().expect("wait state").is_none(),
        "management command must wait for the held sidecar"
    );
    std::fs::rename(&new.db, &old.db).expect("publish replacement while owning lock");
    drop(lock);
    let out = child.wait_with_output().await.expect("result");
    success(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("newly-published-template"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("reserved-template"));
}
