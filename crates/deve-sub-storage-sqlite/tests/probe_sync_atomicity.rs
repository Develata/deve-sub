//! Probe counters and deltas form one optimistic transaction.
#![allow(clippy::expect_used)]

use deve_sub_application::probe::{CreateProbeSourceParams, create_probe_source};
use deve_sub_domain::{
    ProbeError, ProbeSourceKind, ProbeSourceRepository, TrafficRecord, TrafficSourceKind,
};
use deve_sub_kernel::SubscriptionId;
use deve_sub_storage_sqlite::{SqliteConfig, SqliteProbeSourceRepository, create_pool};
use std::sync::Arc;

#[tokio::test]
async fn counter_and_deltas_rollback_together_and_only_one_revision_wins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pool = create_pool(&SqliteConfig::new(dir.path().join("db")))
        .await
        .expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrate");
    let sub_id = SubscriptionId::new();
    sqlx::raw_sql(
        "INSERT INTO users (id, username, password_hash) VALUES ('u', 'test', 'TEST_HASH');
        INSERT INTO templates (id, name) VALUES ('t', 'test');",
    )
    .execute(&pool)
    .await
    .expect("seed");
    sqlx::query("INSERT INTO subscriptions (id, name, slug, owner_id, template_id, profile, node_selection, token_id) VALUES (?, 's', 's', 'u', 't', 'mihomo', '{}', 'TEST_ID')")
        .bind(sub_id.to_string()).execute(&pool).await.expect("subscription");
    let repo = SqliteProbeSourceRepository::new_with_key(
        pool.clone(),
        Arc::new(deve_sub_security::MasterKey::from_bytes(&[0x42; 32])),
    );
    let mut source = create_probe_source(
        &repo,
        CreateProbeSourceParams {
            kind: ProbeSourceKind::Nezha,
            name: "test".into(),
            endpoint_url: "https://example.com".into(),
            auth_config: "TEST_TOKEN".into(),
            subscription_id: Some(sub_id),
        },
    )
    .await
    .expect("source");
    source.last_counter_snapshot = Some("TEST_COUNTER".into());
    let first = TrafficRecord::new(
        sub_id,
        TrafficSourceKind::Probe,
        10,
        20,
        "nezha:test".into(),
    );
    // The second duplicate fails after the first insert and counter update.
    assert!(
        repo.commit_sync(&source, &[first.clone(), first.clone()])
            .await
            .is_err()
    );
    let unchanged = repo
        .find_by_id(source.id)
        .await
        .expect("read")
        .expect("source");
    assert_eq!(unchanged.revision, 0);
    assert_eq!(unchanged.last_counter_snapshot, None);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM traffic_totals")
        .fetch_one(&pool)
        .await
        .expect("totals");
    assert_eq!(count, 0);
    let second = TrafficRecord::new(
        sub_id,
        TrafficSourceKind::Probe,
        10,
        20,
        "nezha:test".into(),
    );
    let a = [first];
    let b = [second];
    let (left, right) = tokio::join!(repo.commit_sync(&source, &a), repo.commit_sync(&source, &b));
    assert!(matches!(
        (&left, &right),
        (Ok(()), Err(ProbeError::Conflict)) | (Err(ProbeError::Conflict), Ok(()))
    ));
    let updated = repo
        .find_by_id(source.id)
        .await
        .expect("read")
        .expect("source");
    assert_eq!(updated.revision, 1);
    assert_eq!(
        updated.last_counter_snapshot.as_deref(),
        Some("TEST_COUNTER")
    );
    let total: i64 = sqlx::query_scalar("SELECT upload FROM traffic_totals")
        .fetch_one(&pool)
        .await
        .expect("total");
    assert_eq!(total, 10);
    assert!(
        matches!(repo.update(&source).await, Err(ProbeError::Conflict)),
        "stale edits cannot overwrite a newer counter"
    );
}
