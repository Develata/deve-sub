//! Recovery code generation for 2FA fallback.
//!
//! Recovery codes are high-entropy one-time codes that allow users to regain
//! access if they lose their TOTP device. Each code is 10 characters from a
//! 32-character alphabet (no ambiguous characters), formatted as `XXXXX-XXXXX`.
//! Codes are stored as HMAC-SHA256 hashes (via [`crate::hmac_digest`] with
//! and are single-use.

use rand::RngCore;
use rand::rngs::OsRng;

use crate::SecurityError;

/// Number of recovery codes generated per batch.
pub const CODE_COUNT: usize = 10;

/// Characters per recovery code (excluding the dash separator).
const CODE_CHARS: usize = 10;

/// Alphabet excluding ambiguous characters: no 0, 1, I, O.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Number of random bytes per code. One byte per character; 10 chars from a
/// 32-char alphabet ≈ 50 bits of entropy per code, far beyond any brute-force
/// threshold.
const RAND_BYTES: usize = CODE_CHARS;

/// Generate a single recovery code string in `XXXXX-XXXXX` format.
fn generate_one() -> Result<String, SecurityError> {
    let mut buf = [0u8; RAND_BYTES];
    OsRng
        .try_fill_bytes(&mut buf)
        .map_err(|e| SecurityError::Crypto(format!("entropy source failure: {e}")))?;

    // WHY: use modulo rejection sampling to avoid modular bias. For a 32-char
    // alphabet, 256 % 32 = 0, so there is no bias — every byte maps evenly.
    // This assertion documents the reasoning; if ALPHABET.len() changes, the
    // bias-free property must be rechecked.
    assert_eq!(ALPHABET.len(), 32);
    assert_eq!(256 % ALPHABET.len(), 0);

    let mut chars = String::with_capacity(CODE_CHARS + 1);
    // WHY: one random byte per character. Previously RAND_BYTES was 8 with a
    // cycling `buf[i % 8]` index, which made chars[8]==chars[0] and
    // chars[9]==chars[1] deterministically — collapsing entropy to 2^40.
    for &byte in &buf {
        chars.push(ALPHABET[byte as usize % ALPHABET.len()] as char);
    }

    // Insert dash separator in the middle: XXXXX-XXXXX
    Ok(format!("{}-{}", &chars[..5], &chars[5..]))
}

/// Generate a batch of recovery codes.
///
/// Returns `CODE_COUNT` unique codes in `XXXXX-XXXXX` format.
///
/// # Errors
/// Returns [`SecurityError::Crypto`] if the OS entropy source fails.
pub fn generate_codes() -> Result<Vec<String>, SecurityError> {
    let mut codes = Vec::with_capacity(CODE_COUNT);
    while codes.len() < CODE_COUNT {
        let code = generate_one()?;
        // WHY: deduplicate within the batch. Collision probability is
        // negligible (2^50 entropy, 10 codes), but dedup is cheap insurance.
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    Ok(codes)
}

/// Normalize a recovery code by stripping non-alphanumeric characters and
/// converting to uppercase. The dash separator is not part of the stored
/// hash, so `"ABCDE-FGHIJ"`, `"ABCDE FGHIJ"`, and `"abcdefghij"` all
/// normalize to `"ABCDEFGHIJ"`.
#[must_use]
pub fn normalize_code(code: &str) -> String {
    code.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_correct_format() {
        let codes = generate_codes().expect("codes");
        assert_eq!(codes.len(), CODE_COUNT);
        for code in &codes {
            // Format: XXXXX-XXXXX (11 chars)
            assert_eq!(code.len(), 11);
            assert_eq!(code.chars().nth(5), Some('-'));
            // All non-dash chars are from the alphabet
            for c in code.chars().filter(|c| *c != '-') {
                assert!(ALPHABET.contains(&(c as u8)));
            }
        }
    }

    #[test]
    fn codes_are_unique() {
        let codes = generate_codes().expect("codes");
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), CODE_COUNT);
    }

    #[test]
    fn normalize_strips_and_uppercases() {
        assert_eq!(normalize_code(" abcde-fghij "), "ABCDEFGHIJ");
        assert_eq!(normalize_code("ABCDE FGHIJ"), "ABCDEFGHIJ");
        assert_eq!(normalize_code("abcde-fghij"), "ABCDEFGHIJ");
    }

    #[test]
    fn chars_are_independent_no_cycling_bug() {
        // Regression guard for the buf[i % RAND_BYTES] bug: with RAND_BYTES=8
        // and a 10-char code, chars[8]==chars[0] and chars[9]==chars[1] for
        // every code. With correct one-byte-per-char randomness, the
        // probability of this pattern holding for all codes across multiple
        // batches is ~(1/32)^(2*100) — astronomically unlikely.
        let mut all_repeated = true;
        for _ in 0..10 {
            let codes = generate_codes().expect("codes");
            for code in &codes {
                let norm: String = code.chars().filter(|c| *c != '-').collect();
                if !(norm.as_bytes()[8] == norm.as_bytes()[0]
                    && norm.as_bytes()[9] == norm.as_bytes()[1])
                {
                    all_repeated = false;
                    break;
                }
            }
            if !all_repeated {
                break;
            }
        }
        assert!(
            !all_repeated,
            "chars[8:10] must not deterministically equal chars[0:2]"
        );
    }
}
