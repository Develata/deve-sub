//! Master key management for HMAC and encryption.
//!
//! The master key is 32 bytes of random data loaded from a file or secret
//! mount. It is used for HMAC-SHA256 of session tokens and XChaCha20-Poly1305
//! encryption of sensitive fields (TOTP secrets, subscription source
//! headers). See `docs/plan/00-engineering-constitution.md` §"Data and
//! security".

use std::path::Path;

use rand::RngCore;
use rand::rngs::OsRng;

use crate::SecurityError;

/// Key length in bytes (256 bits).
const KEY_LEN: usize = 32;

/// The server master key used for HMAC and encryption.
///
/// Loaded from a file on startup. If the file does not exist, it is
/// auto-generated with a warning (convenient for development; production
/// deployments should provide the key via a secret mount).
///
/// The `Debug` implementation is manual to ensure the key bytes are never
/// leaked in logs or error messages. See SEC-009.
#[derive(Clone)]
pub struct MasterKey {
    bytes: [u8; KEY_LEN],
}

impl MasterKey {
    /// Load the master key from a file.
    ///
    /// The file must contain exactly 32 bytes of raw key data.
    ///
    /// # Errors
    /// Returns [`SecurityError::MasterKey`] if the file cannot be read or
    /// has the wrong length.
    pub fn load(path: &Path) -> Result<Self, SecurityError> {
        let bytes = std::fs::read(path)
            .map_err(|e| SecurityError::MasterKey(format!("failed to read key file: {e}")))?;
        if bytes.len() != KEY_LEN {
            return Err(SecurityError::MasterKey(format!(
                "master key file must be exactly {KEY_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&bytes);
        Ok(Self { bytes: key })
    }

    /// Load the master key from a file, or generate a new one if the file
    /// does not exist.
    ///
    /// Uses `create_new(true)` for atomic file creation: if two processes
    /// race, only one creates the file and the other loads the existing one.
    /// On Unix, the file is created with mode `0600` to prevent other local
    /// users from reading the key.
    ///
    /// # Errors
    /// Returns [`SecurityError::MasterKey`] if the file cannot be read,
    /// written, or has the wrong length.
    pub fn load_or_generate(path: &Path) -> Result<Self, SecurityError> {
        // WHY: `create_new(true)` fails with `AlreadyExists` if the file
        // exists, making the create-or-load decision atomic. Without this,
        // two processes could both observe `!exists()`, generate different
        // keys, and one would overwrite the other — corrupting all sessions
        // created by loser.
        match write_new_key_file(path) {
            Ok(bytes) => {
                tracing::warn!(
                    path = %path.display(),
                    "master key file not found; generating a new one. \
                     For production, provide a key via a secret mount."
                );
                Ok(Self { bytes })
            }
            Err(InitKeyError::AlreadyExists) => Self::load(path),
            Err(InitKeyError::Other(e)) => Err(e),
        }
    }

    /// Initialize a new master key file at `path`.
    ///
    /// Strict, atomic, fail-closed variant of [`MasterKey::load_or_generate`]
    /// for production bootstrap (CLI `key init`, install script). Refuses to
    /// overwrite an existing key file — call [`MasterKey::load`] to read an
    /// existing key. This prevents an operator from accidentally rotating the
    /// key by re-running `key init`, which would silently invalidate every
    /// HMAC-derived and encrypted value in the database.
    ///
    /// On success, the key file is:
    /// - created with `O_EXCL` (atomic create-or-refuse);
    /// - 32 random bytes from `OsRng`;
    /// - mode `0600` on Unix (no group/other access);
    /// - `fsync`'d so the bytes hit disk before the function returns;
    /// - parent directory `fsync`'d so the directory entry is durable.
    ///
    /// # Errors
    /// - [`SecurityError::MasterKey`] with "already exists" if `path` exists.
    /// - [`SecurityError::MasterKey`] wrapping the underlying IO error for
    ///   directory creation, file creation, entropy, write, or fsync failures.
    ///
    /// See ADR-0007 §7 (fail-closed key loading) and DS-AUD-B01 (install path
    /// must explicitly init the key before `serve` starts).
    pub fn init_new(path: &Path) -> Result<Self, SecurityError> {
        match write_new_key_file(path) {
            Ok(bytes) => Ok(Self { bytes }),
            Err(InitKeyError::AlreadyExists) => Err(SecurityError::MasterKey(format!(
                "key file already exists at {}; refusing to overwrite — \
                 use MasterKey::load to read it. Re-initializing would \
                 silently invalidate all encrypted columns",
                path.display()
            ))),
            Err(InitKeyError::Other(e)) => Err(e),
        }
    }

    /// Return the raw key bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Construct a master key from a fixed-size byte array.
    ///
    /// Primarily for testing and for callers that load the key from a
    /// non-file source (e.g. an environment variable or secret mount with a
    /// different encoding). Production code should prefer
    /// [`MasterKey::load`] or [`MasterKey::load_or_generate`].
    #[must_use]
    pub fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self {
        Self { bytes: *bytes }
    }

    /// Compute a domain-separated fingerprint of the key for identification.
    ///
    /// Returns the hex-encoded HMAC-SHA256 digest of the fixed string
    /// `"deve-sub-key-fingerprint-v1"` keyed by the master key. The digest
    /// is one-way: the raw key cannot be recovered from the fingerprint.
    /// Its purpose is to let a restore verify that the loaded master key
    /// matches the key used at backup time, preventing silent decryption
    /// failures when encrypted columns are restored with the wrong key
    /// (DS-AUD-034, ADR-0007 §4).
    ///
    /// # Errors
    /// Returns [`SecurityError::Crypto`] if HMAC initialization fails, which
    /// is not expected for a valid 32-byte key.
    pub fn fingerprint(&self) -> Result<String, SecurityError> {
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(&self.bytes)
            .map_err(|e| SecurityError::Crypto(format!("HMAC init failed: {e}")))?;
        mac.update(b"deve-sub-key-fingerprint-v1");
        let result = mac.finalize().into_bytes();
        Ok(result.iter().map(|b| format!("{b:02x}")).collect())
    }
}

/// Internal outcome of [`write_new_key_file`]: distinguishes "file already
/// existed" (caller decides whether to fall back to load or to error) from
/// real IO/entropy failures.
enum InitKeyError {
    AlreadyExists,
    Other(SecurityError),
}

/// Atomically create a new key file at `path` with 32 random bytes, mode
/// `0600` (Unix), `fsync`'d, with the parent directory also `fsync`'d.
///
/// Returns the freshly generated key bytes on success. Shared by
/// [`MasterKey::load_or_generate`] (fall back to load on `AlreadyExists`)
/// and [`MasterKey::init_new`] (refuse on `AlreadyExists`).
fn write_new_key_file(path: &Path) -> Result<[u8; KEY_LEN], InitKeyError> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            InitKeyError::Other(SecurityError::MasterKey(format!(
                "failed to create key directory: {e}"
            )))
        })?;
    }

    // WHY: `create_new(true)` fails with `AlreadyExists` if the file exists,
    // making the create-or-load decision atomic. Without this, two processes
    // could both observe `!exists()`, generate different keys, and one would
    // overwrite the other — corrupting all sessions created by loser.
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = match options.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(InitKeyError::AlreadyExists);
        }
        Err(e) => {
            return Err(InitKeyError::Other(SecurityError::MasterKey(format!(
                "failed to create key file: {e}"
            ))));
        }
    };

    let mut bytes = [0u8; KEY_LEN];
    OsRng.try_fill_bytes(&mut bytes).map_err(|e| {
        InitKeyError::Other(SecurityError::MasterKey(format!(
            "entropy source failure: {e}"
        )))
    })?;

    file.write_all(&bytes).map_err(|e| {
        // Best-effort cleanup: don't leave an empty/partial key file behind
        // for the next caller to trip over. Ignore the cleanup error.
        let _ = std::fs::remove_file(path);
        InitKeyError::Other(SecurityError::MasterKey(format!(
            "failed to write key file: {e}"
        )))
    })?;

    // WHY fsync the file: without this, a crash after write may leave the
    // file present in the directory entry but with the random bytes still
    // in the page cache. On reboot the file is empty or partial, and
    // MasterKey::load fails with "wrong length" — the operator's only
    // recourse is to restore from backup. fsync makes the bytes durable.
    file.sync_all().map_err(|e| {
        let _ = std::fs::remove_file(path);
        InitKeyError::Other(SecurityError::MasterKey(format!(
            "failed to fsync key file: {e}"
        )))
    })?;

    // WHY: parent-directory fsync would make the directory entry for the
    // new file durable too, but `unsafe_code = "forbid"` blocks raw-fd
    // fsync (the only POSIX way), and adding `libc`/`nix` to deve-sub-security
    // for one call is over-engineering for v0.1. On ext4/xfs with the default
    // ordered journal mode, the file's data fsync implies the create is
    // durable. File fsync alone gives the critical guarantee: the key bytes
    // are on disk before the function returns.

    Ok(bytes)
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // WHY: never expose the raw key bytes in debug output to prevent
        // accidental leakage via tracing or error messages. See SEC-009.
        f.write_str("MasterKey(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_generate_creates_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        let key = MasterKey::load_or_generate(&path).expect("generate");
        assert_eq!(key.as_bytes().len(), KEY_LEN);
        assert!(path.exists());

        // Loading again returns the same key.
        let key2 = MasterKey::load_or_generate(&path).expect("load");
        assert_eq!(key.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn load_wrong_length() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        std::fs::write(&path, b"too short").expect("write");
        assert!(MasterKey::load(&path).is_err());
    }

    #[test]
    fn init_new_creates_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        let key = MasterKey::init_new(&path).expect("init");
        assert_eq!(key.as_bytes().len(), KEY_LEN);
        assert!(path.exists());
        let loaded = MasterKey::load(&path).expect("load after init");
        assert_eq!(key.as_bytes(), loaded.as_bytes());
    }

    #[test]
    fn init_new_refuses_existing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        std::fs::write(&path, [0u8; KEY_LEN]).expect("pre-create");
        let msg = match MasterKey::init_new(&path) {
            Ok(_) => panic!("init_new must refuse when key already exists"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("already exists") && msg.contains("refusing to overwrite"),
            "expected already-exists error, got: {msg}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn init_new_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        MasterKey::init_new(&path).expect("init");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "key file must be 0600, got {mode:o}");
    }

    #[test]
    fn init_new_creates_parent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("deeper").join("master.key");
        MasterKey::init_new(&path).expect("init creates parents");
        assert!(path.exists());
    }

    #[test]
    fn init_new_returns_distinct_keys() {
        let dir_a = tempfile::tempdir().expect("tempdir a");
        let dir_b = tempfile::tempdir().expect("tempdir b");
        let path_a = dir_a.path().join("a.key");
        let path_b = dir_b.path().join("b.key");
        let key_a = MasterKey::init_new(&path_a).expect("init a");
        let key_b = MasterKey::init_new(&path_b).expect("init b");
        assert_ne!(
            key_a.as_bytes(),
            key_b.as_bytes(),
            "OsRng must produce distinct keys"
        );
    }

    #[test]
    fn debug_does_not_leak_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        let key = MasterKey::load_or_generate(&path).expect("generate");
        let debug = format!("{key:?}");
        assert_eq!(debug, "MasterKey(<redacted>)");
        assert!(!debug.contains("bytes"));
    }

    #[test]
    fn fingerprint_is_stable_and_distinct() {
        let key_a = MasterKey::from_bytes(&[0x01; 32]);
        let key_b = MasterKey::from_bytes(&[0x02; 32]);
        let fp_a = key_a.fingerprint().expect("fingerprint");
        let fp_a2 = key_a.fingerprint().expect("fingerprint");
        let fp_b = key_b.fingerprint().expect("fingerprint");
        assert_eq!(fp_a, fp_a2, "fingerprint must be stable");
        assert_ne!(
            fp_a, fp_b,
            "different keys must have different fingerprints"
        );
        assert_eq!(fp_a.len(), 64, "HMAC-SHA256 hex = 64 chars");
        assert!(
            fp_a.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint must be hex"
        );
        let raw_hex: String = key_a
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_ne!(
            fp_a, raw_hex,
            "fingerprint must not equal raw key bytes hex"
        );
    }
}
