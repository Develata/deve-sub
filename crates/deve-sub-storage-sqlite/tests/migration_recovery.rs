//! Recovery test for migration 0002_initial.sql (constraint #13).
//!
//! Verifies that:
//! 1. The migration applies cleanly to a fresh database.
//! 2. All expected tables, columns, and indexes exist after migration.
//! 3. The migration is idempotent (re-running does not fail).
//! 4. A pre-migration backup can be restored (forward-only rollback strategy).
//!
//! See docs/plan/13-storage.md §"Migration policy" and acceptance DEPLOY-001.

#![allow(clippy::expect_used)]

use std::path::PathBuf;

use sqlx::sqlite::SqlitePool;

/// Expected tables created by migration 0002.
const EXPECTED_TABLES: &[&str] = &["users", "sessions", "audit_log", "outbox_event"];

/// Expected indexes created by migration 0002.
const EXPECTED_INDEXES: &[&str] = &[
    "idx_sessions_user_expires",
    "idx_audit_log_actor_created",
    "idx_outbox_event_unprocessed",
];

/// Expected columns for each table.
const EXPECTED_COLUMNS: &[(&str, &[&str])] = &[
    (
        "users",
        &[
            "id",
            "username",
            "password_hash",
            "role",
            "enabled",
            "expires_at",
            "traffic_quota",
            "created_at",
        ],
    ),
    (
        "sessions",
        &[
            "id",
            "user_id",
            "token_hash",
            "created_at",
            "expires_at",
            "revoked",
        ],
    ),
    (
        "audit_log",
        &[
            "id",
            "actor_id",
            "action",
            "target_type",
            "target_id",
            "details_json",
            "created_at",
        ],
    ),
    (
        "outbox_event",
        &[
            "id",
            "aggregate_type",
            "aggregate_id",
            "event_type",
            "payload_json",
            "created_at",
            "processed_at",
        ],
    ),
];

async fn create_test_pool(db_path: &PathBuf) -> SqlitePool {
    let config = deve_sub_storage_sqlite::SqliteConfig::new(db_path).max_connections(1);
    deve_sub_storage_sqlite::create_pool(&config)
        .await
        .expect("failed to connect to test database")
}

async fn run_migrations(pool: &SqlitePool) {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .expect("failed to run migrations");
}

async fn get_table_names(pool: &SqlitePool) -> Vec<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .fetch_all(pool)
            .await
            .expect("failed to query tables");
    rows.into_iter().map(|(n,)| n).collect()
}

async fn get_index_names(pool: &SqlitePool) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .expect("failed to query indexes");
    rows.into_iter().map(|(n,)| n).collect()
}

async fn get_columns(pool: &SqlitePool, table: &str) -> Vec<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await
            .expect("failed to query table info");
    rows.into_iter().map(|(n,)| n).collect()
}

#[tokio::test]
async fn migration_0002_applies_and_schema_is_correct() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    // Verify all expected tables exist.
    let tables = get_table_names(&pool).await;
    for expected in EXPECTED_TABLES {
        assert!(
            tables.contains(&expected.to_string()),
            "expected table '{expected}' not found, tables: {tables:?}"
        );
    }

    // Verify all expected indexes exist.
    let indexes = get_index_names(&pool).await;
    for expected in EXPECTED_INDEXES {
        assert!(
            indexes.contains(&expected.to_string()),
            "expected index '{expected}' not found, indexes: {indexes:?}"
        );
    }

    // Verify columns for each table.
    for (table, expected_cols) in EXPECTED_COLUMNS {
        let columns = get_columns(&pool, table).await;
        for expected_col in *expected_cols {
            assert!(
                columns.contains(&expected_col.to_string()),
                "expected column '{expected_col}' in table '{table}' not found, columns: {columns:?}"
            );
        }
    }

    // Verify WAL mode is active.
    let mode: (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .expect("failed to query journal mode");
    assert_eq!(mode.0.to_lowercase(), "wal", "journal_mode should be WAL");

    // Verify foreign keys are ON.
    let fk: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .expect("failed to query foreign_keys");
    assert_eq!(fk.0, 1, "foreign_keys should be ON");

    pool.close().await;
}

#[tokio::test]
async fn migration_0002_is_idempotent() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;

    // First run.
    run_migrations(&pool).await;

    // Second run — should not fail.
    run_migrations(&pool).await;

    // Verify tables still exist.
    let tables = get_table_names(&pool).await;
    for expected in EXPECTED_TABLES {
        assert!(tables.contains(&expected.to_string()));
    }

    pool.close().await;
}

#[tokio::test]
async fn migration_0002_backup_restore_preserves_schema() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    // Apply migration.
    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    // Insert a test row to verify data survives backup/restore.
    let user_id = "01J0TESTUSER00000000000001";
    sqlx::query("INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind("test_user")
        .bind("hash_placeholder")
        .execute(&pool)
        .await
        .expect("failed to insert test user");

    // Create a backup copy of the database file.
    let backup_path = db_path.with_extension("db.bak");
    std::fs::copy(&db_path, &backup_path).expect("failed to copy database");

    // Simulate a failure: drop the database file and restore from backup.
    drop(pool);
    std::fs::remove_file(&db_path).expect("failed to remove database");
    std::fs::copy(&backup_path, &db_path).expect("failed to restore database");

    // Verify the restored database has the data and schema.
    let pool = create_test_pool(&db_path).await;

    let row: (String, String) = sqlx::query_as("SELECT id, username FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("failed to query restored user");

    assert_eq!(row.0, user_id);
    assert_eq!(row.1, "test_user");

    // Verify schema is intact.
    let tables = get_table_names(&pool).await;
    for expected in EXPECTED_TABLES {
        assert!(tables.contains(&expected.to_string()));
    }

    // Clean up WAL and SHM files.
    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(backup_path);
}
