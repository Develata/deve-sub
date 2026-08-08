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

/// Expected tables created by migration 0004.
const EXPECTED_TABLES_0004: &[&str] = &[
    "sources",
    "source_snapshots",
    "source_items",
    "nodes",
    "node_overrides",
    "node_source_bindings",
    "tags",
    "node_tags",
];

/// Expected indexes created by migration 0002.
const EXPECTED_INDEXES: &[&str] = &[
    "idx_sessions_user_expires",
    "idx_audit_log_actor_created",
    "idx_outbox_event_unprocessed",
];

/// Expected indexes created by migration 0004.
const EXPECTED_INDEXES_0004: &[&str] = &[
    "idx_snapshots_source_active",
    "idx_snapshots_source_version",
    "idx_snapshots_single_active",
    "idx_source_items_snapshot",
    "idx_nodes_dedup",
    "idx_bindings_node",
    "idx_bindings_source",
    "idx_node_tags_tag",
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

/// Recovery test for migration 0004 (constraint #13): sources/node-pool
/// tables, the partial unique dedup index, cascade deletes, and idempotency.
/// See docs/plan/milestones/M4-sources-and-node-pool.md.
#[tokio::test]
async fn migration_0004_applies_and_schema_is_correct() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let tables = get_table_names(&pool).await;
    for expected in EXPECTED_TABLES_0004 {
        assert!(
            tables.contains(&expected.to_string()),
            "expected table '{expected}' not found, tables: {tables:?}"
        );
    }

    let indexes = get_index_names(&pool).await;
    for expected in EXPECTED_INDEXES_0004 {
        assert!(
            indexes.contains(&expected.to_string()),
            "expected index '{expected}' not found, indexes: {indexes:?}"
        );
    }

    // WHY: idx_nodes_dedup is a partial unique index (WHERE missing_from_source = 0).
    // Verify it enforces dedup by inserting a duplicate (protocol, host, port)
    // and asserting a constraint violation, then confirm a "missing" row does
    // NOT collide (the partial predicate exempts it).
    sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port) VALUES (?, ?, ?, ?)")
        .bind("01J0NODE000000000000000001")
        .bind("vless")
        .bind("example.com")
        .bind(443_i64)
        .execute(&pool)
        .await
        .expect("first node insert");

    let dup_result =
        sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port) VALUES (?, ?, ?, ?)")
            .bind("01J0NODE000000000000000002")
            .bind("vless")
            .bind("example.com")
            .bind(443_i64)
            .execute(&pool)
            .await;
    assert!(
        dup_result.is_err(),
        "duplicate (protocol, host, port) should be rejected by idx_nodes_dedup"
    );

    // A node marked missing_from_source=1 must NOT collide with the active node.
    sqlx::query(
        "INSERT INTO nodes (id, protocol_kind, host, port, missing_from_source) \
         VALUES (?, ?, ?, ?, 1)",
    )
    .bind("01J0NODE000000000000000003")
    .bind("vless")
    .bind("example.com")
    .bind(443_i64)
    .execute(&pool)
    .await
    .expect("missing node insert should not collide");

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn migration_0004_is_idempotent() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;
    run_migrations(&pool).await;

    let tables = get_table_names(&pool).await;
    for expected in EXPECTED_TABLES_0004 {
        assert!(tables.contains(&expected.to_string()));
    }

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// Verify ON DELETE CASCADE from sources → source_snapshots → source_items
/// and sources → node_source_bindings works (constraint #13: recovery).
#[tokio::test]
async fn migration_0004_cascade_delete_removes_dependents() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let source_id = "01J0SRC0000000000000000001";
    let snapshot_id = "01J0SNP0000000000000000001";
    let item_id = "01J0ITM0000000000000000001";
    let node_id = "01J0NOD0000000000000000001";
    let binding_id = "01J0BND0000000000000000001";

    sqlx::query("INSERT INTO sources (id, name, url) VALUES (?, ?, ?)")
        .bind(source_id)
        .bind("test-source")
        .bind("https://example.com/sub")
        .execute(&pool)
        .await
        .expect("insert source");

    sqlx::query(
        "INSERT INTO source_snapshots (id, source_id, version, is_active) VALUES (?, ?, ?, 1)",
    )
    .bind(snapshot_id)
    .bind(source_id)
    .bind(1_i64)
    .execute(&pool)
    .await
    .expect("insert snapshot");

    // WHY: the partial unique index idx_snapshots_single_active must reject a
    // second active snapshot for the same source.
    let second_active = sqlx::query(
        "INSERT INTO source_snapshots (id, source_id, version, is_active) VALUES (?, ?, ?, 1)",
    )
    .bind("01J0SNP0000000000000000002")
    .bind(source_id)
    .bind(2_i64)
    .execute(&pool)
    .await;
    assert!(
        second_active.is_err(),
        "a second active snapshot should be rejected by idx_snapshots_single_active"
    );

    sqlx::query("INSERT INTO source_items (id, snapshot_id, raw_uri) VALUES (?, ?, ?)")
        .bind(item_id)
        .bind(snapshot_id)
        .bind("vless://example.com:443")
        .execute(&pool)
        .await
        .expect("insert item");

    sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port) VALUES (?, ?, ?, ?)")
        .bind(node_id)
        .bind("vless")
        .bind("example.com")
        .bind(443_i64)
        .execute(&pool)
        .await
        .expect("insert node");

    sqlx::query("INSERT INTO node_source_bindings (id, node_id, source_id) VALUES (?, ?, ?)")
        .bind(binding_id)
        .bind(node_id)
        .bind(source_id)
        .execute(&pool)
        .await
        .expect("insert binding");

    sqlx::query("DELETE FROM sources WHERE id = ?")
        .bind(source_id)
        .execute(&pool)
        .await
        .expect("delete source");

    let (snap_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM source_snapshots WHERE source_id = ?")
            .bind(source_id)
            .fetch_one(&pool)
            .await
            .expect("count snapshots");
    assert_eq!(snap_count, 0, "snapshots should be cascade-deleted");

    let (item_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM source_items WHERE snapshot_id = ?")
            .bind(snapshot_id)
            .fetch_one(&pool)
            .await
            .expect("count items");
    assert_eq!(item_count, 0, "items should be cascade-deleted");

    let (binding_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM node_source_bindings WHERE source_id = ?")
            .bind(source_id)
            .fetch_one(&pool)
            .await
            .expect("count bindings");
    assert_eq!(binding_count, 0, "bindings should be cascade-deleted");

    // The node itself survives — it is not cascade-deleted (nodes are owned
    // by the pool, not by sources).
    let (node_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_one(&pool)
        .await
        .expect("count nodes");
    assert_eq!(node_count, 1, "node should survive source deletion");

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// Recovery test for migration 0005 (constraint #13): the `source_label`
/// column is added to the `nodes` table with `NOT NULL DEFAULT ''`.
///
/// Verifies the column exists, the default is `''` for rows inserted without
/// specifying it, and an explicit `'manual'` value is persisted and
/// retrievable. See
/// `docs/plan/milestones/M4-sources-and-node-pool.md` Slice 3.
#[tokio::test]
async fn migration_0005_adds_source_label_column() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    // Verify the source_label column exists on the nodes table.
    let columns = get_columns(&pool, "nodes").await;
    assert!(
        columns.contains(&"source_label".to_string()),
        "expected column 'source_label' in table 'nodes', columns: {columns:?}"
    );

    // Insert a row without specifying source_label — should default to ''.
    sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port) VALUES (?, ?, ?, ?)")
        .bind("01J0NODE000000000000000005")
        .bind("vless")
        .bind("example.com")
        .bind(443_i64)
        .execute(&pool)
        .await
        .expect("insert node with default source_label");

    let (label,): (String,) = sqlx::query_as("SELECT source_label FROM nodes WHERE id = ?")
        .bind("01J0NODE000000000000000005")
        .fetch_one(&pool)
        .await
        .expect("query source_label");
    assert_eq!(label, "", "default source_label should be empty string");

    // Insert a row with an explicit source_label — should persist.
    sqlx::query(
        "INSERT INTO nodes (id, protocol_kind, host, port, source_label) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind("01J0NODE000000000000000006")
    .bind("trojan")
    .bind("other.com")
    .bind(8443_i64)
    .bind("manual")
    .execute(&pool)
    .await
    .expect("insert node with manual source_label");

    let (label,): (String,) = sqlx::query_as("SELECT source_label FROM nodes WHERE id = ?")
        .bind("01J0NODE000000000000000006")
        .fetch_one(&pool)
        .await
        .expect("query manual source_label");
    assert_eq!(label, "manual", "explicit source_label should be persisted");

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// Migration 0006 adds the `filter_rules_json` TEXT column to the `sources`
/// table (SRC-010). The column is nullable; existing rows get NULL (no
/// filter rules). See constraint #13.
#[tokio::test]
async fn migration_0006_adds_filter_rules_json_column() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let columns = get_columns(&pool, "sources").await;
    assert!(
        columns.contains(&"filter_rules_json".to_string()),
        "expected column 'filter_rules_json' in table 'sources', columns: {columns:?}"
    );

    sqlx::query(
        "INSERT INTO sources (id, name, source_type, url, http_method, auto_update, \
         update_interval_secs, enabled, keep_on_fail) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("01J0SOURCE0000000000000006")
    .bind("test-source")
    .bind("uri_list")
    .bind("https://example.com/sub")
    .bind("GET")
    .bind(0_i64)
    .bind(3600_i64)
    .bind(1_i64)
    .bind(1_i64)
    .execute(&pool)
    .await
    .expect("insert source without filter_rules_json");

    let (rules,): (Option<String>,) =
        sqlx::query_as("SELECT filter_rules_json FROM sources WHERE id = ?")
            .bind("01J0SOURCE0000000000000006")
            .fetch_one(&pool)
            .await
            .expect("query filter_rules_json");
    assert!(
        rules.is_none(),
        "default filter_rules_json should be NULL (no rules)"
    );

    let filter_json = r#"{"include_protocols":["trojan"],"exclude_protocols":[],"include_regions":["US"],"exclude_regions":[]}"#;
    sqlx::query("UPDATE sources SET filter_rules_json = ? WHERE id = ?")
        .bind(filter_json)
        .bind("01J0SOURCE0000000000000006")
        .execute(&pool)
        .await
        .expect("update filter_rules_json");

    let (rules,): (Option<String>,) =
        sqlx::query_as("SELECT filter_rules_json FROM sources WHERE id = ?")
            .bind("01J0SOURCE0000000000000006")
            .fetch_one(&pool)
            .await
            .expect("query filter_rules_json after update");
    assert_eq!(
        rules.as_deref(),
        Some(filter_json),
        "filter_rules_json should round-trip"
    );

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// Recovery test for migration 0008 (constraint #13): pool_meta singleton
/// table, generation_cache.is_active column, and the partial unique index
/// idx_generation_cache_single_active that enforces at most one active
/// generation per (template_id, profile). See
/// `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation cache"
/// (GEN-015: atomic publish).
#[tokio::test]
async fn migration_0008_applies_and_schema_is_correct() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let tables = get_table_names(&pool).await;
    assert!(
        tables.contains(&"pool_meta".to_string()),
        "expected table 'pool_meta', got {tables:?}"
    );

    let indexes = get_index_names(&pool).await;
    assert!(
        indexes.contains(&"idx_generation_cache_single_active".to_string()),
        "expected index 'idx_generation_cache_single_active', got {indexes:?}"
    );

    let meta_cols = get_columns(&pool, "pool_meta").await;
    for col in ["id", "revision"] {
        assert!(
            meta_cols.contains(&col.to_string()),
            "expected column '{col}' in pool_meta, got {meta_cols:?}"
        );
    }

    let cache_cols = get_columns(&pool, "generation_cache").await;
    assert!(
        cache_cols.contains(&"is_active".to_string()),
        "expected column 'is_active' in generation_cache, got {cache_cols:?}"
    );

    let (rev,): (i64,) = sqlx::query_as("SELECT revision FROM pool_meta WHERE id = 1")
        .fetch_one(&pool)
        .await
        .expect("pool_meta row");
    assert_eq!(rev, 0, "pool_meta.revision should default to 0");

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn migration_0008_is_idempotent() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;
    run_migrations(&pool).await;

    let tables = get_table_names(&pool).await;
    assert!(tables.contains(&"pool_meta".to_string()));

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn migration_0008_single_active_generation_enforced() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let template_id = "01J0TMPL000000000000000001";

    sqlx::query("INSERT INTO templates (id, name, description) VALUES (?, ?, ?)")
        .bind(template_id)
        .bind("t")
        .bind("")
        .execute(&pool)
        .await
        .expect("insert template");

    sqlx::query(
        "INSERT INTO generation_cache \
         (id, template_id, template_version, profile, selection_mode, \
          selection_payload, pool_revision, cache_key, content, is_active) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind("01J0CACHE000000000000000000")
    .bind(template_id)
    .bind(1_i64)
    .bind("mihomo")
    .bind("dynamic")
    .bind("{}")
    .bind(0_i64)
    .bind("key0")
    .bind("content")
    .execute(&pool)
    .await
    .expect("insert active cache");

    let second = sqlx::query(
        "INSERT INTO generation_cache \
         (id, template_id, template_version, profile, selection_mode, \
          selection_payload, pool_revision, cache_key, content, is_active) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind("01J0CACHE000000000000000001")
    .bind(template_id)
    .bind(1_i64)
    .bind("mihomo")
    .bind("dynamic")
    .bind("{}")
    .bind(0_i64)
    .bind("key1")
    .bind("content")
    .execute(&pool)
    .await;
    assert!(
        second.is_err(),
        "a second active generation for the same (template_id, profile) should be rejected"
    );

    sqlx::query("UPDATE generation_cache SET is_active = 0 WHERE id = ?")
        .bind("01J0CACHE000000000000000000")
        .execute(&pool)
        .await
        .expect("deactivate first");

    let activate = sqlx::query(
        "INSERT INTO generation_cache \
         (id, template_id, template_version, profile, selection_mode, \
          selection_payload, pool_revision, cache_key, content, is_active) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind("01J0CACHE000000000000000004")
    .bind(template_id)
    .bind(1_i64)
    .bind("mihomo")
    .bind("dynamic")
    .bind("{}")
    .bind(0_i64)
    .bind("key4")
    .bind("content")
    .execute(&pool)
    .await;
    assert!(
        activate.is_ok(),
        "after deactivating the prior active entry, a new one should be allowed"
    );

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// Recovery test for migration 0007 (constraint #13): templates,
/// template_versions, generation_cache tables, the partial unique active-version
/// index, cascade deletes, and idempotency. See
/// `docs/plan/milestones/M5-generator-and-v3-template.md`.
#[tokio::test]
async fn migration_0007_applies_and_schema_is_correct() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let expected_tables_0007 = ["templates", "template_versions", "generation_cache"];
    let tables = get_table_names(&pool).await;
    for expected in expected_tables_0007 {
        assert!(
            tables.contains(&expected.to_string()),
            "expected table '{expected}' not found, tables: {tables:?}"
        );
    }

    let expected_indexes_0007 = [
        "idx_template_versions_template",
        "idx_template_versions_active",
        "idx_template_versions_single_active",
        "idx_generation_cache_lookup",
    ];
    let indexes = get_index_names(&pool).await;
    for expected in expected_indexes_0007 {
        assert!(
            indexes.contains(&expected.to_string()),
            "expected index '{expected}' not found, indexes: {indexes:?}"
        );
    }

    // Verify columns on templates table.
    let tmpl_cols = get_columns(&pool, "templates").await;
    for col in [
        "id",
        "name",
        "description",
        "active_version_id",
        "active_version",
        "created_at",
        "updated_at",
    ] {
        assert!(
            tmpl_cols.contains(&col.to_string()),
            "expected column '{col}' in templates, got {tmpl_cols:?}"
        );
    }

    // Verify columns on template_versions table.
    let ver_cols = get_columns(&pool, "template_versions").await;
    for col in [
        "id",
        "template_id",
        "version",
        "spec_json",
        "spec_yaml",
        "is_active",
        "created_at",
    ] {
        assert!(
            ver_cols.contains(&col.to_string()),
            "expected column '{col}' in template_versions, got {ver_cols:?}"
        );
    }

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn migration_0007_is_idempotent() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;
    run_migrations(&pool).await;

    let tables = get_table_names(&pool).await;
    for expected in ["templates", "template_versions", "generation_cache"] {
        assert!(tables.contains(&expected.to_string()));
    }

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// Verify ON DELETE CASCADE from templates → template_versions and
/// templates → generation_cache works (constraint #13: recovery). Also
/// verifies the partial unique index `idx_template_versions_single_active`
/// rejects a second active version for the same template.
#[tokio::test]
async fn migration_0007_cascade_delete_and_single_active() {
    let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let db_path = tmp
        .into_temp_path()
        .keep()
        .expect("failed to keep temp path");

    let pool = create_test_pool(&db_path).await;
    run_migrations(&pool).await;

    let template_id = "01J0TMPL000000000000000001";
    let version_id = "01J0VER0000000000000000001";
    let cache_id = "01J0CACHE000000000000000001";

    sqlx::query(
        "INSERT INTO templates (id, name, description, active_version_id, active_version) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(template_id)
    .bind("test-template")
    .bind("test")
    .bind(version_id)
    .bind(1_i64)
    .execute(&pool)
    .await
    .expect("insert template");

    sqlx::query(
        "INSERT INTO template_versions (id, template_id, version, spec_json, spec_yaml, is_active) \
         VALUES (?, ?, ?, ?, ?, 1)",
    )
    .bind(version_id)
    .bind(template_id)
    .bind(1_i64)
    .bind("{}")
    .bind("")
    .execute(&pool)
    .await
    .expect("insert version");

    // WHY: the partial unique index idx_template_versions_single_active must
    // reject a second active version for the same template.
    let second_active = sqlx::query(
        "INSERT INTO template_versions (id, template_id, version, spec_json, spec_yaml, is_active) \
         VALUES (?, ?, ?, ?, ?, 1)",
    )
    .bind("01J0VER0000000000000000002")
    .bind(template_id)
    .bind(2_i64)
    .bind("{}")
    .bind("")
    .execute(&pool)
    .await;
    assert!(
        second_active.is_err(),
        "a second active version should be rejected by idx_template_versions_single_active"
    );

    sqlx::query(
        "INSERT INTO generation_cache \
         (id, template_id, template_version, profile, selection_mode, selection_payload, \
          pool_revision, cache_key, content) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(cache_id)
    .bind(template_id)
    .bind(1_i64)
    .bind("mihomo")
    .bind("dynamic")
    .bind("{}")
    .bind(0_i64)
    .bind("test-key")
    .bind("content")
    .execute(&pool)
    .await
    .expect("insert cache");

    sqlx::query("DELETE FROM templates WHERE id = ?")
        .bind(template_id)
        .execute(&pool)
        .await
        .expect("delete template");

    let (ver_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM template_versions WHERE template_id = ?")
            .bind(template_id)
            .fetch_one(&pool)
            .await
            .expect("count versions");
    assert_eq!(ver_count, 0, "versions should be cascade-deleted");

    let (cache_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM generation_cache WHERE template_id = ?")
            .bind(template_id)
            .fetch_one(&pool)
            .await
            .expect("count cache");
    assert_eq!(cache_count, 0, "cache entries should be cascade-deleted");

    pool.close().await;
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}
