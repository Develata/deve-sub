//! DEPLOY-001: preserve data while repairing legacy source-deletion orphans.
#![allow(clippy::expect_used)]
use deve_sub_storage_sqlite::{SqliteConfig, create_pool};

#[tokio::test]
async fn migration0028_repairs_orphans_and_backup_can_reupgrade() {
    let directory = tempfile::tempdir().expect("directory");
    let pool = create_pool(&SqliteConfig::new(directory.path().join("old.db")))
        .await
        .expect("pool");
    let mut old = sqlx::migrate!("../../migrations");
    old.migrations = old
        .migrations
        .iter()
        .filter(|m| m.version <= 27)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    old.run(&pool).await.expect("old schema");
    sqlx::query("INSERT INTO sources (id, name, source_type, url_encrypted) VALUES ('s', 'fixture', 'uri_list', 'fixture-envelope')")
        .execute(&pool).await.expect("source");
    for (id, label, port) in [
        ("orphan", "", 443),
        ("manual", "manual", 444),
        ("bound", "", 445),
    ] {
        sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port, source_label, revision, authentication_json_encrypted) VALUES (?, 'trojan', 'fixture.example.com', ?, ?, 7, 'fixture-envelope')")
            .bind(id).bind(port).bind(label).execute(&pool).await.expect("node");
    }
    sqlx::query(
        "INSERT INTO node_source_bindings (id, source_id, node_id) VALUES ('b', 's', 'bound')",
    )
    .execute(&pool)
    .await
    .expect("binding");
    sqlx::query("INSERT INTO node_overrides (id, node_id, display_name) VALUES ('o', 'orphan', 'keep-name')")
        .execute(&pool).await.expect("override");
    sqlx::query("INSERT INTO tags (id, name) VALUES ('t', 'keep-tag')")
        .execute(&pool)
        .await
        .expect("tag");
    sqlx::query("INSERT INTO node_tags (node_id, tag_id) VALUES ('orphan', 't')")
        .execute(&pool)
        .await
        .expect("membership");
    sqlx::query("UPDATE pool_meta SET revision = 42")
        .execute(&pool)
        .await
        .expect("revision");
    let backup = directory.path().join("backup.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .expect("backup");
    let restored = create_pool(&SqliteConfig::new(&backup))
        .await
        .expect("restore");
    let before: i64 =
        sqlx::query_scalar("SELECT missing_from_source FROM nodes WHERE id = 'orphan'")
            .fetch_one(&restored)
            .await
            .expect("old backup");
    assert_eq!(before, 0);
    for db in [&pool, &restored] {
        sqlx::migrate!("../../migrations")
            .run(db)
            .await
            .expect("upgrade");
        sqlx::migrate!("../../migrations")
            .run(db)
            .await
            .expect("idempotent upgrade");
        let state: (i64, i64) =
            sqlx::query_as("SELECT revision, withdrawal_revision FROM pool_meta")
                .fetch_one(db)
                .await
                .expect("floor");
        assert_eq!(state, (43, 43));
        let nodes: Vec<(String, i64, i64, String)> = sqlx::query_as("SELECT id, missing_from_source, revision, authentication_json_encrypted FROM nodes ORDER BY id")
            .fetch_all(db).await.expect("nodes");
        assert_eq!(
            nodes,
            vec![
                ("bound".into(), 0, 7, "fixture-envelope".into()),
                ("manual".into(), 0, 7, "fixture-envelope".into()),
                ("orphan".into(), 1, 8, "fixture-envelope".into())
            ]
        );
        let name: String =
            sqlx::query_scalar("SELECT display_name FROM node_overrides WHERE node_id = 'orphan'")
                .fetch_one(db)
                .await
                .expect("override retained");
        assert_eq!(name, "keep-name");
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM node_tags WHERE node_id = 'orphan'")
                .fetch_one(db)
                .await
                .expect("tags retained");
        assert_eq!(count, 1);
    }
}
