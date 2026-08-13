//! PERF-001: 10k node parsing benchmark.
//!
//! Generates 10k mixed-protocol URIs and parses them through
//! `deve_sub_protocol::parse_uri`, measuring throughput.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn generate_uris(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let host = format!("host{i}.example.com");
            format!("trojan://PASSWORD_{i}@{host}:443?sni=example.com&type=tcp#Node-{i}")
        })
        .collect()
}

fn bench_parse_10k(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_10k");
    for size in [100, 1_000, 10_000] {
        let uris = generate_uris(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &uris, |b, uris| {
            b.iter(|| {
                for uri in uris {
                    let _ = deve_sub_protocol::parse_uri(uri);
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parse_10k);
criterion_main!(benches);
