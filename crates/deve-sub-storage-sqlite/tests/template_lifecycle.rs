//! GEN-004: deletion and rollback must serialize at the version write boundary.
#![allow(clippy::expect_used)]

use deve_sub_application::template::{CreateTemplateParams, create_template};
use deve_sub_domain::{TemplateError, TemplateVersionRepository};
use deve_sub_storage_sqlite::{SqliteTemplateRepository, SqliteTemplateVersionRepository};

#[tokio::test]
async fn rollback_waiting_for_delete_cannot_report_a_ghost_version() {
    let directory = tempfile::tempdir().expect("directory");
    let config = deve_sub_storage_sqlite::SqliteConfig::new(directory.path().join("test.db"));
    let pool = deve_sub_storage_sqlite::create_pool(&config)
        .await
        .expect("pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("migrate");
    let created = create_template(
        &SqliteTemplateRepository::new(pool.clone()),
        CreateTemplateParams {
            name: "fixture".into(),
            description: String::new(),
            spec_yaml: "rules: ['MATCH,PROXY']".into(),
        },
    )
    .await
    .expect("create");
    let repo = SqliteTemplateVersionRepository::new(pool.clone());
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .expect("delete transaction");
    sqlx::query("DELETE FROM templates WHERE id = ?")
        .bind(created.template.id.to_string())
        .execute(&mut *tx)
        .await
        .expect("pending delete");
    let mut rollback = Box::pin(repo.activate(created.version.id));
    // Poll the actual rollback while the write lock is held. It must remain
    // pending; after deletion commits, its target must be re-read as absent.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut rollback)
            .await
            .is_err()
    );
    tx.commit().await.expect("commit deletion");
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), rollback)
        .await
        .expect("finite wait");
    assert!(
        matches!(result, Err(TemplateError::VersionNotFound)),
        "{result:?}"
    );
    assert!(
        repo.find_active(created.template.id)
            .await
            .expect("lookup")
            .is_none()
    );
    pool.close().await;
}
