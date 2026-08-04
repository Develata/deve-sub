//! TOTP (RFC 6238) — Time-based One-Time Password.
//!
//! Generates and verifies 6-digit TOTP codes using HMAC-SHA1, compatible with
//! Google Authenticator, Authy, 1Password, and other standard TOTP apps.
//!
//! Parameters: SHA-1, 6 digits, 30-second period, 20-byte secret. These are
//! the defaults that every TOTP app supports.

use data_encoding::BASE32;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use sha1::Sha1;
use time::OffsetDateTime;

use crate::SecurityError;

type HmacSha1 = Hmac<Sha1>;

/// TOTP step in seconds (RFC 6238 default).
pub const PERIOD: u64 = 30;

/// Number of digits in the TOTP code (RFC 6238 default).
pub const DIGITS: u32 = 6;

/// TOTP secret length in bytes (160 bits, standard for SHA-1).
pub const SECRET_LEN: usize = 20;

/// Allowed clock drift: ±1 step (±30 seconds). Standard practice to tolerate
/// minor clock skew between server and client.
const WINDOW: u64 = 1;

/// Unix epoch start (RFC 6238 T0).
const EPOCH: u64 = 0;

/// Generate a random 20-byte TOTP secret.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the OS entropy source fails.
pub fn generate_secret() -> Result<[u8; SECRET_LEN], SecurityError> {
    let mut secret = [0u8; SECRET_LEN];
    OsRng
        .try_fill_bytes(&mut secret)
        .map_err(|e| SecurityError::Crypto(format!("entropy source failure: {e}")))?;
    Ok(secret)
}

/// Encode a TOTP secret as Base32 (RFC 4648), the standard encoding used in
/// otpauth URIs and QR codes.
#[must_use]
pub fn base32_secret(secret: &[u8]) -> String {
    BASE32.encode(secret)
}

/// Decode a Base32-encoded TOTP secret back to raw bytes.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the input is not valid Base32.
pub fn base32_decode(s: &str) -> Result<Vec<u8>, SecurityError> {
    BASE32
        .decode(s.as_bytes())
        .map_err(|e| SecurityError::Crypto(format!("base32 decode failed: {e}")))
}

/// Compute the TOTP counter for a Unix timestamp.
fn counter(timestamp: u64) -> u64 {
    (timestamp - EPOCH) / PERIOD
}

/// Compute the HOTP value for a given counter (RFC 4226).
#[allow(clippy::expect_used)]
fn hotp(secret: &[u8], counter: u64) -> u32 {
    // WHY: HMAC-SHA1 accepts any key length (the KeyInit error is
    // GenericArray's InvalidLength, which HMAC never returns). This is
    // infallible per the hmac crate's implementation.
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();

    // Dynamic truncation (RFC 4226 §5.3): take the low 4 bits of the last byte
    // as offset, then extract 4 bytes at that offset.
    let offset = (result[result.len() - 1] & 0x0f) as usize;
    let truncated: u32 = ((u32::from(result[offset]) & 0x7f) << 24)
        | (u32::from(result[offset + 1]) << 16)
        | (u32::from(result[offset + 2]) << 8)
        | u32::from(result[offset + 3]);

    truncated % 10_u32.pow(DIGITS)
}

/// Generate a TOTP code for the given secret at the current time.
#[must_use]
pub fn generate_code(secret: &[u8]) -> u32 {
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    hotp(secret, counter(now))
}

/// Generate a TOTP code as a zero-padded string (e.g. `"012345"`).
#[must_use]
pub fn generate_code_string(secret: &[u8]) -> String {
    format!("{:0width$}", generate_code(secret), width = DIGITS as usize)
}

/// Verify a TOTP code against the secret, allowing ±1 step clock drift.
///
/// # Returns
/// `true` if the code matches any step in `[now - WINDOW, now + WINDOW]`.
#[must_use]
pub fn verify_code(secret: &[u8], code: u32) -> bool {
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    let current_counter = counter(now);

    // WHY: check all steps in the window to tolerate clock skew. We check
    // without early return to keep timing constant regardless of which step
    // matched (mitigates timing side-channel on the match position).
    let mut valid = false;
    for offset in 0..=2 * WINDOW {
        let candidate_counter = current_counter.saturating_sub(WINDOW) + offset;
        if hotp(secret, candidate_counter) == code {
            valid = true;
        }
    }
    valid
}

/// Percent-encode a string per RFC 3986 (unreserved characters only).
fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

/// Build an `otpauth://` URI for QR code generation.
///
/// The URI format is defined by Google Authenticator and widely supported:
/// ```text
/// otpauth://totp/<issuer>:<account>?secret=<base32>&issuer=<issuer>&algorithm=SHA1&digits=6&period=30
/// ```
///
/// The label and issuer are percent-encoded per RFC 3986.
#[must_use]
pub fn otpauth_uri(secret: &[u8], issuer: &str, account: &str) -> String {
    let secret_b32 = base32_secret(secret);
    let label = format!("{}:{}", percent_encode(issuer), percent_encode(account));
    let issuer_encoded = percent_encode(issuer);
    format!(
        "otpauth://totp/{label}?secret={secret_b32}&issuer={issuer_encoded}&algorithm=SHA1&digits={DIGITS}&period={PERIOD}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotp_rfc4226_test_vector() {
        // RFC 4226 Appendix D test values. Secret = "12345678901234567890"
        // (ASCII). Counter 0 → 755224, counter 1 → 287082, etc.
        let secret = b"12345678901234567890";
        assert_eq!(hotp(secret, 0), 755_224);
        assert_eq!(hotp(secret, 1), 287_082);
        assert_eq!(hotp(secret, 5), 254_676);
    }

    #[test]
    fn totp_rfc6238_test_vector() {
        // RFC 6238 Appendix B test vectors for SHA-1, truncated to 6 digits.
        // Secret = "12345678901234567890" (ASCII, 20 bytes).
        // T=59 (counter=1): 8-digit 94287082 → 6-digit 287082
        // T=1111111109 (counter=37037036): 8-digit 07081804 → 6-digit 081804
        let secret = b"12345678901234567890";
        assert_eq!(hotp(secret, counter(59)), 287_082);
        assert_eq!(hotp(secret, counter(1_111_111_109)), 81_804);
    }

    #[test]
    fn generate_and_verify_roundtrip() {
        let secret = generate_secret().expect("secret");
        let code = generate_code(&secret);
        assert!(verify_code(&secret, code));
    }

    #[test]
    fn verify_rejects_wrong_code() {
        let secret = generate_secret().expect("secret");
        assert!(!verify_code(&secret, 0));
    }

    #[test]
    fn base32_secret_roundtrip() {
        let secret = generate_secret().expect("secret");
        let encoded = base32_secret(&secret);
        let decoded = BASE32.decode(encoded.as_bytes()).expect("decode");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn otpauth_uri_contains_required_params() {
        let secret = b"test-secret-20-bytes!";
        let uri = otpauth_uri(secret, "Deve Sub", "admin");
        assert!(uri.starts_with("otpauth://totp/Deve%20Sub:admin?"));
        assert!(uri.contains("algorithm=SHA1"));
        assert!(uri.contains("digits=6"));
        assert!(uri.contains("period=30"));
    }
}
