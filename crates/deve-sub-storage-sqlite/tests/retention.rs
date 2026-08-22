#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Retention integration tests (review C-5): bounded storage growth for
//! source_snapshots, generation_cache, and probe_runs/latency_records.

use std::sync::Arc;

use deve_sub_domain::{
    GenerationCacheEntry, GenerationCacheRepository, ItemParseStatus, LatencyRecord,
    LatencyRecordRepository, NodePoolRepository, ProbeRun, ProbeRunRepository, ProbeRunStatus,
    ProbeType, ReconcileEntry, ReconcileInput, Source, SourceRepository, SourceSnapshot,
    SourceType,
};
use deve_sub_kernel::{
    GenerationCacheId, LatencyRecordId, NodeId, ProbeRunId, SourceId, SourceSnapshotId, TemplateId,
    Timestamp,
};
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteLatencyRecordRepository, SqliteNodePoolRepository,
    SqliteProbeRunRepository, SqliteSourceRepository,
};

struct TestDb {
    pool: sqlx::sqlite::SqlitePool,
    master_key: Arc<deve_sub_security::MasterKey>,
    _dir: tempfile::TempDir,
}

impl TestDb {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool =
            sqlx::sqlite::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
                .await
                .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");
        Self {
            pool,
            master_key: Arc::new(deve_sub_security::MasterKey::from_bytes(&[0x42u8; 32])),
            _dir: dir,
        }
    }
}

/// Build a valid 26-char ULID string: 8-char prefix + zero-padded numeric
/// suffix. Sequential suffixes give deterministic lexicographic ordering —
/// `Ulid::new()` is not monotonic within one millisecond, and retention /
/// find_latest order by id.
fn ulid_str(prefix: &str, suffix: u32) -> String {
    assert_eq!(prefix.len(), 8, "8-char Crockford-safe prefix required");
    format!("{prefix}{suffix:0>18}")
}

fn days_ago(days: i64) -> Timestamp {
    Timestamp::now() - time::Duration::days(days)
}

// ---------------------------------------------------------------------------
// source_snapshots retention (reconcile keeps newest 10 per source)
// ---------------------------------------------------------------------------

fn make_source(name: &str) -> Source {
    let mut s = Source::new(
        name,
        SourceType::UriList,
        "https://example.com/sub".to_owned(),
    );
    s.id = SourceId::new();
    s
}

fn trojan_entry(uri: &str) -> ReconcileEntry {
    let mut node = deve_sub_protocol::parse_uri(uri).expect("parse trojan URI");
    node.id = NodeId::new();
    node.source.imported_at = Timestamp::now();
    let raw = node.source.raw_uri.clone().unwrap_or_default();
    ReconcileEntry {
        raw_uri: raw,
        initial_status: ItemParseStatus::Parsed,
        node: Some(node),
    }
}

const TROJAN_A: &str = "trojan://TEST_PASSWORD@example.com:443?sni=example.com&type=tcp#NodeA";

/// 12 refreshes keep only the newest 10 snapshots; source_items are removed
/// by cascade with their snapshot; the active snapshot is the newest.
#[tokio::test]
async fn reconcile_prunes_snapshots_beyond_retention() {
    let db = TestDb::new().await;
    let source_repo =
        SqliteSourceRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));
    let pool_repo =
        SqliteNodePoolRepository::new_with_key(db.pool.clone(), Arc::clone(&db.master_key));

    let source = make_source("retention-source");
    source_repo.create(&source).await.expect("create source");

    const REFRESHES: u64 = 12;
    for version in 1..=REFRESHES {
        let snapshot = SourceSnapshot {
            id: SourceSnapshotId::new(),
            source_id: source.id,
            version,
            fetched_at: Timestamp::now(),
            etag: None,
            node_count: 1,
            is_active: true,
        };
        pool_repo
            .reconcile(ReconcileInput {
                source_id: source.id,
                snapshot: &snapshot,
                entries: &[trojan_entry(TROJAN_A)],
            })
            .await
            .expect("reconcile");
    }

    let (snapshots,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM source_snapshots WHERE source_id = ?")
            .bind(source.id.to_string())
            .fetch_one(&db.pool)
            .await
            .expect("count snapshots");
    assert_eq!(snapshots, 10, "newest 10 snapshots kept");

    let (min_version,): (i64,) =
        sqlx::query_as("SELECT MIN(version) FROM source_snapshots WHERE source_id = ?")
            .bind(source.id.to_string())
            .fetch_one(&db.pool)
            .await
            .expect("min version");
    assert_eq!(min_version, 3, "versions 1-2 pruned, 3-12 kept");

    let (active_version,): (i64,) = sqlx::query_as(
        "SELECT version FROM source_snapshots WHERE source_id = ? AND is_active = 1",
    )
    .bind(source.id.to_string())
    .fetch_one(&db.pool)
    .await
    .expect("active version");
    assert_eq!(active_version, REFRESHES as i64, "newest snapshot active");

    // Cascade check: source_items of pruned snapshots are gone. Each kept
    // snapshot has exactly 1 item, so total items = kept snapshots.
    let (items,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM source_items si \
         JOIN source_snapshots ss ON si.snapshot_id = ss.id \
         WHERE ss.source_id = ?",
    )
    .bind(source.id.to_string())
    .fetch_one(&db.pool)
    .await
    .expect("count items");
    assert_eq!(items, 10, "items cascade-removed with pruned snapshots");
}

// ---------------------------------------------------------------------------
// generation_cache retention (store keeps newest 8 inactive per shape)
// ---------------------------------------------------------------------------

fn template_id() -> TemplateId {
    TemplateId::parse(&ulid_str("01KZTEMP", 0)).expect("ulid")
}

fn cache_entry(suffix: u32, is_active: bool) -> GenerationCacheEntry {
    GenerationCacheEntry {
        id: GenerationCacheId::parse(&ulid_str("01KZCACA", suffix)).expect("ulid"),
        template_id: template_id(),
        template_version: 1,
        profile: "mihomo".to_owned(),
        mode: "lenient".to_owned(),
        selection_mode: "dynamic".to_owned(),
        selection_payload: "{}".to_owned(),
        pool_revision: u64::from(suffix),
        cache_key: format!("ck-{suffix}"),
        content: format!("content-{suffix}"),
        is_active,
    }
}

#[tokio::test]
async fn generation_cache_store_prunes_inactive_beyond_retention() {
    let db = TestDb::new().await;
    let template_id = template_id();
    sqlx::query("INSERT INTO templates (id, name) VALUES (?, 'retention-tpl')")
        .bind(template_id.to_string())
        .execute(&db.pool)
        .await
        .expect("insert template");

    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());

    // 12 inactive stores: only the newest 8 (suffixes 4-11) survive.
    for suffix in 0..12u32 {
        cache_repo
            .store(&cache_entry(suffix, false))
            .await
            .expect("store");
    }
    let (inactive,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM generation_cache WHERE is_active = 0")
            .fetch_one(&db.pool)
            .await
            .expect("count inactive");
    assert_eq!(inactive, 8, "newest 8 inactive entries kept");

    let (oldest_kept,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM generation_cache WHERE id IN (?, ?, ?, ?)")
            .bind(ulid_str("01KZCACA", 0))
            .bind(ulid_str("01KZCACA", 1))
            .bind(ulid_str("01KZCACA", 2))
            .bind(ulid_str("01KZCACA", 3))
            .fetch_one(&db.pool)
            .await
            .expect("oldest ids");
    assert_eq!(oldest_kept, 0, "oldest 4 inactive entries pruned");

    // Activate the newest entry, then store more: the active entry is never
    // pruned, and inactive stays bounded at 8.
    cache_repo
        .activate(template_id, "mihomo", cache_entry(11, false).id)
        .await
        .expect("activate");
    for suffix in 12..15u32 {
        cache_repo
            .store(&cache_entry(suffix, false))
            .await
            .expect("store");
    }
    let (active,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM generation_cache WHERE is_active = 1")
            .fetch_one(&db.pool)
            .await
            .expect("count active");
    assert_eq!(active, 1, "active entry preserved");
    let (active_id,): (String,) =
        sqlx::query_as("SELECT id FROM generation_cache WHERE is_active = 1")
            .fetch_one(&db.pool)
            .await
            .expect("active id");
    assert_eq!(active_id, ulid_str("01KZCACA", 11));

    let (inactive,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM generation_cache WHERE is_active = 0")
            .fetch_one(&db.pool)
            .await
            .expect("count inactive");
    assert_eq!(inactive, 8, "inactive entries bounded at 8 after activate");
}

// ---------------------------------------------------------------------------
// probe_runs retention (prune_older_than cascades latency_records)
// ---------------------------------------------------------------------------

fn make_run(suffix: u32, created_days_ago: i64) -> ProbeRun {
    ProbeRun {
        id: ProbeRunId::parse(&ulid_str("01KZRANA", suffix)).expect("ulid"),
        probe_type: ProbeType::TcpConnect,
        node_ids: vec![],
        status: ProbeRunStatus::Completed,
        results: vec![],
        created_at: days_ago(created_days_ago),
        completed_at: None,
    }
}

#[tokio::test]
async fn prune_older_than_removes_old_runs_and_cascades_latency_records() {
    let db = TestDb::new().await;
    let run_repo = SqliteProbeRunRepository::new(db.pool.clone());
    let latency_repo = SqliteLatencyRecordRepository::new(db.pool.clone());

    // latency_records.node_id has an FK to nodes; insert a minimal node row.
    let node_id = NodeId::parse(&ulid_str("01KZNDEA", 0)).expect("ulid");
    sqlx::query("INSERT INTO nodes (id, protocol_kind, host, port) VALUES (?, 'trojan', ?, 443)")
        .bind(node_id.to_string())
        .bind("node.example.com")
        .execute(&db.pool)
        .await
        .expect("insert node");

    let old_run = make_run(0, 40);
    let fresh_run = make_run(1, 1);
    run_repo.create(&old_run).await.expect("create old");
    run_repo.create(&fresh_run).await.expect("create fresh");

    let old_record = LatencyRecord {
        id: LatencyRecordId::parse(&ulid_str("01KZRTTA", 0)).expect("ulid"),
        run_id: old_run.id,
        node_id,
        probe_type: ProbeType::TcpConnect,
        rtt_ms: Some(12),
        error_class: deve_sub_domain::ErrorClass::Ok,
        measured_at: days_ago(40),
    };
    let fresh_record = LatencyRecord {
        id: LatencyRecordId::parse(&ulid_str("01KZRTTA", 1)).expect("ulid"),
        run_id: fresh_run.id,
        node_id,
        probe_type: ProbeType::TcpConnect,
        rtt_ms: Some(34),
        error_class: deve_sub_domain::ErrorClass::Ok,
        measured_at: days_ago(1),
    };
    latency_repo.create(&old_record).await.expect("latency old");
    latency_repo
        .create(&fresh_record)
        .await
        .expect("latency fresh");

    let cutoff = days_ago(30);
    let pruned = run_repo.prune_older_than(cutoff).await.expect("prune");
    assert_eq!(pruned, 1, "only the 40-day-old run is pruned");

    assert!(run_repo.find_by_id(old_run.id).await.unwrap().is_none());
    assert!(run_repo.find_by_id(fresh_run.id).await.unwrap().is_some());

    let (latency_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM latency_records")
        .fetch_one(&db.pool)
        .await
        .expect("count latency");
    assert_eq!(latency_count, 1, "cascade removed the old run's records");

    // Idempotent: re-pruning removes nothing more.
    let pruned_again = run_repo
        .prune_older_than(cutoff)
        .await
        .expect("prune again");
    assert_eq!(pruned_again, 0);
}
