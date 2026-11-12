#![allow(clippy::expect_used)]
//! Cache payload confidentiality must hold for the actual SQLite snapshot.

use std::sync::Arc;

use deve_sub_domain::{GenerationCacheEntry, GenerationCacheRepository};
use deve_sub_kernel::{GenerationCacheId, TemplateId};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{SqliteConfig, SqliteGenerationCacheRepository, create_pool};

#[path = "generation_cache_encryption/migration.rs"]
mod migration;
#[path = "generation_cache_encryption/semantics.rs"]
mod semantics;

const CANARY: &str = "reserved-fixture-cache-password-829e9d";

struct Fixture {
    directory: tempfile::TempDir,
    pool: sqlx::SqlitePool,
    key: Arc<MasterKey>,
    repository: SqliteGenerationCacheRepository,
    template: TemplateId,
}

impl Fixture {
    async fn new(schema: i64) -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let pool = create_pool(&SqliteConfig::new(directory.path().join("live.db")))
            .await
            .expect("pool");
        let mut migrator = sqlx::migrate!("../../migrations");
        migrator.migrations = migrator
            .migrations
            .iter()
            .filter(|m| m.version <= schema)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        migrator.run(&pool).await.expect("schema");
        let template = TemplateId::new();
        sqlx::query("INSERT INTO templates (id, name) VALUES (?, 'cache confidentiality fixture')")
            .bind(template.to_string())
            .execute(&pool)
            .await
            .expect("template");
        let key = Arc::new(MasterKey::from_bytes(&[0x42; 32]));
        let repository = SqliteGenerationCacheRepository::new(pool.clone(), Arc::clone(&key));
        Self {
            directory,
            pool,
            key,
            repository,
            template,
        }
    }

    fn entry(&self) -> GenerationCacheEntry {
        let id = GenerationCacheId::new();
        let mut entry = GenerationCacheEntry {
            id,
            template_id: self.template,
            template_version: 3,
            profile: "mihomo".into(),
            mode: "lenient".into(),
            selection_mode: "fixed".into(),
            selection_payload: format!(r#"{{"mode":"fixed","nodeIds":["{id}"]}}"#),
            pool_revision: 7,
            cache_key: format!("cache-encryption-fixture-{id}"),
            content: format!("proxies:\n  - password: {CANARY}\n"),
            is_active: true,
        };
        rekey(&mut entry);
        entry
    }

    async fn snapshot(&self, name: &str) -> std::path::PathBuf {
        let snapshot = self.directory.path().join(name);
        sqlx::query("VACUUM INTO ?")
            .bind(snapshot.to_str().expect("path"))
            .execute(&self.pool)
            .await
            .expect("consistent backup");
        snapshot
    }

    async fn encrypted(&self, id: GenerationCacheId) -> String {
        sqlx::query_scalar("SELECT content_encrypted FROM generation_cache WHERE id = ?")
            .bind(id.to_string())
            .fetch_one(&self.pool)
            .await
            .expect("envelope")
    }
}

fn assert_no_canary(path: &std::path::Path) {
    let bytes = std::fs::read(path).expect("snapshot");
    assert!(
        !bytes
            .windows(CANARY.len())
            .any(|part| part == CANARY.as_bytes()),
        "a generation-cache credential must not be readable in a database backup"
    );
}

#[tokio::test]
async fn cache_content_does_not_leak_into_database_backup() {
    let f = Fixture::new(i64::MAX).await;
    let entry = f.entry();
    f.repository.store(&entry).await.expect("store");
    assert_eq!(
        f.repository
            .find_by_key(&entry.cache_key)
            .await
            .expect("cache lookup")
            .expect("cache hit")
            .content,
        entry.content
    );
    assert_no_canary(&f.snapshot("snapshot.db").await);
    let envelope = f.encrypted(entry.id).await;
    assert!(envelope.starts_with("v2:"));
    assert!(!envelope.contains(CANARY));
    let legacy: String = sqlx::query_scalar("SELECT content_legacy FROM generation_cache")
        .fetch_one(&f.pool)
        .await
        .expect("legacy column");
    assert!(legacy.is_empty());
}

#[tokio::test]
async fn wrong_key_cannot_read_write_or_demote_last_good_output() {
    let f = Fixture::new(i64::MAX).await;
    let good = f.entry();
    f.repository.store(&good).await.expect("last good");
    let mut next = f.entry();
    next.is_active = false;
    f.repository.store(&next).await.expect("candidate");
    let wrong = SqliteGenerationCacheRepository::new(
        f.pool.clone(),
        Arc::new(MasterKey::from_bytes(&[0x99; 32])),
    );
    assert!(wrong.find_by_key(&good.cache_key).await.is_err());
    assert!(wrong.find_active(f.template, "mihomo").await.is_err());
    assert!(
        wrong
            .find_latest(
                f.template,
                "mihomo",
                "fixed",
                &good.selection_payload,
                None,
                "lenient"
            )
            .await
            .is_err()
    );
    assert!(wrong.activate(f.template, "mihomo", next.id).await.is_err());
    let mut rejected = f.entry();
    rejected.is_active = false;
    assert!(wrong.store(&rejected).await.is_err());
    assert!(
        f.repository
            .find_by_key(&rejected.cache_key)
            .await
            .expect("rejected write")
            .is_none()
    );
    assert_eq!(
        f.repository
            .find_active(f.template, "mihomo")
            .await
            .expect("active")
            .expect("last good")
            .id,
        good.id
    );
}

#[tokio::test]
async fn tampered_and_relocated_envelopes_never_become_plaintext_or_active() {
    let f = Fixture::new(i64::MAX).await;
    let good = f.entry();
    f.repository.store(&good).await.expect("last good");
    let mut next = f.entry();
    next.is_active = false;
    f.repository.store(&next).await.expect("candidate");
    let original = f.encrypted(next.id).await;
    assert_ne!(original, f.encrypted(good.id).await, "nonce must be fresh");
    for value in [format!("v2:{CANARY}:invalid"), f.encrypted(good.id).await] {
        sqlx::query("UPDATE generation_cache SET content_encrypted = ? WHERE id = ?")
            .bind(value)
            .bind(next.id.to_string())
            .execute(&f.pool)
            .await
            .expect("tamper fixture");
        let err = f
            .repository
            .find_by_key(&next.cache_key)
            .await
            .expect_err("fail closed");
        assert!(!err.to_string().contains(CANARY));
        assert!(
            f.repository
                .activate(f.template, "mihomo", next.id)
                .await
                .is_err()
        );
        assert_eq!(
            f.repository
                .find_active(f.template, "mihomo")
                .await
                .expect("active")
                .expect("last good")
                .id,
            good.id
        );
    }
    sqlx::query("UPDATE generation_cache SET content_encrypted = ? WHERE id = ?")
        .bind(original)
        .bind(next.id.to_string())
        .execute(&f.pool)
        .await
        .expect("recover fixture");
    f.repository
        .activate(f.template, "mihomo", next.id)
        .await
        .expect("recovered candidate");
}

#[tokio::test]
async fn schema_rejects_plaintext_reintroduction() {
    let f = Fixture::new(i64::MAX).await;
    let entry = f.entry();
    f.repository.store(&entry).await.expect("store");
    for sql in [
        "UPDATE generation_cache SET content_encrypted = NULL",
        "UPDATE generation_cache SET content_encrypted = 'plaintext'",
        "UPDATE generation_cache SET content_legacy = 'plaintext'",
    ] {
        assert!(sqlx::query(sql).execute(&f.pool).await.is_err());
    }
    assert!(sqlx::query(
        "INSERT INTO generation_cache (id, template_id, template_version, profile, selection_mode, \
         selection_payload, pool_revision, cache_key, content_legacy) \
         VALUES ('rejected', ?, 1, 'mihomo', 'fixed', '{}', 0, 'rejected', 'plaintext')",
    ).bind(f.template.to_string()).execute(&f.pool).await.is_err());
    assert_eq!(
        f.repository
            .find_by_key(&entry.cache_key)
            .await
            .expect("read")
            .expect("last good")
            .content,
        entry.content
    );
}

fn rekey(entry: &mut GenerationCacheEntry) {
    entry.cache_key = deve_sub_domain::CacheKeyParams {
        template_id: entry.template_id,
        template_version: entry.template_version,
        profile: &entry.profile,
        mode: entry.mode.parse().expect("fixture mode"),
        selection_mode: &entry.selection_mode,
        selection_payload: &entry.selection_payload,
        pool_revision: deve_sub_kernel::Revision::new(entry.pool_revision),
    }
    .compute_key();
}
