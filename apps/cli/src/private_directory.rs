//! Private workspace creation for database snapshots, extraction and migration.
use std::{io, path::Path};

/// Restrict access in the creation syscall, before any sensitive file is written.
pub(crate) fn create(prefix: &str, parent: Option<&Path>) -> io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // tempfile directories default to 0777 masked by the caller's umask.
        // A private snapshot must never rely on that ambient process setting.
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    match parent {
        Some(parent) => builder.tempdir_in(parent),
        None => builder.tempdir(),
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::expect_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn system_and_sibling_workspaces_are_private() {
        let parent = tempfile::tempdir().expect("parent");
        for location in [None, Some(parent.path())] {
            let directory = super::create("deve-sub-private-test-", location).expect("private dir");
            assert_eq!(
                directory
                    .path()
                    .metadata()
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            if let Some(parent) = location {
                assert_eq!(directory.path().parent(), Some(parent));
            }
            let path = directory.path().to_path_buf();
            drop(directory);
            assert!(!path.exists(), "temporary workspace must still auto-clean");
        }
    }
}
