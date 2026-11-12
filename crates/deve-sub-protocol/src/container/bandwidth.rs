//! Bandwidth units belong to the input format, not the shared numeric parser.

use crate::{ParseError, forward_proxy::invalid};
use serde_json::Value;

pub(super) fn mihomo(entry: &Value, field: &str) -> Result<Option<u64>, ParseError> {
    if entry.get(field).is_none() {
        return Ok(None);
    }
    let text = super::get_scalar_text(entry, field)
        .ok_or_else(|| invalid("Mihomo bandwidth must be numeric or text"))?;
    let value = text.trim();
    // WHY: Mihomo declares unitless up/down in Mbps; explicit bps remains bps.
    // The URI extension parser has a different existing unitless convention.
    let value = if value.chars().all(|c| c.is_ascii_digit() || c == '.') {
        format!("{value} Mbps")
    } else {
        value.to_owned()
    };
    crate::uri::parse_bandwidth(&value).map(Some)
}

pub(super) fn singbox(entry: &Value, field: &str) -> Result<Option<u64>, ParseError> {
    entry
        .get(field)
        .map(|value| {
            value
                .as_u64()
                .and_then(|mbps| mbps.checked_mul(1_000_000))
                .ok_or_else(|| {
                    invalid("sing-box bandwidth requires non-negative whole Mbps within range")
                })
        })
        .transpose()
}
