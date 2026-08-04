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
        use std::fs::OpenOptions;

        // WHY: `create_new(true)` fails with `AlreadyExists` if the file
        // exists, making the create-or-load decision atomic. Without this,
        // two processes could both observe `!exists()`, generate different
        // keys, and one would overwrite the other — corrupting all sessions
        // created by the loser.
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        match options.open(path) {
            Ok(mut file) => {
                tracing::warn!(
                    path = %path.display(),
                    "master key file not found; generating a new one. \
                     For production, provide a key via a secret mount."
                );

                let mut bytes = [0u8; KEY_LEN];
                OsRng.try_fill_bytes(&mut bytes).map_err(|e| {
                    SecurityError::MasterKey(format!("entropy source failure: {e}"))
                })?;

                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        SecurityError::MasterKey(format!("failed to create key directory: {e}"))
                    })?;
                }

                use std::io::Write;
                file.write_all(&bytes).map_err(|e| {
                    SecurityError::MasterKey(format!("failed to write key file: {e}"))
                })?;

                Ok(Self { bytes })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Self::load(path),
            Err(e) => Err(SecurityError::MasterKey(format!(
                "failed to create key file: {e}"
            ))),
        }
    }

    /// Return the raw key bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
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
    fn debug_does_not_leak_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("master.key");
        let key = MasterKey::load_or_generate(&path).expect("generate");
        let debug = format!("{key:?}");
        assert_eq!(debug, "MasterKey(<redacted>)");
        assert!(!debug.contains("bytes"));
    }
}
