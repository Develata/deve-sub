//! Explicit TLS mapping for ordinary proxies; unknown security settings fail closed.

use deve_sub_domain::{CertificatePin, TlsConfig};
use serde_json::Value;

use super::forward_proxy::{allowed, bool_field, string};
use crate::{ParseError, forward_proxy::invalid};

pub(super) fn mihomo(entry: &Value, https: bool) -> Result<Option<TlsConfig>, ParseError> {
    // WHY: two SNI aliases must not discard one authored security setting.
    if entry.get("sni").is_some() && entry.get("servername").is_some() {
        return Err(invalid("multiple proxy SNI representations"));
    }
    let enabled = bool_field(entry, "tls")?.unwrap_or(https);
    if https && !enabled {
        return Err(invalid("HTTPS cannot disable TLS"));
    }
    let mut tls = super::default_tls_enabled();
    tls.enabled = enabled;
    tls.server_name = string(entry, "sni")?.or(string(entry, "servername")?);
    tls.skip_cert_verify = bool_field(entry, "skip-cert-verify")?;
    tls.alpn = strings(entry, "alpn")?;
    tls.client_fingerprint = string(entry, "client-fingerprint")?;
    if let Some(pin) = string(entry, "fingerprint")? {
        tls.certificate_pins.push(CertificatePin::new(format!(
            "mihomoFingerprintSHA256:{pin}"
        )));
    }
    finish(tls, entry.get("tls").is_some())
}

pub(super) fn singbox(value: Option<&Value>) -> Result<Option<TlsConfig>, ParseError> {
    let Some(value) = value else { return Ok(None) };
    allowed(
        value,
        &[
            "enabled",
            "server_name",
            "insecure",
            "alpn",
            "utls",
            "certificate_public_key_sha256",
        ],
    )?;
    let mut tls = super::default_tls_enabled();
    tls.enabled = bool_field(value, "enabled")?.unwrap_or(false);
    tls.server_name = string(value, "server_name")?;
    tls.skip_cert_verify = bool_field(value, "insecure")?;
    tls.alpn = strings(value, "alpn")?;
    if let Some(utls) = value.get("utls") {
        allowed(utls, &["enabled", "fingerprint"])?;
        if bool_field(utls, "enabled")? != Some(true) {
            return Err(invalid("disabled uTLS settings cannot be represented"));
        }
        // WHY: enabled uTLS without a concrete fingerprint has no distinct
        // canonical representation; treating it as absent would disable it.
        tls.client_fingerprint = Some(
            string(utls, "fingerprint")?
                .filter(|fingerprint| !fingerprint.is_empty())
                .ok_or_else(|| invalid("uTLS requires an explicit fingerprint"))?,
        );
    }
    tls.certificate_pins = strings(value, "certificate_public_key_sha256")?
        .into_iter()
        .map(|pin| CertificatePin::new(format!("spkiSHA256:{pin}")))
        .collect();
    finish(tls, true)
}

pub(super) fn xray(value: Option<&Value>) -> Result<Option<TlsConfig>, ParseError> {
    let Some(value) = value else { return Ok(None) };
    allowed(value, &["network", "security", "tlsSettings"])?;
    if string(value, "network")?.is_some_and(|v| v != "tcp" && v != "raw") {
        return Err(invalid("forward proxy requires raw TCP transport"));
    }
    match string(value, "security")?.as_deref() {
        None if value.get("tlsSettings").is_none() => return Ok(None),
        Some("none") if value.get("tlsSettings").is_none() => {
            let mut tls = super::default_tls_enabled();
            tls.enabled = false;
            return Ok(Some(tls));
        }
        Some("tls") => {}
        _ => return Err(invalid("unsupported proxy transport security")),
    }
    let empty = serde_json::json!({});
    let value = value.get("tlsSettings").unwrap_or(&empty);
    allowed(
        value,
        &["serverName", "allowInsecure", "alpn", "fingerprint"],
    )?;
    let mut tls = super::default_tls_enabled();
    tls.server_name = string(value, "serverName")?;
    tls.skip_cert_verify = bool_field(value, "allowInsecure")?;
    tls.alpn = strings(value, "alpn")?;
    tls.client_fingerprint = string(value, "fingerprint")?;
    Ok(Some(tls))
}

fn strings(value: &Value, key: &str) -> Result<Vec<String>, ParseError> {
    let Some(value) = value.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| invalid("expected a string list"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("expected a string list"))
        })
        .collect()
}

fn finish(tls: TlsConfig, explicit: bool) -> Result<Option<TlsConfig>, ParseError> {
    if !tls.enabled
        && (tls.server_name.is_some()
            || tls.skip_cert_verify.is_some()
            || !tls.alpn.is_empty()
            || tls.client_fingerprint.is_some()
            || !tls.certificate_pins.is_empty())
    {
        return Err(invalid("TLS security options require enabled TLS"));
    }
    Ok((tls.enabled || explicit).then_some(tls))
}
