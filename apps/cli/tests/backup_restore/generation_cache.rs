//! Actual CLI archives must not expose cached node credentials.
use super::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deve_sub_domain::GenerationCacheRepository;
use deve_sub_kernel::{GenerationCacheId, TemplateId};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::SqliteGenerationCacheRepository;

const CACHE_CANARY: &str = "reserved-archive-cache-password-3b7fc4";
const CACHE_KEY: &str = "reserved-archive-cache-key";

struct CacheFixture {
    directory: tempfile::TempDir,
    database: PathBuf,
    key_path: PathBuf,
    wrong_key_path: PathBuf,
    id: GenerationCacheId,
    template: TemplateId,
}

impl CacheFixture {
    async fn legacy() -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("legacy.db");
        setup_db_schema(&database, 28).await;
        let key_path = directory.path().join("existing.key");
        let wrong_key_path = directory.path().join("wrong.key");
        std::fs::write(&key_path, [0x42; 32]).expect("fixture key");
        std::fs::write(&wrong_key_path, [0x99; 32]).expect("wrong fixture key");
        let id = GenerationCacheId::new();
        let template = TemplateId::new();
        let f = Self {
            directory,
            database,
            key_path,
            wrong_key_path,
            id,
            template,
        };
        let pool = f.pool(&f.database).await;
        let key = MasterKey::load(&f.key_path).expect("key");
        deve_sub_storage_sqlite::ensure_key_binding(
            &pool,
            &key.fingerprint().expect("fingerprint"),
        )
        .await
        .expect("binding");
        sqlx::query("INSERT INTO templates (id, name) VALUES (?, 'archive-cache-fixture')")
            .bind(template.to_string())
            .execute(&pool)
            .await
            .expect("template");
        sqlx::query("INSERT INTO generation_cache (id, template_id, template_version, profile, mode, selection_mode, selection_payload, pool_revision, cache_key, content, is_active) VALUES (?, ?, 4, 'mihomo', 'lenient', 'fixed', '{}', 7, ?, ?, 1)")
            .bind(id.to_string()).bind(template.to_string()).bind(CACHE_KEY)
            .bind(format!("proxies:\n  - password: {CACHE_CANARY}\n"))
            .execute(&pool).await.expect("legacy cache");
        pool.close().await;
        f
    }

    async fn pool(&self, path: &Path) -> sqlx::SqlitePool {
        sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
            .await
            .expect("pool")
    }

    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(BIN)
            .args(args)
            .current_dir(self.directory.path())
            .env_remove("DEVE_SUB_KEY_PATH")
            .env_remove("DEVE_SUB_CONFIG")
            .env_remove("DEVE_SUB_DB_PATH")
            .output()
            .expect("CLI")
    }

    async fn verify_cache(&self, path: &Path) {
        let pool = self.pool(path).await;
        let repository = SqliteGenerationCacheRepository::new(
            pool.clone(),
            Arc::new(MasterKey::load(&self.key_path).expect("key")),
        );
        assert!(
            repository
                .find_by_key(CACHE_KEY)
                .await
                .expect("historical direct")
                .is_none()
        );
        assert!(
            repository
                .find_active(self.template, "mihomo")
                .await
                .expect("historical active")
                .is_none()
        );
        let metadata: (String, i64, i64, i64, i64) = sqlx::query_as("SELECT cache_key, template_version, pool_revision, is_active, semantics_version FROM generation_cache WHERE id = ?")
            .bind(self.id.to_string()).fetch_one(&pool).await.expect("retained history");
        assert_eq!(metadata, (CACHE_KEY.into(), 4, 7, 1, 0));
        let raw: (String, String) = sqlx::query_as(
            "SELECT content_legacy, content_encrypted FROM generation_cache WHERE id = ?",
        )
        .bind(self.id.to_string())
        .fetch_one(&pool)
        .await
        .expect("raw ciphertext");
        assert!(raw.0.is_empty());
        assert!(raw.1.starts_with("v2:"));
        assert!(!raw.1.contains(CACHE_CANARY));
        pool.close().await;
    }
}

fn path(path: &Path) -> &str {
    path.to_str().expect("UTF-8 fixture path")
}

fn succeeded(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_migrate_archive_restore_keeps_key_and_last_good_output() {
    let f = CacheFixture::legacy().await;
    let archive = f.directory.path().join("backup.tar");
    assert!(
        !f.command(&[
            "backup",
            "--db-path",
            path(&f.database),
            "--output",
            path(&archive)
        ])
        .status
        .success()
    );
    assert!(
        !archive.exists(),
        "plaintext backup is refused before publication"
    );
    let rejected = f.command(&[
        "migrate",
        "--db-path",
        path(&f.database),
        "--key-path",
        path(&f.wrong_key_path),
    ]);
    assert!(!rejected.status.success());
    let pool = f.pool(&f.database).await;
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .expect("unchanged schema");
    assert_eq!(version, 28, "wrong key must fail before schema changes");
    let legacy: String = sqlx::query_scalar("SELECT content FROM generation_cache")
        .fetch_one(&pool)
        .await
        .expect("recoverable legacy");
    assert!(legacy.contains(CACHE_CANARY));
    pool.close().await;
    let serve = f.command(&[
        "serve",
        "--db-path",
        path(&f.database),
        "--key-path",
        path(&f.key_path),
        "--headless",
    ]);
    assert!(!serve.status.success());
    // Pending encryption must fail at startup before any socket is bound.
    assert!(String::from_utf8_lossy(&serve.stderr).contains("migrat"));
    succeeded(&f.command(&[
        "migrate",
        "--db-path",
        path(&f.database),
        "--key-path",
        path(&f.key_path),
    ]));
    f.verify_cache(&f.database).await;
    assert!(
        !f.command(&[
            "backup",
            "--db-path",
            path(&f.database),
            "--output",
            path(&archive),
            "--key-path",
            path(&f.wrong_key_path)
        ])
        .status
        .success()
    );
    assert!(!archive.exists());
    // A bound database backup requires the matching original key (cp01).
    succeeded(&f.command(&[
        "backup",
        "--db-path",
        path(&f.database),
        "--output",
        path(&archive),
        "--key-path",
        path(&f.key_path),
    ]));
    let bytes = std::fs::read(&archive).expect("archive");
    assert!(
        !bytes
            .windows(CACHE_CANARY.len())
            .any(|bytes| bytes == CACHE_CANARY.as_bytes())
    );
    let snapshot = read_tar_entry(&archive, "database.sqlite").expect("actual archived SQLite");
    assert!(
        !snapshot
            .windows(CACHE_CANARY.len())
            .any(|bytes| bytes == CACHE_CANARY.as_bytes())
    );
    let manifest: BackupManifest =
        serde_json::from_slice(&read_tar_entry(&archive, "manifest.json").expect("manifest"))
            .expect("manifest");
    assert_eq!(
        manifest.master_key_fingerprint,
        Some(
            MasterKey::load(&f.key_path)
                .expect("key")
                .fingerprint()
                .expect("fingerprint")
        )
    );

    let target = f.directory.path().join("existing.db");
    setup_db(&target).await;
    let before = std::fs::read(&target).expect("target");
    let wrong = f.command(&[
        "restore",
        "--input",
        path(&archive),
        "--db-path",
        path(&target),
        "--key-path",
        path(&f.wrong_key_path),
    ]);
    assert!(!wrong.status.success());
    assert_eq!(std::fs::read(&target).expect("untouched target"), before);
    succeeded(&f.command(&[
        "restore",
        "--input",
        path(&archive),
        "--db-path",
        path(&target),
        "--key-path",
        path(&f.key_path),
    ]));
    f.verify_cache(&target).await;
    assert_eq!(
        std::fs::read(&f.key_path).expect("key continuity"),
        vec![0x42; 32]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_restore_legacy_archive_backfills_staging_and_tamper_never_swaps_target() {
    let f = CacheFixture::legacy().await;
    let snapshot = f.directory.path().join("historical.sqlite");
    let pool = f.pool(&f.database).await;
    sqlx::query("VACUUM INTO ?")
        .bind(path(&snapshot))
        .execute(&pool)
        .await
        .expect("old snapshot");
    pool.close().await;
    let archive = f.directory.path().join("historical.tar");
    let manifest = serde_json::json!({
        "version": 1, "schema_version": 28,
        "created_at": "2026-01-01T00:00:00Z",
        "row_counts": {"users": 1, "templates": 1, "generation_cache": 1},
        "master_key_fingerprint": MasterKey::load(&f.key_path).expect("key").fingerprint().expect("fp"),
    });
    let mut builder = tar::Builder::new(std::fs::File::create(&archive).expect("archive"));
    builder
        .append_path_with_name(&snapshot, "database.sqlite")
        .expect("snapshot entry");
    let content = serde_json::to_vec(&manifest).expect("manifest");
    let mut header = tar::Header::new_gnu();
    header.set_path("manifest.json").expect("manifest path");
    header.set_size(content.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append(&header, content.as_slice())
        .expect("manifest entry");
    builder.finish().expect("finish archive");
    drop(builder);
    let old_bytes = std::fs::read(&archive).expect("historical archive");
    assert!(
        old_bytes
            .windows(CACHE_CANARY.len())
            .any(|part| part == CACHE_CANARY.as_bytes()),
        "existing historical archives remain sensitive"
    );
    let restored = f.directory.path().join("restored.db");
    assert!(
        !f.command(&[
            "restore",
            "--input",
            path(&archive),
            "--db-path",
            path(&restored)
        ])
        .status
        .success()
    );
    assert!(!restored.exists());
    succeeded(&f.command(&[
        "restore",
        "--input",
        path(&archive),
        "--db-path",
        path(&restored),
        "--key-path",
        path(&f.key_path),
    ]));
    f.verify_cache(&restored).await;
    assert_eq!(
        std::fs::read(&archive).expect("historical archive unchanged"),
        old_bytes
    );

    let pool = f.pool(&restored).await;
    sqlx::query("UPDATE generation_cache SET content_encrypted = 'v2:tampered:invalid'")
        .execute(&pool)
        .await
        .expect("tamper");
    pool.close().await;
    let corrupt = f.directory.path().join("tampered.tar");
    succeeded(&f.command(&[
        "backup",
        "--db-path",
        path(&restored),
        "--output",
        path(&corrupt),
        "--key-path",
        path(&f.key_path),
    ]));
    let target = f.directory.path().join("untouched.db");
    setup_db(&target).await;
    let before = std::fs::read(&target).expect("existing target");
    let result = f.command(&[
        "restore",
        "--input",
        path(&corrupt),
        "--db-path",
        path(&target),
        "--key-path",
        path(&f.key_path),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("authentication failed"));
    assert_eq!(
        std::fs::read(&target).expect("existing target preserved"),
        before
    );
}
