//! Explicit wrapper markers must never degrade to an ordinary inner protocol.
use deve_sub_domain::{Node, ProtocolConfig, ProtocolKind, ShadowTlsConfig, ShadowTlsVersion};
use serde_json::Value;

use super::super::{default_tls_enabled, get_scalar_text, get_str};
use super::extract_tls;
use crate::{ParseError, forward_proxy::invalid};

pub(super) fn project(entry: &Value, inner: &Node) -> Result<Option<Node>, ParseError> {
    let top = entry.get("shadow-tls-opts");
    let plugin = get_str(entry, "plugin").as_deref() == Some("shadow-tls");
    let obfs = entry
        .get("obfs-opts")
        .filter(|o| get_str(o, "mode").as_deref() == Some("shadow-tls"));
    let markers = usize::from(top.is_some()) + usize::from(plugin) + usize::from(obfs.is_some());
    if markers == 0 {
        return Ok(None);
    }
    if entry.get("restls-opts").is_some() || entry.get("jls-opts").is_some() {
        return Err(invalid(
            "ShadowTLS conflicts with another camouflage wrapper",
        ));
    }
    if markers != 1 {
        return Err(invalid("conflicting ShadowTLS wrapper markers"));
    }
    let (opts, allowed, sni_key): (&Value, &[&str], &str) = if let Some(opts) = top {
        if !matches!(
            inner.protocol,
            ProtocolKind::Vless | ProtocolKind::Trojan | ProtocolKind::VMess | ProtocolKind::AnyTls
        ) || entry.get("plugin").is_some()
            || entry.get("plugin-opts").is_some()
            || entry.get("obfs-opts").is_some()
        {
            return Err(invalid(
                "ShadowTLS wrapper conflicts with the inner protocol or plugin slot",
            ));
        }
        (opts, &["version", "password", "sni"], "sni")
    } else if plugin {
        if inner.protocol != ProtocolKind::Shadowsocks || entry.get("obfs-opts").is_some() {
            return Err(invalid(
                "ShadowTLS plugin requires a single Shadowsocks plugin slot",
            ));
        }
        (
            entry
                .get("plugin-opts")
                .ok_or_else(|| invalid("missing ShadowTLS plugin options"))?,
            &["version", "password", "host"],
            "host",
        )
    } else {
        if inner.protocol != ProtocolKind::Snell
            || entry.get("plugin").is_some()
            || entry.get("plugin-opts").is_some()
        {
            return Err(invalid(
                "ShadowTLS obfuscation requires a single Snell obfuscation slot",
            ));
        }
        (
            obfs.ok_or_else(|| invalid("missing ShadowTLS obfuscation"))?,
            &["mode", "version", "password", "host", "alpn"],
            "host",
        )
    };
    let map = opts
        .as_object()
        .ok_or_else(|| invalid("ShadowTLS options must be a map"))?;
    if map.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(invalid("unsupported ShadowTLS option"));
    }
    let version = map
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .and_then(ShadowTlsVersion::from_u32)
        .ok_or_else(|| invalid("ShadowTLS version must be an integer from 1 through 3"))?;
    let password = match map.get("password") {
        None => None,
        Some(Value::Number(_)) if inner.protocol == ProtocolKind::Snell => {
            return Err(invalid("Snell ShadowTLS password must be text"));
        }
        Some(Value::String(_) | Value::Number(_)) => get_scalar_text(opts, "password"),
        _ => {
            return Err(invalid(
                "ShadowTLS password must be text or a numeric scalar",
            ));
        }
    };
    let text = |key| -> Result<Option<String>, ParseError> {
        match opts.get(key) {
            None => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            _ => Err(invalid("ShadowTLS camouflage host must be text")),
        }
    };
    let sni = text(sni_key)?;
    // WHY: permissive getters must not erase explicit security policy before
    // the compatibility layer can inspect it.
    for key in ["sni", "servername", "client-fingerprint"] {
        if entry.get(key).is_some_and(|value| !value.is_string()) {
            return Err(invalid("ShadowTLS TLS text field has an invalid type"));
        }
    }
    for key in ["tls", "skip-cert-verify"] {
        if entry.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(invalid("ShadowTLS TLS boolean field has an invalid type"));
        }
    }
    if entry.get("alpn").is_some_and(|value| {
        value
            .as_array()
            .is_none_or(|a| !a.iter().all(Value::is_string))
    }) {
        return Err(invalid("ShadowTLS TLS ALPN must be a text list"));
    }
    let mut tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);
    tls.enabled = true;
    if sni.is_some() && tls.server_name.is_some() && sni != tls.server_name {
        return Err(invalid("conflicting ShadowTLS camouflage host fields"));
    }
    if tls.server_name.is_none() {
        tls.server_name = sni;
    }
    if let Some(value) = opts.get("alpn") {
        let alpn = value
            .as_array()
            .filter(|a| a.iter().all(Value::is_string))
            .ok_or_else(|| invalid("ShadowTLS ALPN must be a text list"))?;
        if !alpn.is_empty() {
            return Err(invalid("Snell ShadowTLS ALPN mapping is not implemented"));
        }
    }
    let mut inner_config = inner.config.clone();
    if let ProtocolConfig::Snell(snell) = &mut inner_config
        && let Some(alias) = &mut snell.obfs
    {
        // WHY: the native host fallback belongs to this same wrapper. Store
        // its resolved value so a canonical round trip has a single policy.
        alias.host = tls.server_name.clone();
    }
    let config = ShadowTlsConfig {
        version,
        password,
        inner_protocol: inner.protocol.clone(),
        inner_config: Box::new(inner_config),
    };
    if !config.has_valid_password() {
        return Err(invalid(
            "ShadowTLS V2/V3 requires a nonempty wrapper password",
        ));
    }
    let mut node = inner.clone();
    node.protocol = ProtocolKind::ShadowTls;
    node.config = ProtocolConfig::ShadowTls(config);
    node.tls = Some(tls);
    Ok(Some(node))
}
