//! Mihomo Snell mapping. Kept with its format-specific TLS semantics.
use super::super::{default_tls_enabled, get_bool, get_str, get_str_array, node_shell_container};
use super::{build_base, extract_tls, extract_udp};
use crate::error::ParseError;
use deve_sub_domain::{
    Authentication, Node, ProtocolConfig, ProtocolKind, SnellConfig, SnellObfs, SnellObfsMode,
    SnellVersion,
};
use serde_json::Value;

pub(super) fn parse_snell(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let psk = get_str(entry, "psk").ok_or(ParseError::MissingField("psk"))?;

    // WHY: Snell default version is 1 in mihomo when `version` is absent; all
    // numeric versions 1–5 are accepted. V6 is sing-box-only and rejected
    // here with `InvalidField` so the entry surfaces as `Unsupported` rather
    // than silently downgrading.
    let version = match entry.get("version") {
        Some(v) => {
            let n = v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or(ParseError::InvalidField {
                    field: "version",
                    value: v.to_string(),
                })?;
            SnellVersion::from_u32(u32::try_from(n).unwrap_or(0)).ok_or(
                ParseError::InvalidField {
                    field: "version",
                    value: n.to_string(),
                },
            )?
        }
        None => SnellVersion::V1,
    };

    let reuse = get_bool(entry, "reuse");

    let obfs = entry.get("obfs-opts").map(parse_snell_obfs).transpose()?;

    // WHY: Snell has no TLS by default; TLS only when `obfs-opts.mode = tls`.
    // `extract_tls` reads top-level `tls`/`servername`/`skip-cert-verify`/
    // `alpn`/`client-fingerprint`, which mihomo Snell uses for the TLS-shaped
    // obfs modes. When obfs is TLS we map the obfs host to `tls.server_name`
    // only if `servername` is absent, mirroring mihomo's fallback semantics.
    let tls = if matches!(obfs.as_ref().map(|o| o.mode), Some(SnellObfsMode::Tls)) {
        let mut t = extract_tls(entry).unwrap_or_else(default_tls_enabled);
        t.enabled = true;
        if t.server_name.is_none() {
            t.server_name = obfs.as_ref().and_then(|o| o.host.clone());
        }
        if t.alpn.is_empty()
            && let Some(o) = obfs.as_ref()
        {
            t.alpn = o.alpn.clone();
        }
        Some(t)
    } else {
        None
    };

    let config = ProtocolConfig::Snell(SnellConfig {
        version,
        reuse,
        obfs,
        // mihomo does not expose v6 mode; sing-box only.
        v6_mode: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Snell;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password: psk };
    node.udp = extract_udp(entry);
    node.tls = tls;
    Ok(node)
}

fn parse_snell_obfs(o: &Value) -> Result<SnellObfs, ParseError> {
    let mode_str = get_str(o, "mode").ok_or(ParseError::MissingField("obfs-opts.mode"))?;
    let mode = match mode_str.as_str() {
        "tls" => SnellObfsMode::Tls,
        "http" => SnellObfsMode::Http,
        "shadow-tls" => SnellObfsMode::ShadowTls,
        "restls" => SnellObfsMode::Restls,
        "jls" => SnellObfsMode::Jls,
        other => {
            return Err(ParseError::InvalidField {
                field: "obfs-opts.mode",
                value: other.to_owned(),
            });
        }
    };
    let host = get_str(o, "host");
    let password = get_str(o, "password");
    let version = o
        .get("version")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));
    let alpn = get_str_array(o, "alpn");
    Ok(SnellObfs {
        mode,
        host,
        password,
        version,
        alpn,
    })
}
