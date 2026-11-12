//! Certificate pin extraction shared by native TLS protocol parsers.

use deve_sub_domain::CertificatePin;
use serde_json::Value;

use crate::{ParseError, forward_proxy::invalid};

pub(super) fn mihomo(entry: &Value) -> Result<Vec<CertificatePin>, ParseError> {
    // WHY: Mihomo fingerprint accepts a matching chain certificate and
    // overrides ordinary CA verification. HY2 pinSHA256 is a leaf-only pin;
    // the two trust policies must not share a canonical namespace.
    match entry.get("fingerprint") {
        None => Ok(vec![]),
        Some(Value::String(pin)) if !pin.is_empty() => Ok(vec![CertificatePin::new(format!(
            "mihomoFingerprintSHA256:{pin}"
        ))]),
        _ => Err(invalid("certificate fingerprint must be text")),
    }
}

pub(super) fn xray(entry: &Value) -> Result<(), ParseError> {
    let Some(tls) = entry
        .get("streamSettings")
        .and_then(|v| v.get("tlsSettings"))
    else {
        return Ok(());
    };
    if [
        "pinnedPeerCertSha256",
        "pinnedPeerCertificateChainSha256",
        "pinnedPeerCertificatePublicKeySha256",
        "verifyPeerCertByName",
    ]
    .iter()
    .any(|key| tls.get(key).is_some())
    {
        return Err(invalid(
            "Xray/V2Ray certificate verification settings have no supported mapping",
        ));
    }
    Ok(())
}

pub(super) fn singbox(entry: &Value) -> Result<Vec<CertificatePin>, ParseError> {
    let Some(tls) = entry.get("tls") else {
        return Ok(vec![]);
    };
    // WHY: whole-certificate SHA256 is only available after pinned 1.14.3;
    // public-key SHA256 hashes different bytes and cannot substitute for it.
    if tls.get("certificate_sha256").is_some() {
        return Err(invalid(
            "whole-certificate pin is unsupported by sing-box 1.14",
        ));
    }
    let Some(value) = tls.get("certificate_public_key_sha256") else {
        return Ok(vec![]);
    };
    if tls.get("enabled").and_then(Value::as_bool) != Some(true) {
        return Err(invalid("certificate pins require enabled TLS"));
    }
    let values = match value {
        Value::String(_) => std::slice::from_ref(value),
        Value::Array(values) => values,
        _ => return Err(invalid("certificate pins must be text or a text list")),
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|v| !v.is_empty())
                .map(|pin| CertificatePin::new(format!("spkiSHA256:{pin}")))
                .ok_or_else(|| invalid("certificate pin must be text"))
        })
        .collect()
}
