//! URI parsing dispatcher and shared helpers.
//!
//! The top-level [`parse_uri`] function dispatches on the URI scheme to the
//! appropriate protocol parser. Shared helpers for host parsing, query
//! parameter extraction, boolean conversion, TLS building, and bandwidth
//! parsing live here to avoid duplication across protocol parsers.

use std::collections::HashMap;

use deve_sub_domain::{CertificatePin, Host, Node, TlsConfig};
use deve_sub_kernel::NodeId;

use crate::error::ParseError;

/// Parse a share URI into a canonical [`Node`].
///
/// Dispatches on the URI scheme (`vless://`, `hysteria2://`, `hy2://`,
/// `tuic://`, `naive+https://`, `ss://`, `vmess://`, `trojan://`,
/// `wireguard://`, `anytls://`, `snell://`).
///
/// The returned `Node` has protocol-specific fields populated from the URI.
/// Metadata fields (`source`, `tags`, `region`) are set to defaults; the
/// caller should override them as needed. The `source.raw_uri` field is set
/// to the original URI string.
///
/// # Errors
/// Returns [`ParseError`] if the URI is malformed, the scheme is unknown,
/// or a required field is missing.
pub fn parse_uri(uri: &str) -> Result<Node, ParseError> {
    // WHY: `ss://` legacy and `vmess://` use non-standard URI formats that
    // may not parse as valid URLs (Base64 body without an `@` host separator).
    // Handle them before `url::Url::parse` to avoid spurious `InvalidUri`.
    if uri.starts_with("ss://") {
        return crate::shadowsocks::parse(uri);
    }
    if uri.starts_with("vmess://") {
        return crate::vmess::parse(uri);
    }

    let parsed = url::Url::parse(uri).map_err(|e| ParseError::InvalidUri(e.to_string()))?;

    match parsed.scheme() {
        "vless" => crate::vless_reality::parse(&parsed, uri),
        "trojan" => crate::trojan::parse(&parsed, uri),
        "hysteria2" | "hy2" => crate::hysteria2::parse(&parsed, uri),
        "tuic" => crate::tuic_v5::parse(&parsed, uri),
        "naive+https" | "naive+http" => crate::naive::parse(&parsed, uri),
        "wireguard" => crate::wireguard::parse(&parsed, uri),
        "anytls" => crate::anytls::parse(&parsed, uri),
        "snell" => crate::snell::parse(&parsed, uri),
        "shadow-tls" => crate::shadowtls::parse(&parsed, uri),
        other => Err(ParseError::UnknownScheme(other.to_owned())),
    }
}

/// Convert a host string from the URL parser to a domain [`Host`].
pub(crate) fn parse_host(host_str: &str) -> Result<Host, ParseError> {
    if host_str.is_empty() {
        return Err(ParseError::InvalidHost("empty host".to_owned()));
    }
    // WHY: strip IPv6 bracket notation `[2001:db8::1]` → `2001:db8::1` so
    // that `parse_host` accepts both bracketed and bare IPv6 literals.
    // Container parsers call this with raw `server` fields that may include
    // brackets (e.g. sing-box `server: "[2001:db8::1]"`).
    let host_str = host_str
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host_str);
    if let Ok(ipv4) = host_str.parse::<std::net::Ipv4Addr>() {
        return Ok(Host::Ipv4(ipv4));
    }
    if let Ok(ipv6) = host_str.parse::<std::net::Ipv6Addr>() {
        return Ok(Host::Ipv6(ipv6));
    }
    // WHY: reject domain names containing control characters, whitespace, or
    // URI structural characters. Container format parsers (Mihomo YAML,
    // sing-box JSON, Xray/V2Ray JSON) call this function directly with
    // untrusted input, unlike URI parsers which use `url::Url::parse` first.
    // A malicious `server` field like "real.com\nvless://attacker.com:443"
    // would inject additional URI lines when re-emitted via `emit_uri_list`.
    if host_str.chars().any(|c| {
        c.is_control() || c.is_whitespace() || matches!(c, '/' | ':' | '@' | '#' | '?' | '[' | ']')
    }) {
        return Err(ParseError::InvalidHost(format!(
            "domain name contains invalid characters: {host_str}"
        )));
    }
    Ok(Host::Domain(deve_sub_domain::DomainName::new(
        host_str.to_owned(),
    )))
}

/// Parse a `host:port` string into a [`Host`] and port number.
///
/// Handles IPv6 bracket notation: `[2001:db8::1]:443`.
pub(crate) fn parse_host_port(s: &str) -> Result<(Host, u16), ParseError> {
    if let Some(rest) = s.strip_prefix('[') {
        let (ipv6_str, after) = rest
            .split_once(']')
            .ok_or_else(|| ParseError::InvalidHost("unclosed IPv6 bracket".to_owned()))?;
        let port_str = after
            .strip_prefix(':')
            .ok_or(ParseError::MissingField("port after IPv6 host"))?;
        let host = parse_host(ipv6_str)?;
        let port: u16 = port_str
            .parse()
            .map_err(|_| ParseError::InvalidPort(port_str.to_owned()))?;
        return Ok((host, port));
    }

    let (host_str, port_str) = s
        .rsplit_once(':')
        .ok_or(ParseError::MissingField("host:port"))?;
    let host = parse_host(host_str)?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| ParseError::InvalidPort(port_str.to_owned()))?;
    Ok((host, port))
}

/// Parse a query parameter value as a boolean.
///
/// Accepts `true`/`false` (case-sensitive) and `1`/`0`.
pub(crate) fn parse_bool(value: &str) -> Result<bool, ParseError> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(ParseError::InvalidField {
            field: "boolean",
            value: value.to_owned(),
        }),
    }
}

/// Create a `Node` with default metadata fields.
///
/// Protocol-specific fields are filled by the caller. The `source.raw_uri`
/// is set to the original URI string for provenance. Container format
/// parsers pass `None` since there is no URI.
pub(crate) fn node_shell(raw_uri: Option<&str>) -> Node {
    Node {
        id: NodeId::new(),
        display_name: String::new(),
        protocol: deve_sub_domain::ProtocolKind::Unknown(String::new()),
        config: deve_sub_domain::ProtocolConfig::Unsupported(deve_sub_domain::UnsupportedNode {
            raw: serde_json::Value::Null,
            raw_format: None,
            reason: String::new(),
        }),
        endpoint: deve_sub_domain::Endpoint {
            host: Host::Domain(deve_sub_domain::DomainName::new(String::new())),
            port: 0,
        },
        authentication: deve_sub_domain::Authentication::None,
        transport: None,
        tls: None,
        udp: deve_sub_domain::UdpCapability::default(),
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: deve_sub_domain::NodeSource {
            source_label: String::new(),
            raw_uri: raw_uri.map(|s| s.to_owned()),
            imported_at: deve_sub_kernel::Timestamp::now(),
        },
        tags: vec![],
        region: deve_sub_domain::RegionAssignment {
            method: deve_sub_domain::RegionMethod::Auto,
            value: None,
        },
        extras: std::collections::BTreeMap::new(),
    }
}

/// Look up the TLS insecure flag from multiple possible query parameter names.
///
/// Different protocols use different parameter names for the same concept:
/// VLESS uses `allowInsecure`, Hysteria2 uses `insecure`, TUIC/Trojan/NaiveProxy
/// use `skip-cert-verify`. This helper checks the provided names in order and
/// returns the first match as a three-state `Option<bool>`.
pub(crate) fn query_insecure(
    query: &HashMap<String, String>,
    params: &[&str],
) -> Result<Option<bool>, ParseError> {
    for &param in params {
        if let Some(value) = query.get(param) {
            return match value.as_str() {
                "0" | "false" => Ok(Some(false)),
                "1" | "true" => Ok(Some(true)),
                _ => Err(ParseError::InvalidField {
                    field: "insecure",
                    value: value.clone(),
                }),
            };
        }
    }
    Ok(None)
}

/// Parse a comma-separated ALPN query parameter into a vector.
///
/// An empty value (`alpn=`) produces an empty vec, not `vec![""]`.
pub(crate) fn parse_alpn(value: &str) -> Vec<String> {
    if value.is_empty() {
        vec![]
    } else {
        value.split(',').map(String::from).collect()
    }
}

/// Parse a bandwidth string (e.g. `100 Mbps`) into bits per second.
///
/// Supported units: `bps`, `Kbps`/`kbps`, `Mbps`/`mbps`, `Gbps`/`gbps`.
/// The numeric part may be an integer or a decimal. Whitespace between the
/// number and unit is optional.
pub(crate) fn parse_bandwidth(value: &str) -> Result<u64, ParseError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ParseError::InvalidBandwidth("empty".to_owned()));
    }

    // Find the boundary between the numeric prefix and the unit suffix.
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());

    let (num_str, unit_str) = trimmed.split_at(split);
    let num: f64 = num_str
        .parse()
        .map_err(|_| ParseError::InvalidBandwidth(value.to_owned()))?;
    let unit = unit_str.trim();

    let multiplier: u64 = match unit.to_ascii_lowercase().as_str() {
        "" | "bps" => 1,
        "k" | "kbps" => 1_000,
        "m" | "mbps" => 1_000_000,
        "g" | "gbps" => 1_000_000_000,
        _ => return Err(ParseError::InvalidBandwidth(value.to_owned())),
    };

    Ok((num * multiplier as f64) as u64)
}

/// Parse an integer number of seconds into a [`time::Duration`].
pub(crate) fn parse_duration_secs(value: &str) -> Result<time::Duration, ParseError> {
    let secs: i64 = value.parse().map_err(|_| ParseError::InvalidField {
        field: "duration (seconds)",
        value: value.to_owned(),
    })?;
    if secs < 0 {
        return Err(ParseError::InvalidField {
            field: "duration (seconds)",
            value: value.to_owned(),
        });
    }
    Ok(time::Duration::seconds(secs))
}

/// Parse an integer number of milliseconds into a [`time::Duration`].
pub(crate) fn parse_duration_millis(value: &str) -> Result<time::Duration, ParseError> {
    let millis: i64 = value.parse().map_err(|_| ParseError::InvalidField {
        field: "duration (milliseconds)",
        value: value.to_owned(),
    })?;
    if millis < 0 {
        return Err(ParseError::InvalidField {
            field: "duration (milliseconds)",
            value: value.to_owned(),
        });
    }
    Ok(time::Duration::milliseconds(millis))
}

/// Build a [`TlsConfig`] from query parameters common to TLS-based protocols.
///
/// `insecure_params` is the list of query parameter names that map to
/// `skip_cert_verify` (e.g. `["insecure"]` for Hysteria2,
/// `["skip-cert-verify", "insecure"]` for TUIC). `pin_param` is the query
/// parameter name for certificate pinning (e.g. `pinSHA256`), if the protocol
/// supports it.
///
/// Returns `None` if no TLS-related parameters are present.
pub(crate) fn build_common_tls(
    query: &HashMap<String, String>,
    insecure_params: &[&str],
    pin_param: Option<&str>,
) -> Result<Option<TlsConfig>, ParseError> {
    let has_tls = query.contains_key("sni")
        || query.contains_key("alpn")
        || insecure_params.iter().any(|p| query.contains_key(*p))
        || pin_param.is_some_and(|p| query.contains_key(p));

    if !has_tls {
        return Ok(None);
    }

    let skip_cert_verify = query_insecure(query, insecure_params)?;

    let alpn = query.get("alpn").map(|v| parse_alpn(v)).unwrap_or_default();

    let certificate_pins = pin_param
        .and_then(|p| query.get(p))
        .map(|v| {
            if v.is_empty() {
                vec![]
            } else {
                v.split(',')
                    .map(|pin| CertificatePin::new(format!("pinSHA256:{pin}")))
                    .collect()
            }
        })
        .unwrap_or_default();

    Ok(Some(TlsConfig {
        enabled: true,
        server_name: query.get("sni").cloned(),
        skip_cert_verify,
        alpn,
        client_fingerprint: None,
        certificate_pins,
        reality: None,
    }))
}

/// Decode a percent-encoded URI fragment into a display name.
pub(crate) fn decode_fragment(url: &url::Url) -> String {
    url.fragment()
        .map(|f| {
            percent_encoding::percent_decode_str(f)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

/// Percent-decode a URI userinfo credential.
///
/// WHY: `url::Url::username()` and `url::Url::password()` return the
/// percent-ENCODED form (e.g. `p%40ss` for `p@ss`). Storing that form in the
/// domain model would corrupt container emitters (JSON/YAML), which need the
/// raw credential. This helper decodes userinfo to its true value so the
/// canonical node holds the raw password, and URI emitters re-encode on
/// output (RFC 3986 §3.2.1).
pub(crate) fn decode_userinfo(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// Decode an optional raw fragment string (already split from the URI) into a
/// display name.
pub(crate) fn decode_fragment_parts(fragment: Option<&str>) -> String {
    fragment
        .map(|f| {
            percent_encoding::percent_decode_str(f)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

/// Collect query parameters into a `HashMap`.
pub(crate) fn collect_query(url: &url::Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Decode a Base64 string, trying both padded and unpadded, standard and
/// URL-safe variants. PARSE-010: both padding formats must work.
pub(crate) fn decode_base64_flexible(input: &str) -> Result<Vec<u8>, ParseError> {
    use base64::Engine;
    use base64::engine::general_purpose;
    // WHY: try each engine individually because `Engine` is not dyn-compatible
    // (its methods have generic type parameters).
    if let Ok(result) = general_purpose::STANDARD.decode(input) {
        return Ok(result);
    }
    if let Ok(result) = general_purpose::STANDARD_NO_PAD.decode(input) {
        return Ok(result);
    }
    if let Ok(result) = general_purpose::URL_SAFE.decode(input) {
        return Ok(result);
    }
    if let Ok(result) = general_purpose::URL_SAFE_NO_PAD.decode(input) {
        return Ok(result);
    }
    Err(ParseError::InvalidBase64(input.to_owned()))
}
