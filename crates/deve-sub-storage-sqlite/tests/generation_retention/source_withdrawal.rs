//! GEN-015/OUT-014: a late old write cannot restore a withdrawn cache.
use super::*;
use deve_sub_domain::{PoolMetaRepository, Source, SourceRepository, SourceType};
use deve_sub_storage_sqlite::{SqlitePoolMetaRepository, SqliteSourceRepository};

#[tokio::test]
async fn gen015_source_deletion_fences_all_cache_reads_and_late_writes() {
    let fixture = Fixture::new().await;
    let sources = SqliteSourceRepository::new_with_key(
        fixture.pool.clone(),
        std::sync::Arc::new(deve_sub_security::MasterKey::from_bytes(&[0x42; 32])),
    );
    let revision = SqlitePoolMetaRepository::new(fixture.pool.clone());
    revision.bump_revision().await.expect("initial pool");
    let source = Source::new(
        "A",
        SourceType::UriList,
        "https://source.example.com/sub".into(),
    );
    sources.create(&source).await.expect("source");
    let old = fixture.store("{}", 1, "lenient").await;
    fixture
        .cache
        .activate(fixture.template, "mihomo", old.id)
        .await
        .expect("active");
    sources.delete(source.id).await.expect("delete source");
    assert!(
        fixture
            .cache
            .find_by_key(&old.cache_key)
            .await
            .expect("direct")
            .is_none()
    );
    assert!(
        fixture
            .cache
            .find_active(fixture.template, "mihomo")
            .await
            .expect("active")
            .is_none()
    );
    assert!(
        fixture
            .cache
            .find_latest(fixture.template, "mihomo", "fixed", "{}", None, "lenient")
            .await
            .expect("fallback")
            .is_none()
    );
    let mut current = old.clone();
    current.id = GenerationCacheId::new();
    current.cache_key = current.id.to_string();
    current.pool_revision = revision.get_revision().await.expect("revision").value();
    current.content = "current-without-A".into();
    fixture.cache.store(&current).await.expect("store current");
    fixture
        .cache
        .activate(fixture.template, "mihomo", current.id)
        .await
        .expect("activate current");
    // The old generation acquired its inputs before deletion and finished last.
    let mut late = old.clone();
    late.id = GenerationCacheId::new();
    late.cache_key = late.id.to_string();
    assert!(
        fixture.cache.store(&late).await.is_err(),
        "late old snapshot must be fenced"
    );
    assert!(
        fixture
            .cache
            .activate(fixture.template, "mihomo", old.id)
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .cache
            .find_active(fixture.template, "mihomo")
            .await
            .expect("active")
            .expect("current")
            .id,
        current.id
    );
    assert_eq!(
        fixture
            .cache
            .find_latest(fixture.template, "mihomo", "fixed", "{}", None, "lenient")
            .await
            .expect("fallback")
            .expect("current")
            .id,
        current.id
    );
    assert!(
        fixture
            .cache
            .find_by_key(&late.cache_key)
            .await
            .expect("late")
            .is_none()
    );
    fixture.subscribe("post-withdrawal", "{}", None).await;
    for i in 0..16 {
        let mut churn = current.clone();
        churn.id = GenerationCacheId::new();
        churn.cache_key = churn.id.to_string();
        churn.selection_payload = format!("churn-{i}");
        fixture
            .cache
            .store(&churn)
            .await
            .expect("current revision churn");
        fixture
            .cache
            .activate(fixture.template, "mihomo", churn.id)
            .await
            .expect("new active");
    }
    assert_eq!(
        fixture
            .cache
            .find_latest(fixture.template, "mihomo", "fixed", "{}", None, "lenient")
            .await
            .expect("fallback")
            .expect("protected current output")
            .id,
        current.id
    );
    assert!(
        fixture.count().await <= 10,
        "active plus protected fallback plus eight history entries"
    );
    let reopened = SqliteGenerationCacheRepository::new(fixture.pool.clone());
    assert!(
        reopened
            .find_by_key(&old.cache_key)
            .await
            .expect("persistent floor")
            .is_none()
    );
}
