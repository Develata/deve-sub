//! Extraction rejects ambiguous or redirected authority files.
#![allow(clippy::expect_used)]
use super::*;

fn archive_with(entries: &[(&str, tar::EntryType)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive_path = dir.path().join("test.tar");
    let mut archive = tar::Builder::new(std::fs::File::create(&archive_path).expect("file"));
    for (name, kind) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o600);
        header.set_entry_type(*kind);
        if kind.is_symlink() || kind.is_hard_link() {
            header
                .set_link_name("/tmp/deve-sub-should-never-be-written")
                .expect("link");
        }
        header.set_cksum();
        archive
            .append_data(&mut header, *name, std::io::empty())
            .expect("append");
    }
    archive.finish().expect("finish");
    (dir, archive_path)
}

#[test]
fn known_paths_must_be_unique_regular_files() {
    for name in [
        "database.sqlite",
        "manifest.json",
        "config.json",
        "metadata.json",
    ] {
        for kind in [
            tar::EntryType::Symlink,
            tar::EntryType::Link,
            tar::EntryType::Directory,
        ] {
            let (dir, path) = archive_with(&[(name, kind)]);
            let dest = dir.path().join("out");
            std::fs::create_dir(&dest).expect("dest");
            let error = extract_archive(&path, &dest).expect_err("nonregular rejected");
            assert!(error.to_string().contains("regular file"), "{error:#}");
        }
    }
    let (dir, path) = archive_with(&[
        ("database.sqlite", tar::EntryType::Regular),
        ("database.sqlite", tar::EntryType::Regular),
    ]);
    let error = extract_archive(&path, dir.path()).expect_err("duplicate rejected");
    assert!(error.to_string().contains("duplicate backup entry"));
}

#[test]
fn manifest_counts_cover_lifetime_ledgers() {
    assert!(COUNTED_TABLES.contains(&"traffic_totals"));
    assert!(COUNTED_TABLES.contains(&"probe_traffic_totals"));
}
