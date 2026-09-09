//! PERF-001: 10k node parsing benchmark.
//!
//! Generates up to 10k Trojan URIs and parses them through
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
    group.sample_size(30);
    group.warm_up_time(std::time::Duration::from_secs(1));
    group.measurement_time(std::time::Duration::from_secs(3));
    for size in [100, 1_000, 10_000] {
        let uris = generate_uris(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &uris, |b, uris| {
            b.iter(|| {
                for uri in uris {
                    let _ = std::hint::black_box(deve_sub_protocol::parse_uri(
                        std::hint::black_box(uri),
                    ));
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parse_10k);
criterion_main!(benches);
