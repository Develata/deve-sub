#![allow(clippy::expect_used)]

//! SRC-001: source form edits preserve secrets and share cache invalidation.

use std::sync::Arc;

use deve_sub_domain::{
    PoolMetaRepository, Source, SourceConfigUpdate, SourceRepository, SourceType,
};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{SqlitePoolMetaRepository, SqliteSourceRepository};

struct TestDb {
    pool: sqlx::SqlitePool,
    repo: SqliteSourceRepository,
    _dir: tempfile::TempDir,
}

impl TestDb {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("directory");
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("source.db").display(),
        ))
        .await
        .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");
        let repo = SqliteSourceRepository::new_with_key(
            pool.clone(),
            Arc::new(MasterKey::from_bytes(&[0x42; 32])),
        );
        Self {
            pool,
            repo,
            _dir: dir,
        }
    }

    async fn revision(&self) -> u64 {
        SqlitePoolMetaRepository::new(self.pool.clone())
            .get_revision()
            .await
            .expect("revision")
            .value()
    }

    async fn ciphertexts(&self, source: &Source) -> (String, Option<String>) {
        sqlx::query_as("SELECT url_encrypted, headers_encrypted FROM sources WHERE id = ?")
            .bind(source.id.to_string())
            .fetch_one(&self.pool)
            .await
            .expect("ciphertexts")
    }
}

fn source() -> Source {
    let mut source = Source::new(
        "original",
        SourceType::Auto,
        "https://source.example/private/sub?token=fixture-old".into(),
    );
    source.headers_encrypted = Some(r#"{"X-Fixture":"private-value"}"#.into());
    source
}

fn edit(source: &Source, url: Option<String>) -> SourceConfigUpdate {
    SourceConfigUpdate {
        id: source.id,
        name: "renamed".into(),
        source_type: source.source_type,
        url,
        auto_update: source.auto_update,
        update_interval_secs: source.update_interval_secs,
        enabled: source.enabled,
        keep_on_fail: source.keep_on_fail,
        filter_rules: source.filter_rules.clone(),
    }
}

#[tokio::test]
async fn src001_omitted_url_preserves_exact_secret_ciphertexts_and_current_transport() {
    let db = TestDb::new().await;
    let source = source();
    db.repo.create(&source).await.expect("source");
    let before = db.ciphertexts(&source).await;
    let revision = db.revision().await;
    let updated = db
        .repo
        .update_config(&edit(&source, None))
        .await
        .expect("edit");
    assert_eq!(updated.name, "renamed");
    assert!(updated.url == source.url, "URL retained");
    assert!(
        updated.headers_encrypted == source.headers_encrypted,
        "headers retained"
    );
    assert_eq!(updated.http_method, source.http_method);
    assert_eq!(db.ciphertexts(&source).await, before);
    assert_eq!(db.revision().await, revision + 1);

    // An edit created from an old view still retains the latest URL and headers.
    let mut changed = source.clone();
    changed.url = "https://source.example/new?token=fixture-new".into();
    changed.headers_encrypted = Some(r#"{"X-Fixture":"new-value"}"#.into());
    db.repo
        .update(&changed)
        .await
        .expect("concurrent credential change");
    let current_ciphertexts = db.ciphertexts(&source).await;
    let updated = db
        .repo
        .update_config(&edit(&source, None))
        .await
        .expect("old form edit");
    assert!(updated.url == changed.url, "latest URL retained");
    assert!(
        updated.headers_encrypted == changed.headers_encrypted,
        "latest headers retained"
    );
    assert_eq!(db.ciphertexts(&source).await, current_ciphertexts);
}

#[tokio::test]
async fn src001_concurrent_omission_never_reverts_explicit_url_replacement() {
    let db = TestDb::new().await;
    let source = source();
    db.repo.create(&source).await.expect("source");
    let original_headers = db.ciphertexts(&source).await.1;
    for i in 0..12 {
        let replacement = format!("https://source.example/sub?token=fixture-{i}");
        let replace = edit(&source, Some(replacement.clone()));
        let preserve = edit(&source, None);
        let (replaced, preserved) =
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                tokio::join!(
                    db.repo.update_config(&replace),
                    db.repo.update_config(&preserve)
                )
            })
            .await
            .expect("bounded concurrent writes");
        assert!(
            replaced.expect("replace").url == replacement,
            "returned replacement"
        );
        preserved.expect("preserve");
        let stored = db
            .repo
            .find_by_id(source.id)
            .await
            .expect("source")
            .expect("exists");
        assert!(
            stored.url == replacement,
            "omission cannot revert committed URL"
        );
        assert_eq!(db.ciphertexts(&source).await.1, original_headers);
    }
}

#[tokio::test]
async fn src001_config_update_and_revision_failure_roll_back_together() {
    let db = TestDb::new().await;
    let source = source();
    db.repo.create(&source).await.expect("source");
    // Compare persisted state: storage intentionally normalizes timestamp
    // precision, which is unrelated to the update's rollback behavior.
    let source = db
        .repo
        .find_by_id(source.id)
        .await
        .expect("source")
        .expect("exists");
    let before = db.ciphertexts(&source).await;
    let revision = db.revision().await;
    sqlx::query("CREATE TRIGGER fail_revision BEFORE UPDATE ON pool_meta BEGIN SELECT RAISE(ABORT, 'fixture failure'); END")
        .execute(&db.pool).await.expect("failure trigger");
    let result = db
        .repo
        .update_config(&edit(
            &source,
            Some("https://source.example/replacement".into()),
        ))
        .await;
    assert!(result.is_err());
    let stored = db
        .repo
        .find_by_id(source.id)
        .await
        .expect("source")
        .expect("exists");
    assert!(
        stored == source,
        "failed revision must roll back the whole edit"
    );
    assert_eq!(db.ciphertexts(&source).await, before);
    assert_eq!(db.revision().await, revision);
}
