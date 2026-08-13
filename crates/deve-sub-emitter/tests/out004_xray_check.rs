//! OUT-004: Xray config validation.
//!
//! Emits a multi-protocol subscription via `emit_xray` and validates the
//! JSON with `xray run -test -config <file>`. The test is skipped when
//! the `xray` binary is not on PATH.

#![allow(clippy::expect_used)]

mod common;

use std::process::Command;

/// Returns `true` when `xray` is on PATH.
fn xray_available() -> bool {
    Command::new("xray")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// OUT-004: emitted Xray JSON passes `xray run -test` for every
/// supported protocol.
#[test]
fn out004_xray_check_passes() {
    if !xray_available() {
        eprintln!("skip: xray binary not on PATH");
        return;
    }

    let nodes = common::compatible_nodes(deve_sub_compatibility::ProfileKind::Xray);
    let json = deve_sub_emitter::emit_xray(&nodes).expect("emit xray");

    let tmp = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .expect("temp file");
    std::fs::write(tmp.path(), &json).expect("write config");

    let output = Command::new("xray")
        .arg("run")
        .arg("-test")
        .arg("-config")
        .arg(tmp.path())
        .output()
        .expect("run xray run -test");

    assert!(
        output.status.success(),
        "xray run -test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// OUT-004 (negative): malformed JSON is rejected by `xray run -test`,
/// proving the check is a real validation, not a no-op.
#[test]
fn out004_xray_check_rejects_garbage() {
    if !xray_available() {
        eprintln!("skip: xray binary not on PATH");
        return;
    }

    let tmp = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .expect("temp file");
    std::fs::write(tmp.path(), b"{not valid json").expect("write config");

    let output = Command::new("xray")
        .arg("run")
        .arg("-test")
        .arg("-config")
        .arg(tmp.path())
        .output()
        .expect("run xray run -test");

    assert!(
        !output.status.success(),
        "xray run -test should reject garbage, but succeeded"
    );
}
