#![allow(clippy::expect_used)]

use deve_sub_storage_sqlite::{SqliteConfig, SqliteMaintenance, create_pool};

#[tokio::test]
async fn passive_checkpoint_recovers_after_pinned_reader() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wal.db");
    let pool = create_pool(&SqliteConfig::new(&path)).await.expect("pool");
    sqlx::query("CREATE TABLE payload (id INTEGER PRIMARY KEY, value BLOB)")
        .execute(&pool)
        .await
        .expect("schema");
    sqlx::query("INSERT INTO payload VALUES (1, zeroblob(65536))")
        .execute(&pool)
        .await
        .expect("seed");
    let maintenance = SqliteMaintenance::new(pool.clone(), &path);
    maintenance.checkpoint().await.expect("initial checkpoint");
    let mut reader = pool.begin().await.expect("reader");
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM payload")
        .fetch_one(&mut *reader)
        .await
        .expect("pin snapshot");
    for _ in 0..10 {
        sqlx::query("UPDATE payload SET value = randomblob(65536)")
            .execute(&pool)
            .await
            .expect("write");
    }
    let pinned = tokio::time::timeout(std::time::Duration::from_secs(2), maintenance.checkpoint())
        .await
        .expect("PASSIVE does not wait for reader")
        .expect("checkpoint");
    assert!(pinned.log_frames > pinned.checkpointed_frames);
    reader.rollback().await.expect("release snapshot");
    let recovered = maintenance
        .checkpoint()
        .await
        .expect("checkpoint after reader");
    assert_eq!(recovered.log_frames, recovered.checkpointed_frames);
    let envelope = recovered.wal_bytes.expect("WAL metadata");
    for _ in 0..100 {
        sqlx::query("UPDATE payload SET value = randomblob(65536)")
            .execute(&pool)
            .await
            .expect("write");
        let sample = maintenance.checkpoint().await.expect("checkpoint");
        assert_eq!(sample.log_frames, sample.checkpointed_frames);
        assert!(sample.wal_bytes.expect("WAL metadata") <= envelope);
    }
    assert!(recovered.database_bytes.expect("DB metadata") > 0);
    pool.close().await;
}
