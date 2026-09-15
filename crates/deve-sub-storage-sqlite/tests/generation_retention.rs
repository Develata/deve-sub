#![allow(clippy::expect_used)]
//! OUT-014: live subscription fallback survives other selectors and version pins.

use deve_sub_domain::{GenerationCacheEntry, GenerationCacheRepository};
use deve_sub_kernel::{GenerationCacheId, TemplateId};
use deve_sub_storage_sqlite::SqliteGenerationCacheRepository;

#[path = "generation_retention/source_withdrawal.rs"]
mod source_withdrawal;

struct Fixture {
    pool: sqlx::SqlitePool,
    cache: SqliteGenerationCacheRepository,
    template: TemplateId,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}?mode=rwc",
            directory.path().join("db").display()
        ))
        .await
        .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrate");
        let template = TemplateId::new();
        sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES ('fixture-owner', 'fixture', 'fixture-hash', 'admin')")
            .execute(&pool).await.expect("user");
        sqlx::query("INSERT INTO templates (id, name) VALUES (?, 'fixture-template')")
            .bind(template.to_string())
            .execute(&pool)
            .await
            .expect("template");
        Self {
            cache: SqliteGenerationCacheRepository::new(pool.clone()),
            pool,
            template,
            _directory: directory,
        }
    }

    async fn subscribe(&self, name: &str, selection: &str, pin: Option<i64>) {
        sqlx::query("INSERT INTO subscriptions (id, name, slug, owner_id, template_id, template_version_pin, profile, node_selection, token_id) VALUES (?, ?, ?, 'fixture-owner', ?, ?, 'mihomo', ?, 'fixture-token')")
            .bind(name).bind(name).bind(name).bind(self.template.to_string()).bind(pin).bind(selection)
            .execute(&self.pool).await.expect("subscription");
    }

    async fn store(&self, selection: &str, version: u64, mode: &str) -> GenerationCacheEntry {
        let id = GenerationCacheId::new();
        let entry = GenerationCacheEntry {
            id,
            template_id: self.template,
            template_version: version,
            profile: "mihomo".into(),
            mode: mode.into(),
            selection_mode: "fixed".into(),
            selection_payload: selection.into(),
            pool_revision: 1,
            cache_key: id.to_string(),
            content: format!("fixture-{id}"),
            is_active: false,
        };
        self.cache.store(&entry).await.expect("store");
        entry
    }

    async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_cache")
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }
}

#[tokio::test]
async fn out014_retention_preserves_each_live_selection_and_reclaims_orphans() {
    let fixture = Fixture::new().await;
    let mut entries = Vec::new();
    for index in 0..12 {
        let selection = format!(r#"{{"mode":"fixed","nodeIds":["fixture-{index}"]}}"#);
        fixture
            .subscribe(&format!("sub-{index}"), &selection, None)
            .await;
        entries.push(fixture.store(&selection, 1, "lenient").await);
    }
    for entry in &entries {
        assert!(
            fixture
                .cache
                .find_by_key(&entry.cache_key)
                .await
                .expect("lookup")
                .is_some(),
            "another subscription must not evict this last-good result"
        );
    }
    for _ in 0..30 {
        fixture
            .store(&entries[0].selection_payload, 1, "lenient")
            .await;
    }
    assert_eq!(
        fixture.count().await,
        20,
        "12 protected results plus 8 extra entries"
    );
    sqlx::query("UPDATE subscriptions SET node_selection = ? WHERE id = 'sub-11'")
        .bind(&entries[1].selection_payload)
        .execute(&fixture.pool)
        .await
        .expect("edit selection");
    sqlx::query("UPDATE subscriptions SET enabled = 0 WHERE id = 'sub-1'")
        .execute(&fixture.pool)
        .await
        .expect("temporarily disable subscription");
    for _ in 0..12 {
        fixture
            .store(&entries[0].selection_payload, 1, "lenient")
            .await;
    }
    assert_eq!(
        fixture.count().await,
        19,
        "shared selection needs only one protected result"
    );
    assert!(
        fixture
            .cache
            .find_by_key(&entries[11].cache_key)
            .await
            .expect("obsolete selection")
            .is_none()
    );
    assert!(
        fixture
            .cache
            .find_by_key(&entries[1].cache_key)
            .await
            .expect("disabled subscription")
            .is_some()
    );
    sqlx::query("DELETE FROM subscriptions")
        .execute(&fixture.pool)
        .await
        .expect("delete subscriptions");
    fixture.store("{}", 1, "lenient").await;
    assert_eq!(
        fixture.count().await,
        8,
        "deleted subscription shapes become reclaimable"
    );
}

#[tokio::test]
async fn out014_retention_preserves_pins_and_lenient_fallback_separately() {
    let fixture = Fixture::new().await;
    let selection = r#"{"mode":"fixed","nodeIds":[]}"#;
    fixture.subscribe("pinned", selection, Some(1)).await;
    fixture.subscribe("tracking", selection, None).await;
    let pinned = fixture.store(selection, 1, "lenient").await;
    let tracking = fixture.store(selection, 2, "lenient").await;
    let active = fixture.store("{}", 1, "strict").await;
    fixture
        .cache
        .activate(fixture.template, "mihomo", active.id)
        .await
        .expect("activate");
    for _ in 0..20 {
        fixture.store(selection, 3, "strict").await;
    }
    for entry in [&pinned, &tracking, &active] {
        assert!(
            fixture
                .cache
                .find_by_key(&entry.cache_key)
                .await
                .expect("lookup")
                .is_some()
        );
    }
    assert_eq!(
        fixture.count().await,
        11,
        "2 live fallbacks, 1 active, 8 extra entries"
    );
    assert_eq!(
        fixture
            .cache
            .find_latest(
                fixture.template,
                "mihomo",
                "fixed",
                selection,
                Some(1),
                "lenient"
            )
            .await
            .expect("pinned lookup")
            .expect("pinned fallback")
            .id,
        pinned.id
    );
    assert_eq!(
        fixture
            .cache
            .find_latest(
                fixture.template,
                "mihomo",
                "fixed",
                selection,
                None,
                "lenient"
            )
            .await
            .expect("tracking lookup")
            .expect("tracking fallback")
            .id,
        tracking.id
    );
}
