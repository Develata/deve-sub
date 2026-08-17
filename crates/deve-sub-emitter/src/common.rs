//! Shared encoding constants and helpers for URI emitters.
//!
//! These constants and functions are used by all protocol emitters to ensure
//! consistent percent-encoding and query-string construction.

use percent_encoding::{AsciiSet, CONTROLS};

use deve_sub_domain::CertificatePin;

/// Percent-encode set for URI userinfo (RFC 3986 §3.2.1). Encodes all
/// structural delimiters that would break URI parsing: `@` (userinfo/host
/// boundary), `:` (username/password boundary), `/` (path), `?` (query),
/// `#` (fragment), `[` `]` (IPv6), `%` (escape char), plus control chars
/// and other sub-delimiters that are unsafe in userinfo.
pub(crate) const USERINFO_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'&')
    .add(b'?')
    .add(b'/')
    .add(b'@')
    .add(b':')
    .add(b'[')
    .add(b']')
    .add(b'%');

/// Percent-encode set for URI fragments: control chars plus characters that
/// would break the URI structure.
pub(crate) const FRAGMENT_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'&')
    .add(b'?')
    .add(b'%');

/// Percent-encode set for query parameter values. Encodes delimiters and
/// structural characters that would break query parsing, but leaves path-safe
/// characters like `/` unencoded for readability and golden-test stability.
pub(crate) const QUERY_VALUE_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'&')
    .add(b'+')
    .add(b'=')
    .add(b'%');

/// Percent-encode a credential for the URI userinfo component.
pub(crate) fn encode_userinfo(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, USERINFO_ENCODE).to_string()
}

/// Build a query string from ordered key-value pairs, percent-encoding values.
pub(crate) fn format_query(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}={v}",
                v = percent_encoding::utf8_percent_encode(v, QUERY_VALUE_ENCODE)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode a display name for use as a URI fragment.
pub(crate) fn format_fragment(name: &str) -> String {
    percent_encoding::utf8_percent_encode(name, FRAGMENT_ENCODE).to_string()
}

/// Format certificate pins as a comma-separated string, stripping the
/// `pinSHA256:` prefix if present.
pub(crate) fn format_pins(pins: &[CertificatePin]) -> String {
    pins.iter()
        .map(|pin| {
            pin.as_str()
                .strip_prefix("pinSHA256:")
                .unwrap_or(pin.as_str())
        })
        .collect::<Vec<&str>>()
        .join(",")
}
///
/// Chooses the largest unit that divides evenly: `Gbps`, `Mbps`, `Kbps`, or
/// `bps`. This ensures deterministic round-trip output for common values.
pub(crate) fn format_bandwidth(bps: u64) -> String {
    if bps >= 1_000_000_000 && bps.is_multiple_of(1_000_000_000) {
        format!("{} Gbps", bps / 1_000_000_000)
    } else if bps >= 1_000_000 && bps.is_multiple_of(1_000_000) {
        format!("{} Mbps", bps / 1_000_000)
    } else if bps >= 1_000 && bps.is_multiple_of(1_000) {
        format!("{} Kbps", bps / 1_000)
    } else {
        format!("{} bps", bps)
    }
}
