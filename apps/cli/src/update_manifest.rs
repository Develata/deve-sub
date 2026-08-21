//! Signed update manifest — publisher authentication via Ed25519 (B-09).
//!
//! DS-AUD-B09: the previous self-update fetched a `checksums.txt` from the
//! same unsigned release as the binary. That guards against transport
//! corruption but cannot authenticate the publisher — anyone who can push a
//! release can ship a malicious binary with a matching checksum.
//!
//! This module replaces the unsigned checksum with a signed manifest:
//!
//! - The release includes `deve-sub-manifest.json` (version, target triple,
//!   per-asset sha256 + size) and `deve-sub-manifest.json.sig` (Ed25519
//!   signature over the raw manifest bytes).
//! - `deve-sub update` fetches both, verifies the signature against a
//!   compile-time-embedded public key, and only then trusts the asset hashes.
//! - A tampered manifest, a signature from a different key, or a missing
//!   signature all cause the update to abort before any binary is downloaded.
//!
//! Key rotation: the embedded key is the release signing key. To rotate,
//! embed the new key, cut a release signed with it, and ship. Old binaries
//! that still have the old key will refuse updates signed by the new key
//! until they are manually upgraded — this is intentional (no out-of-band
//! key channel exists, so automatic rotation would be indistinguishable from
//! a compromise).

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Compile-time-embedded Ed25519 public key for manifest verification.
///
/// WHY hardcoded: there is no out-of-band key-distribution channel for a
/// self-hosted single-binary tool. Embedding the key in the binary means the
/// trust root is the binary the operator initially installed. A compromise of
/// the release key does not retroactively affect already-installed binaries.
///
/// The key below is the development release key (P0-04). It MUST be rotated
/// to a production key before the first public release. The corresponding
/// private key seed is stored as a GitHub Actions secret
/// (`DEVE_SUB_RELEASE_KEY_SEED`).
///
/// WHY the seed must differ from any value that has ever appeared in the
/// repository: Ed25519 derives the public key deterministically from the
/// seed, so anyone with the seed can forge signatures. The development seed
/// was used to generate this public key and the fixture in
/// `tests/fixtures/`; it is NOT the production secret. Before the first
/// public release, rotate to a fresh seed generated on an air-gapped host,
/// update this constant to the new public key, regenerate the fixture, and
/// store the new seed as `DEVE_SUB_RELEASE_KEY_SEED`.
///
/// See `scripts/sign-release-manifest.sh` for the signing procedure and
/// `scripts/verify-release-key.sh` for the CI seed↔public-key check.
const RELEASE_PUBLIC_KEY: [u8; 32] = [
    0x0f, 0x38, 0xc5, 0x97, 0x58, 0xf1, 0x98, 0x54, 0x70, 0x22, 0x31, 0xf1, 0xb8, 0x8d, 0xe1, 0xaa,
    0x69, 0x37, 0xcf, 0xc7, 0x20, 0x57, 0x44, 0xc2, 0xee, 0x44, 0xeb, 0x6d, 0x7b, 0x35, 0x5c, 0x76,
];

/// An asset listed in the signed manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestAsset {
    /// Asset filename, e.g. `deve-sub-linux-amd64`.
    pub name: String,
    /// Hex-encoded SHA-256 of the asset content.
    pub sha256: String,
    /// Asset size in bytes (verified during streaming download).
    pub size: u64,
}

/// The signed manifest payload. Serialized as JSON and signed as raw bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedManifest {
    /// Release version (SemVer, without leading `v`).
    pub version: String,
    /// Target triple, e.g. `x86_64-unknown-linux-gnu`.
    pub target: String,
    /// Assets with their expected hashes and sizes.
    pub assets: Vec<ManifestAsset>,
}

impl SignedManifest {
    /// Look up an asset by name.
    pub fn find_asset(&self, name: &str) -> Option<&ManifestAsset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// Verify an Ed25519 signature over the raw manifest bytes and deserialize
/// the manifest. Returns `Err` if the signature is invalid, from the wrong
/// key, or the manifest JSON is malformed.
///
/// `manifest_bytes` is the raw JSON body of `deve-sub-manifest.json`.
/// `signature_bytes` is the raw 64-byte Ed25519 signature.
pub fn verify_signed_manifest(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
) -> Result<SignedManifest> {
    // WHY: parse the key at runtime rather than const-initialize, because
    // VerifyingKey::from_bytes is not yet const-constructable. The embedded
    // key is a fixed constant, so this is a zero-cost parse at call time.
    let public_key = VerifyingKey::from_bytes(&RELEASE_PUBLIC_KEY)
        .context("internal error: embedded release public key is malformed")?;

    let signature = Signature::from_slice(signature_bytes).context("invalid signature encoding")?;

    public_key
        .verify(manifest_bytes, &signature)
        .context("manifest signature verification failed — refusing untrusted update")?;

    let manifest: SignedManifest =
        serde_json::from_slice(manifest_bytes).context("failed to parse signed manifest JSON")?;

    Ok(manifest)
}

/// Encode an Ed25519 signature as base64 (for storage/transmission as a
/// `.sig` file alongside the manifest).
//
// WHY allow(dead_code): these base64 helpers are exercised by the test module
// (base64_signature_round_trip) and will be used by the future release
// signing tool. The update binary path uses raw 64-byte signatures directly.
#[allow(dead_code)]
pub fn signature_to_base64(sig: &Signature) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
}

/// Decode a base64-encoded Ed25519 signature (from a `.sig` file).
#[allow(dead_code)]
pub fn signature_from_base64(s: &str) -> Result<Signature> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .context("signature is not valid base64")?;
    Signature::from_slice(&bytes).context("decoded signature is not 64 bytes")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    /// Generate a signing keypair, create a manifest, sign it, and verify.
    /// This is the happy-path round-trip. Uses a freshly generated key (not
    /// the embedded RELEASE_PUBLIC_KEY) because the embedded key is a
    /// placeholder with no known private counterpart.
    fn make_signed_manifest(
        signing_key: &SigningKey,
        version: &str,
        target: &str,
        assets: Vec<ManifestAsset>,
    ) -> (Vec<u8>, Signature) {
        let manifest = SignedManifest {
            version: version.to_owned(),
            target: target.to_owned(),
            assets,
        };
        let json = serde_json::to_vec(&manifest).unwrap();
        let sig = signing_key.sign(&json);
        (json, sig)
    }

    #[test]
    fn sign_verify_round_trip() {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let assets = vec![ManifestAsset {
            name: "deve-sub-linux-amd64".to_owned(),
            sha256: "abc123".to_owned(),
            size: 1024,
        }];
        let (json, sig) =
            make_signed_manifest(&signing_key, "0.2.0", "x86_64-unknown-linux-gnu", assets);

        // Verify with the correct public key.
        let verifying_key = signing_key.verifying_key();
        let public_key_bytes = verifying_key.to_bytes();
        let result = verify_with_key(&public_key_bytes, &json, &sig.to_bytes());
        assert!(result.is_ok(), "valid signature should verify");
        let manifest = result.unwrap();
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].name, "deve-sub-linux-amd64");
    }

    #[test]
    fn tampered_manifest_rejected() {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let (mut json, sig) =
            make_signed_manifest(&signing_key, "0.2.0", "x86_64-unknown-linux-gnu", vec![]);
        // Tamper: flip a byte in the JSON.
        json[0] ^= 0xff;
        let verifying_key = signing_key.verifying_key();
        let result = verify_with_key(&verifying_key.to_bytes(), &json, &sig.to_bytes());
        assert!(result.is_err(), "tampered manifest should be rejected");
    }

    #[test]
    fn wrong_key_rejected() {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let (json, sig) =
            make_signed_manifest(&signing_key, "0.2.0", "x86_64-unknown-linux-gnu", vec![]);
        // Different key.
        let other_key = SigningKey::generate(&mut rng);
        let result = verify_with_key(
            &other_key.verifying_key().to_bytes(),
            &json,
            &sig.to_bytes(),
        );
        assert!(
            result.is_err(),
            "signature from wrong key should be rejected"
        );
    }

    #[test]
    fn malformed_signature_rejected() {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let (json, _sig) =
            make_signed_manifest(&signing_key, "0.2.0", "x86_64-unknown-linux-gnu", vec![]);
        let bad_sig = [0u8; 64]; // 64 zero bytes — valid length, invalid signature
        let result = verify_with_key(&signing_key.verifying_key().to_bytes(), &json, &bad_sig);
        assert!(result.is_err(), "all-zero signature should be rejected");
    }

    #[test]
    fn truncated_signature_rejected() {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let (json, _sig) =
            make_signed_manifest(&signing_key, "0.2.0", "x86_64-unknown-linux-gnu", vec![]);
        let bad_sig = [0u8; 32]; // too short
        let result = verify_with_key(&signing_key.verifying_key().to_bytes(), &json, &bad_sig);
        assert!(result.is_err(), "truncated signature should be rejected");
    }

    #[test]
    fn base64_signature_round_trip() {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let sig = signing_key.sign(b"test message");
        let encoded = signature_to_base64(&sig);
        let decoded = signature_from_base64(&encoded).unwrap();
        assert_eq!(sig, decoded);
    }

    #[test]
    fn find_asset_by_name() {
        let manifest = SignedManifest {
            version: "0.2.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            assets: vec![
                ManifestAsset {
                    name: "deve-sub-linux-amd64".to_owned(),
                    sha256: "aaa".to_owned(),
                    size: 1,
                },
                ManifestAsset {
                    name: "deve-sub-linux-arm64".to_owned(),
                    sha256: "bbb".to_owned(),
                    size: 2,
                },
            ],
        };
        assert!(manifest.find_asset("deve-sub-linux-amd64").is_some());
        assert!(manifest.find_asset("deve-sub-linux-arm64").is_some());
        assert!(manifest.find_asset("nonexistent").is_none());
    }

    /// P0-04: verify that the fixture manifest — signed offline by the
    /// release key holder using `scripts/sign-release-manifest.sh` (Python
    /// `cryptography` Ed25519) — passes `verify_signed_manifest`, which uses
    /// the embedded `RELEASE_PUBLIC_KEY` (Rust `ed25519_dalek`).
    ///
    /// This is the cross-language compatibility proof: Python signs, Rust
    /// verifies, and the embedded public key accepts the signature. The
    /// fixture contains only the manifest JSON and the 64-byte signature —
    /// the signing seed is NEVER in the repository.
    ///
    /// WHY fixture-based (not seed-based): hardcoding the seed in a test
    /// would publish the private key, defeating the entire signing scheme.
    /// The seed↔public-key correspondence is verified in CI (see
    /// `scripts/verify-release-key.sh`) where the secret is available.
    #[test]
    fn embedded_key_verifies_fixture_signature() {
        let manifest_bytes = include_bytes!("../tests/fixtures/test-manifest.json");
        let sig_bytes = include_bytes!("../tests/fixtures/test-manifest.json.sig");

        let result = verify_signed_manifest(manifest_bytes, sig_bytes);
        assert!(
            result.is_ok(),
            "fixture signed by release key must verify against embedded public key"
        );
        let manifest = result.unwrap();
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.target, "x86_64-unknown-linux-gnu");
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].name, "deve-sub-linux-amd64");
    }

    /// Test helper: verify with an explicit public key (not the embedded one).
    fn verify_with_key(
        public_key: &[u8; 32],
        manifest_bytes: &[u8],
        signature_bytes: &[u8],
    ) -> Result<SignedManifest> {
        let public_key =
            VerifyingKey::from_bytes(public_key).context("malformed test public key")?;
        let signature = Signature::from_slice(signature_bytes).context("invalid signature")?;
        public_key
            .verify(manifest_bytes, &signature)
            .context("signature verification failed")?;
        let manifest: SignedManifest =
            serde_json::from_slice(manifest_bytes).context("failed to parse manifest")?;
        Ok(manifest)
    }
}
