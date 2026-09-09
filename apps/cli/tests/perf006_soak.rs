#![allow(clippy::expect_used)]
//! PERF-006 launches the real CLI with a temporary database and configuration.
//! See scripts/perf/soak.py for workflows, observations and explicit limits.

#[test]
#[ignore = "real-process soak: run explicitly; defaults to 90 seconds"]
fn soak() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/perf/soak.py");
    let seconds = std::env::var("DEVE_SUB_SOAK_SECONDS").unwrap_or_else(|_| "90".into());
    let status = std::process::Command::new("python3")
        .arg(script)
        .args([
            "--binary",
            env!("CARGO_BIN_EXE_deve-sub"),
            "--seconds",
            &seconds,
            "--require-telemetry",
        ])
        .status()
        .expect("run soak harness");
    assert!(status.success(), "real application soak failed");
}
