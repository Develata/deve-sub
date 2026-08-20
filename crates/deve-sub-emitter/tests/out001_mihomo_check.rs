//! OUT-001: Mihomo (Clash Meta) config validation.
//!
//! Emits a multi-protocol subscription via `emit_mihomo`, wraps the
//! `proxies:` array in a minimal Mihomo config, and validates it with
//! `mihomo -t -f <file>`.
//!
//! WHY #[ignore]: these tests require the `mihomo` binary on PATH. Without
//! it, `cargo test` reports them as `ignored` (honest), not `pass` (which
//! would be a false positive per B-19). The release gate runs them via
//! `cargo test -- --ignored` in a CI job that installs the binary.

#![allow(clippy::expect_used)]

mod common;

use std::process::Command;

use tempfile::NamedTempFile;

/// Returns `true` when `mihomo` is on PATH.
fn mihomo_available() -> bool {
    Command::new("mihomo")
        .arg("-v")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// OUT-001: emitted Mihomo YAML passes `mihomo -t` for every supported
/// protocol.
#[test]
#[ignore = "requires mihomo binary on PATH; run with cargo test -- --ignored"]
fn out001_mihomo_check_passes() {
    if !mihomo_available() {
        panic!("mihomo binary not on PATH — required for release gate validation (B-19)");
    }

    let nodes = common::compatible_nodes(deve_sub_compatibility::ProfileKind::Mihomo);
    let proxies_yaml = deve_sub_emitter::emit_mihomo(&nodes).expect("emit mihomo");

    // WHY: emit_mihomo produces only the `proxies:` array. Mihomo's `-t`
    // flag requires a complete config with at least a port and mode, so
    // we prepend a minimal header.
    let config = format!("mixed-port: 7890\nmode: rule\nlog-level: info\n{proxies_yaml}\n");

    let tmp = NamedTempFile::new().expect("temp file");
    std::fs::write(tmp.path(), &config).expect("write config");

    let output = Command::new("mihomo")
        .arg("-t")
        .arg("-f")
        .arg(tmp.path())
        .output()
        .expect("run mihomo -t");

    assert!(
        output.status.success(),
        "mihomo -t failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// OUT-001 (negative): malformed YAML is rejected by `mihomo -t`,
/// proving the check is a real validation, not a no-op.
#[test]
#[ignore = "requires mihomo binary on PATH; run with cargo test -- --ignored"]
fn out001_mihomo_check_rejects_garbage() {
    if !mihomo_available() {
        panic!("mihomo binary not on PATH — required for release gate validation (B-19)");
    }

    let tmp = NamedTempFile::new().expect("temp file");
    std::fs::write(
        tmp.path(),
        b"mixed-port: 7890\nproxies: [this is not valid yaml ]]",
    )
    .expect("write config");

    let output = Command::new("mihomo")
        .arg("-t")
        .arg("-f")
        .arg(tmp.path())
        .output()
        .expect("run mihomo -t");

    assert!(
        !output.status.success(),
        "mihomo -t should reject garbage, but succeeded"
    );
}
