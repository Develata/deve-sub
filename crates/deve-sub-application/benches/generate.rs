//! PERF-003 / PERF-004: subscription generation latency.
//!
//! PERF-003 (cached): `generate` with a pre-populated cache entry — measures
//! the cache-hit path (hash lookup → return stored content).
//!
//! PERF-004 (uncached): `generate` with an empty cache — measures the full
//! pipeline (resolve context → select nodes → compatibility check → emit →
//! validate → store + activate cache).
//!
//! Setup: 100 Trojan nodes in the pool, one minimal mihomo V3 template with
//! `nodeSelector: mode: dynamic` (selects all active nodes).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use deve_sub_application::template::{CreateTemplateParams, create_template, generate};
use deve_sub_domain::{
    Authentication, DomainName, Endpoint, GenerationMode, GenerationRequest, Host, Node,
    NodePoolRepository, NodeSource, ProtocolConfig, ProtocolKind, RegionAssignment, RegionMethod,
    TrojanConfig, UdpCapability,
};
use deve_sub_kernel::Timestamp;
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteNodePoolRepository, SqlitePoolMetaRepository,
    SqliteTemplateRepository, SqliteTemplateVersionRepository,
};

/// Minimal V3 template selecting all active nodes for mihomo.
const SPEC_YAML: &str = concat!(
    "apiVersion: deve-sub.io/v1\n",
    "kind: SubscriptionTemplate\n",
    "\n",
    "metadata:\n",
    "  name: bench-mihomo\n",
    "  description: Benchmark template\n",
    "  version: 1\n",
    "\n",
    "spec:\n",
    "  targetProfiles:\n",
    "    - mihomo\n",
    "  variables: {}\n",
    "  nodeSelector:\n",
    "    mode: dynamic\n",
    "  proxyGroups: []\n",
    "  rules: []\n",
    "  dns: {}\n",
    "  tun: {}\n",
    "  output: {}",
);

/// A temporary SQLite database with nodes, a template, and all repos wired.
struct BenchDb {
    pool: sqlx::SqlitePool,
    template_id: deve_sub_kernel::TemplateId,
    _dir: tempfile::TempDir,
}

impl BenchDb {
    async fn new(node_count: usize) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("bench.db");
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
            .await
            .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");

        // Populate nodes.
        let pool_repo = SqliteNodePoolRepository::new(pool.clone());
        let batch = 500;
        let mut made = 0;
        while made < node_count {
            let take = std::cmp::min(batch, node_count - made);
            let nodes = (0..take).map(|i| make_trojan_node(made + i)).collect();
            pool_repo.import_nodes(nodes).await.expect("import batch");
            made += take;
        }

        // Create the template.
        let template_repo = SqliteTemplateRepository::new(pool.clone());
        let version_repo = SqliteTemplateVersionRepository::new(pool.clone());
        let result = create_template(
            &template_repo,
            &version_repo,
            CreateTemplateParams {
                name: "bench-mihomo".to_owned(),
                description: "Benchmark template".to_owned(),
                spec_yaml: SPEC_YAML.to_owned(),
            },
        )
        .await
        .expect("create template");

        Self {
            pool,
            template_id: result.template.id,
            _dir: dir,
        }
    }

    /// Delete all generation cache entries so the next `generate` call misses.
    async fn clear_cache(&self) {
        sqlx::query("DELETE FROM generation_cache")
            .execute(&self.pool)
            .await
            .expect("clear cache");
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

/// Build a `GenerationRequest` for the bench template + mihomo profile.
fn make_request(template_id: deve_sub_kernel::TemplateId) -> GenerationRequest {
    GenerationRequest::new(template_id, "mihomo".to_owned(), GenerationMode::Lenient)
}

fn bench_generate(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let db = rt.block_on(BenchDb::new(100));

    let template_repo = SqliteTemplateRepository::new(db.pool.clone());
    let version_repo = SqliteTemplateVersionRepository::new(db.pool.clone());
    let pool_repo = SqliteNodePoolRepository::new(db.pool.clone());
    let cache_repo = SqliteGenerationCacheRepository::new(db.pool.clone());
    let pool_meta_repo = SqlitePoolMetaRepository::new(db.pool.clone());
    let request = make_request(db.template_id);

    // PERF-003: prime the cache so every iteration hits.
    rt.block_on(generate(
        &template_repo,
        &version_repo,
        &pool_repo,
        &cache_repo,
        &pool_meta_repo,
        request.clone(),
    ))
    .expect("prime cache");

    let mut group = c.benchmark_group("generate");

    // PERF-003: cached path (cache hit).
    group.bench_with_input(
        BenchmarkId::new("generate", "cached"),
        &request,
        |b, req| {
            b.iter(|| {
                rt.block_on(generate(
                    &template_repo,
                    &version_repo,
                    &pool_repo,
                    &cache_repo,
                    &pool_meta_repo,
                    req.clone(),
                ))
                .expect("generate cached")
            });
        },
    );

    // PERF-004: uncached path (full pipeline). Clear the cache before each
    // measurement so every call misses and runs the full pipeline.
    group.bench_with_input(
        BenchmarkId::new("generate", "uncached"),
        &request,
        |b, req| {
            b.iter_batched(
                || rt.block_on(db.clear_cache()),
                |_| {
                    rt.block_on(generate(
                        &template_repo,
                        &version_repo,
                        &pool_repo,
                        &cache_repo,
                        &pool_meta_repo,
                        req.clone(),
                    ))
                    .expect("generate uncached")
                },
                BatchSize::SmallInput,
            );
        },
    );

    group.finish();
}

criterion_group!(benches, bench_generate);
criterion_main!(benches);
