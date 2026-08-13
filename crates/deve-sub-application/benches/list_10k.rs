//! PERF-002: list 10k nodes through the application query path.
//!
//! Populates a temporary SQLite database with 10k Trojan nodes, then
//! benchmarks `deve_sub_application::source::list_nodes` at page sizes
//! 100 / 1k / 10k to measure end-to-end query latency (storage row mapping
//! → domain `NodePoolEntry` → application response).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use deve_sub_application::source::{ListNodesParams, list_nodes};
use deve_sub_domain::{
    Authentication, DomainName, Endpoint, Host, Node, NodePoolRepository, NodeSource,
    ProtocolConfig, ProtocolKind, RegionAssignment, RegionMethod, TrojanConfig, UdpCapability,
};
use deve_sub_kernel::Timestamp;
use deve_sub_storage_sqlite::SqliteNodePoolRepository;

/// A temporary SQLite database populated with `count` Trojan nodes.
struct BenchDb {
    pool: sqlx::SqlitePool,
    _dir: tempfile::TempDir,
}

impl BenchDb {
    async fn new(count: usize) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("bench.db");
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
            .await
            .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let repo = SqliteNodePoolRepository::new(pool.clone());

        // WHY batch size 500: SQLite has a default 999 host-parameter limit
        // per statement. `import_nodes` binds ~20 columns per node, so 500
        // nodes × 20 ≈ 10k params stays well under the ceiling while keeping
        // round-trips low.
        let batch = 500;
        let mut made = 0;
        while made < count {
            let take = std::cmp::min(batch, count - made);
            let nodes = (0..take)
                .map(|i| {
                    let idx = made + i;
                    make_trojan_node(idx)
                })
                .collect();
            repo.import_nodes(nodes).await.expect("import batch");
            made += take;
        }

        Self { pool, _dir: dir }
    }
}

fn make_trojan_node(idx: usize) -> Node {
    let host = format!("host{idx}.example.com");
    Node {
        id: deve_sub_kernel::NodeId::new(),
        display_name: format!("node-{idx}"),
        protocol: ProtocolKind::Trojan,
        config: ProtocolConfig::Trojan(TrojanConfig {
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new(host)),
            port: 443,
        },
        authentication: Authentication::Password {
            password: format!("PASS_{idx}"),
        },
        transport: None,
        tls: None,
        udp: UdpCapability {
            supported: None,
            xudp: None,
        },
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "bench".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::now(),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: BTreeMap::new(),
    }
}

fn bench_list_10k(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let db = rt.block_on(BenchDb::new(10_000));
    let repo = SqliteNodePoolRepository::new(db.pool.clone());

    let mut group = c.benchmark_group("list_10k");
    for limit in [100_u32, 1_000, 10_000] {
        group.throughput(Throughput::Elements(u64::from(limit)));
        group.bench_with_input(BenchmarkId::from_parameter(limit), &limit, |b, &limit| {
            b.iter(|| {
                let params = ListNodesParams {
                    limit,
                    ..Default::default()
                };
                rt.block_on(list_nodes(&repo, params))
                    .expect("list_nodes")
                    .len()
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_list_10k);
criterion_main!(benches);
