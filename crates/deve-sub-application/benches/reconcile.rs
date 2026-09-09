//! Real encrypted SQLite reconciliation at 100 / 1k / 10k nodes.
//! Measures complete transactions (new pool and steady refresh); setup and
//! snapshot construction are outside the timed region. No mock repositories.
#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use deve_sub_domain::{
    ItemParseStatus, NodePoolRepository, ReconcileEntry, ReconcileInput, Source, SourceRepository,
    SourceSnapshot, SourceType,
};
use deve_sub_kernel::{SourceSnapshotId, Timestamp};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteConfig, SqliteNodePoolRepository, SqliteSourceRepository, create_pool, run_migrations,
};

fn bench_reconcile(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let mut group = c.benchmark_group("reconcile");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    for count in [100, 1_000, 10_000] {
        let entries: Vec<_> = (0..count)
            .map(|i| {
                let uri = format!("trojan://TEST_PASSWORD@node-{i}.example.com:443#Node-{i}");
                ReconcileEntry {
                    node: Some(deve_sub_protocol::parse_uri(&uri).expect("parse")),
                    raw_uri: uri,
                    initial_status: ItemParseStatus::Parsed,
                }
            })
            .collect();
        for mode in ["new", "refresh"] {
            group.bench_with_input(BenchmarkId::new(mode, count), &count, |b, _| {
                b.iter_custom(|iterations| {
                    rt.block_on(async {
                        let dir = tempfile::tempdir().expect("tempdir");
                        let pool = create_pool(&SqliteConfig::new(dir.path().join("bench.db")))
                            .await
                            .expect("pool");
                        run_migrations(&pool).await.expect("migrations");
                        let key = Arc::new(MasterKey::from_bytes(&[0x42; 32]));
                        let repo =
                            SqliteNodePoolRepository::new_with_key(pool.clone(), key.clone());
                        let sources = SqliteSourceRepository::new_with_key(pool.clone(), key);
                        let source = Source::new(
                            "bench",
                            SourceType::UriList,
                            "https://example.com/test".into(),
                        );
                        sources.create(&source).await.expect("source");
                        let mut elapsed = Duration::ZERO;
                        for i in 0..iterations {
                            if mode == "new" {
                                // Cascades clear source items/bindings before the next
                                // sample; cache/pool revision are irrelevant to reconcile.
                                sqlx::query("DELETE FROM source_snapshots")
                                    .execute(&pool)
                                    .await
                                    .expect("clear snapshots");
                                sqlx::query("DELETE FROM nodes")
                                    .execute(&pool)
                                    .await
                                    .expect("clear nodes");
                            }
                            let snapshot = SourceSnapshot {
                                id: SourceSnapshotId::new(),
                                source_id: source.id,
                                version: i + 1,
                                fetched_at: Timestamp::now(),
                                etag: None,
                                node_count: count as u64,
                                is_active: true,
                            };
                            let start = Instant::now();
                            let result = repo
                                .reconcile(ReconcileInput {
                                    source_id: source.id,
                                    snapshot: &snapshot,
                                    entries: &entries,
                                })
                                .await
                                .expect("reconcile");
                            elapsed += start.elapsed();
                            assert_eq!(result.new_nodes + result.duplicate_nodes, count as u64);
                        }
                        pool.close().await;
                        elapsed
                    })
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_reconcile);
criterion_main!(benches);
