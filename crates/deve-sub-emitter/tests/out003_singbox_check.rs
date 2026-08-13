//! OUT-003: sing-box `check` validation.
//!
//! Emits a multi-protocol subscription via `emit_singbox` and pipes the
//! output through `sing-box check -c <file>`. The test is skipped when
//! the `sing-box` binary is not on PATH (CI without the tool still gets
//! fmt/clippy/test coverage from the rest of the suite).

#![allow(clippy::expect_used)]

mod common;

use std::process::Command;

use tempfile::NamedTempFile;

/// Returns `true` when `sing-box` is on PATH.
fn singbox_available() -> bool {
    Command::new("sing-box")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// OUT-003: emitted sing-box JSON passes `sing-box check` for every
/// supported protocol.
#[test]
fn out003_singbox_check_passes() {
    if !singbox_available() {
        eprintln!("skip: sing-box binary not on PATH");
        return;
    }

    let nodes = common::compatible_nodes(deve_sub_compatibility::ProfileKind::SingBox);
    let json = deve_sub_emitter::emit_singbox(&nodes).expect("emit sing-box");

    let tmp = NamedTempFile::new().expect("temp file");
    std::fs::write(tmp.path(), &json).expect("write config");

    let output = Command::new("sing-box")
        .arg("check")
        .arg("-c")
        .arg(tmp.path())
        .output()
        .expect("run sing-box check");

    assert!(
        output.status.success(),
        "sing-box check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// OUT-003 (negative): malformed JSON is rejected by `sing-box check`,
/// proving the check is a real validation, not a no-op.
#[test]
fn out003_singbox_check_rejects_garbage() {
    if !singbox_available() {
        eprintln!("skip: sing-box binary not on PATH");
        return;
    }

    let tmp = NamedTempFile::new().expect("temp file");
    std::fs::write(tmp.path(), b"{not valid json").expect("write config");

    let output = Command::new("sing-box")
        .arg("check")
        .arg("-c")
        .arg(tmp.path())
        .output()
        .expect("run sing-box check");

    assert!(
        !output.status.success(),
        "sing-box check should reject garbage, but succeeded"
    );
}
