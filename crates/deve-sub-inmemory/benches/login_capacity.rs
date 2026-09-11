//! Capacity-scale timing, excluding fixture setup; no timing pass threshold.
use deve_sub_application::auth::LoginRateLimiter;
use deve_sub_inmemory::InMemoryLoginRateLimiter;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn main() {
    if cfg!(debug_assertions) {
        println!("NOT RUN: use cargo bench for optimized timing");
        return;
    }
    let inputs: Vec<_> = (0..100_000)
        .map(|i| (format!("synthetic-user-{i}"), format!("synthetic-ip-{i}")))
        .collect();
    for locked in [0, 9_998, 9_999, 10_000] {
        let mut times = Vec::new();
        for _ in 0..5 {
            let limiter = InMemoryLoginRateLimiter::new(2, Duration::from_secs(3600));
            for i in 0..10_000 {
                let user = format!("resident-{i}");
                limiter.record_failure(&user, None);
                if i < locked {
                    limiter.record_failure(&user, None);
                }
            }
            let start = Instant::now();
            let mut allowed = 0;
            for (user, ip) in &inputs {
                if black_box(limiter.check(user, if locked == 9_999 { None } else { Some(ip) }))
                    .is_ok()
                {
                    allowed += 1;
                    limiter.record_failure(user, if locked == 9_999 { None } else { Some(ip) });
                }
            }
            let elapsed = start.elapsed();
            assert!(limiter.resident_entries().is_some_and(|n| n <= 10_000));
            if locked > 0 {
                assert!(limiter.check("resident-0", None).is_err());
            }
            println!(
                "locked={locked} attempts={} allowed={allowed} resident={:?} elapsed_ms={:.3}",
                inputs.len(),
                limiter.resident_entries(),
                elapsed.as_secs_f64() * 1000.0
            );
            times.push(elapsed);
        }
        times.sort();
        println!(
            "locked={locked} median_ms={:.3}",
            times[2].as_secs_f64() * 1000.0
        );
    }
}
