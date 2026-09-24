#![allow(clippy::expect_used)]

//! BACKUP-001: an explicit key must never silently lose fingerprint protection.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");

#[test]
fn backup001_invalid_explicit_key_preserves_previous_archive() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("database.sqlite");
    let archive = directory.path().join("backup.tar");
    let key = directory.path().join("explicit.key");
    let output = Command::new(BIN)
        .args(["migrate", "--db-path"])
        .arg(&database)
        .output()
        .expect("migrate");
    assert!(output.status.success());
    let previous_archive = b"previous backup must survive validation failure";

    // Both CLI and environment overrides are explicit intent. A missing key,
    // malformed key, or directory must fail before replacing an existing backup.
    for via_environment in [false, true] {
        for invalid in ["missing", "malformed", "directory"] {
            if key.is_dir() {
                std::fs::remove_dir(&key).expect("remove fixture directory");
            } else if key.exists() {
                std::fs::remove_file(&key).expect("remove fixture key");
            }
            match invalid {
                "malformed" => std::fs::write(&key, b"invalid key").expect("write fixture"),
                "directory" => std::fs::create_dir(&key).expect("key directory"),
                _ => {}
            }
            std::fs::write(&archive, previous_archive).expect("previous backup");
            let mut command = Command::new(BIN);
            command
                .args(["backup", "--db-path"])
                .arg(&database)
                .arg("--output")
                .arg(&archive)
                .env_remove("DEVE_SUB_KEY_PATH");
            if via_environment {
                command.env("DEVE_SUB_KEY_PATH", &key);
            } else {
                command.arg("--key-path").arg(&key);
            }
            let output = command.output().expect("backup");
            assert!(
                !output.status.success(),
                "{invalid}, environment={via_environment}: invalid explicit key was accepted"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("explicitly requested"));
            assert_eq!(
                std::fs::read(&archive).expect("existing backup"),
                previous_archive
            );
        }
    }
}
