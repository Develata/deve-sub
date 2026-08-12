//! sing-box JSON parser.
//!
//! Parses the `outbounds` array of a sing-box JSON config into canonical
//! [`Node`] values. Supported outbound types: `vless`, `vmess`, `trojan`,
//! `shadowsocks`, `hysteria2`, `tuic`. Unknown types are preserved as
//! `UnsupportedNode` (constraint #7).
//!
//! See `docs/plan/05-protocol-engine.md` §"Input formats vs protocols".

use serde_json::Value;

use deve_sub_domain::{
    AnyTlsConfig, Authentication, CongestionConfig, CongestionController, Endpoint,
    Hysteria2Config, Node, Obfuscation, ProtocolConfig, ProtocolKind, RealityConfig, SnellConfig,
    SnellObfs, SnellObfsMode, SnellV6Mode, SnellVersion, TlsConfig, Transport, TransportKind,
    TrojanConfig, TuicV5Config, UdpRelayMode, VMessConfig, VlessRealityConfig, WireGuardConfig,
    WireGuardPeer,
};

use crate::error::ParseError;

use super::{
    default_tls_enabled, get_bool, get_port, get_str, get_str_array, node_shell_container,
    parse_host_str, unsupported_entry,
};

/// Parse a sing-box JSON config into a list of [`Node`] values.
///
/// # Errors
/// Returns [`ParseError::InvalidJson`] if the JSON is malformed.
/// Returns [`ParseError::MissingContainerKey`] if the `outbounds` key is absent.
pub fn parse_singbox_json(text: &str) -> Result<Vec<Node>, ParseError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| ParseError::InvalidJson(e.to_string()))?;

    let outbounds = value
        .get("outbounds")
        .ok_or(ParseError::MissingContainerKey("outbounds"))?
        .as_array()
        .ok_or(ParseError::MissingContainerKey("outbounds (not a list)"))?;

    Ok(outbounds.iter().map(parse_outbound).collect())
}

/// Dispatch a single outbound entry to the appropriate protocol mapper.
fn parse_outbound(entry: &Value) -> Node {
    let outbound_type = match get_str(entry, "type") {
        Some(t) => t,
        None => {
            return unsupported_entry(
                entry,
                "singbox-json",
                ProtocolKind::Unknown(String::new()),
                "missing 'type' field".to_owned(),
            );
        }
    };

    let result = match outbound_type.as_str() {
        "vless" => parse_vless(entry),
        "vmess" => parse_vmess(entry),
        "trojan" => parse_trojan(entry),
        "shadowsocks" => parse_shadowsocks(entry),
        "hysteria2" => parse_hysteria2(entry),
        "tuic" => parse_tuic(entry),
        "wireguard" => parse_wireguard(entry),
        "anytls" => parse_anytls(entry),
        "snell" => parse_snell(entry),
        // sing-box internal types (direct, block, dns) are not proxy nodes.
        "direct" | "block" | "dns" | "selector" | "urltest" => {
            return unsupported_entry(
                entry,
                "singbox-json",
                ProtocolKind::Unknown(outbound_type),
                "sing-box internal outbound (not a proxy)".to_owned(),
            );
        }
        other => Err(ParseError::UnsupportedProxyType(other.to_owned())),
    };

    match result {
        Ok(node) => node,
        Err(e) => unsupported_entry(
            entry,
            "singbox-json",
            ProtocolKind::Unknown(outbound_type),
            e.to_string(),
        ),
    }
}

/// Extract common fields: tag (display name), server, server_port.
fn build_base(entry: &Value) -> Result<(String, Endpoint), ParseError> {
    let name = get_str(entry, "tag").unwrap_or_default();
    let server = get_str(entry, "server").ok_or(ParseError::MissingField("server"))?;
    let port = get_port(entry, "server_port")?.ok_or(ParseError::MissingField("server_port"))?;
    let host = parse_host_str(&server)?;
    Ok((name, Endpoint { host, port }))
}

/// Extract TLS config from a sing-box outbound.
fn extract_tls(entry: &Value) -> Option<TlsConfig> {
    let tls = entry.get("tls")?;
    let enabled = get_bool(tls, "enabled").unwrap_or(false);
    if !enabled {
        return None;
    }

    let server_name = get_str(tls, "server_name");
    let insecure = get_bool(tls, "insecure");
    let alpn = get_str_array(tls, "alpn");

    let fingerprint = tls.get("utls").and_then(|u| get_str(u, "fingerprint"));

    let reality = tls.get("reality").and_then(|r| {
        get_bool(r, "enabled")?.then_some(RealityConfig {
            public_key: get_str(r, "public_key").unwrap_or_default(),
            short_id: get_str(r, "short_id").unwrap_or_default(),
            spider_x: None,
        })
    });

    Some(TlsConfig {
        enabled: true,
        server_name,
        skip_cert_verify: insecure,
        alpn,
        client_fingerprint: fingerprint,
        certificate_pins: vec![],
        reality,
    })
}

/// Extract transport from a sing-box outbound.
fn extract_transport(entry: &Value) -> Option<Transport> {
    let transport = entry.get("transport")?;
    let transport_type = get_str(transport, "type")?;

    let kind = match transport_type.as_str() {
        "ws" => TransportKind::Ws,
        "grpc" => TransportKind::Grpc,
        "http" => TransportKind::H2,
        "httpupgrade" => TransportKind::HttpUpgrade,
        _ => return None,
    };

    let (path, host) = match kind {
        TransportKind::Ws => {
            let path = get_str(transport, "path");
            let host = transport.get("headers").and_then(|h| get_str(h, "Host"));
            (path, host)
        }
        TransportKind::Grpc => (get_str(transport, "service_name"), None),
        TransportKind::H2 => {
            let path = get_str(transport, "path");
            let host = transport
                .get("host")
                .and_then(|h| h.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(String::from);
            (path, host)
        }
        TransportKind::HttpUpgrade => (get_str(transport, "path"), None),
        _ => (None, None),
    };

    Some(Transport { kind, path, host })
}

// --- Per-protocol mappers ---

/// Parse a Go-style duration string (e.g. `"10s"`, `"500ms"`, `"1m30s"`,
/// `"1.5s"`) into a `time::Duration`. Returns `None` on malformed input.
fn parse_go_duration(s: &str) -> Option<time::Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let mut total_ms: i64 = 0;
    let mut parsed_any = false;
    let mut chars = s.chars().peekable();
    let mut num = String::new();

    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
            chars.next();
        } else {
            let value: f64 = num.parse().ok()?;
            num.clear();

            let mut unit = String::new();
            while let Some(&u) = chars.peek() {
                if u.is_ascii_alphabetic() {
                    unit.push(u);
                    chars.next();
                } else {
                    break;
                }
            }

            if unit.is_empty() {
                return None;
            }

            let ms: i64 = match unit.as_str() {
                "h" => (value * 3_600_000.0) as i64,
                "m" => (value * 60_000.0) as i64,
                "s" => (value * 1_000.0) as i64,
                "ms" => value as i64,
                _ => return None,
            };
            total_ms = total_ms.checked_add(ms)?;
            parsed_any = true;
        }
    }

    if !num.is_empty() || !parsed_any {
        return None;
    }

    Some(time::Duration::milliseconds(total_ms))
}

fn parse_vless(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let uuid = get_str(entry, "uuid").ok_or(ParseError::MissingField("uuid"))?;

    let config = ProtocolConfig::VlessReality(VlessRealityConfig {
        encryption: get_str(entry, "encryption"),
        flow: get_str(entry, "flow"),
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Vless;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Uuid { uuid };
    node.tls = extract_tls(entry);
    node.transport = extract_transport(entry);
    Ok(node)
}

fn parse_trojan(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let config = ProtocolConfig::Trojan(TrojanConfig {
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Trojan;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.tls = extract_tls(entry).or_else(|| Some(default_tls_enabled()));
    node.transport = extract_transport(entry);
    Ok(node)
}

fn parse_shadowsocks(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;
    let method = get_str(entry, "method").ok_or(ParseError::MissingField("method"))?;

    let config = ProtocolConfig::Shadowsocks(deve_sub_domain::ShadowsocksConfig {
        method,
        plugin: get_str(entry, "plugin"),
        plugin_opts: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Shadowsocks;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    Ok(node)
}

fn parse_vmess(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let uuid = get_str(entry, "uuid").ok_or(ParseError::MissingField("uuid"))?;

    let config = ProtocolConfig::VMess(VMessConfig {
        alter_id: entry
            .get("alter_id")
            .and_then(|v| v.as_u64())
            .map(|n| u32::try_from(n).unwrap_or(0)),
        security: get_str(entry, "security"),
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::VMess;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Uuid { uuid };
    node.tls = extract_tls(entry);
    node.transport = extract_transport(entry);
    Ok(node)
}

fn parse_hysteria2(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let obfuscation = get_str(entry, "obfs").map(|kind| Obfuscation {
        kind,
        password: get_str(entry, "obfs_password"),
    });

    let config = ProtocolConfig::Hysteria2(Hysteria2Config {
        ports: get_str(entry, "port_hopping"),
        hop_interval: None,
        fast_open: get_bool(entry, "fast_open"),
        lazy: get_bool(entry, "lazy"),
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Hysteria2;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.tls = extract_tls(entry).or_else(|| Some(default_tls_enabled()));
    node.obfuscation = obfuscation;
    Ok(node)
}

fn parse_tuic(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let uuid = get_str(entry, "uuid").ok_or(ParseError::MissingField("uuid"))?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let congestion_controller = get_str(entry, "congestion_control").map(|c| match c.as_str() {
        "bbr" => CongestionController::Bbr,
        "cubic" => CongestionController::Cubic,
        "new_reno" => CongestionController::NewReno,
        other => CongestionController::Other(other.to_owned()),
    });

    let congestion = congestion_controller.map(|controller| CongestionConfig {
        controller,
        up_bps: None,
        down_bps: None,
    });

    let udp_relay_mode = get_str(entry, "udp_relay_mode").and_then(|m| match m.as_str() {
        "native" => Some(UdpRelayMode::Native),
        "quic" => Some(UdpRelayMode::Quic),
        _ => None,
    });

    let heartbeat = entry
        .get("heartbeat")
        .and_then(|v| v.as_str())
        .and_then(parse_go_duration);

    let config = ProtocolConfig::TuicV5(TuicV5Config {
        udp_relay_mode,
        zero_rtt_handshake: get_bool(entry, "zero_rtt_handshake"),
        heartbeat,
        disable_sni: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::TuicV5;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::UuidPassword { uuid, password };
    node.tls = extract_tls(entry).or_else(|| Some(default_tls_enabled()));
    node.congestion = congestion;
    Ok(node)
}

fn parse_wireguard(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let private_key =
        get_str(entry, "private_key").ok_or(ParseError::MissingField("private_key"))?;

    let address = get_str_array(entry, "local_address");

    let mtu = entry
        .get("mtu")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let workers = entry
        .get("workers")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let peers = if let Some(peers_arr) = entry.get("peers").and_then(|p| p.as_array()) {
        peers_arr
            .iter()
            .map(|p| {
                let public_key =
                    get_str(p, "public_key").ok_or(ParseError::MissingField("peer public_key"))?;
                let pre_shared_key = get_str(p, "pre_shared_key");
                let allowed_ips = get_str_array(p, "allowed_ips");
                let reserved = p
                    .get("reserved")
                    .and_then(|r| r.as_array())
                    .and_then(|arr| {
                        if arr.len() == 3 {
                            let mut bytes = [0u8; 3];
                            for (i, v) in arr.iter().enumerate() {
                                bytes[i] = v.as_u64()?.try_into().ok()?;
                            }
                            Some(bytes)
                        } else {
                            None
                        }
                    });
                let persistent_keepalive = p
                    .get("persistent_keepalive_interval")
                    .and_then(|v| v.as_u64())
                    .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0)));
                Ok(WireGuardPeer {
                    public_key,
                    pre_shared_key,
                    allowed_ips,
                    reserved,
                    persistent_keepalive,
                })
            })
            .collect::<Result<Vec<_>, ParseError>>()?
    } else {
        return Err(ParseError::MissingField("peers"));
    };

    let config = ProtocolConfig::WireGuard(WireGuardConfig {
        private_key,
        address,
        peers,
        mtu,
        workers,
        dns: vec![],
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::WireGuard;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::None;
    Ok(node)
}

fn parse_anytls(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    // WHY: AnyTLS always requires TLS; sing-box rejects configs without
    // `tls.enabled = true` at dial time, so fall back to a default-enabled
    // TLS config when no TLS block is present (matches Trojan handling).
    let tls = extract_tls(entry).or_else(|| Some(default_tls_enabled()));

    let idle_session_check_interval = entry
        .get("idle_session_check_interval")
        .and_then(|v| v.as_str())
        .and_then(parse_go_duration);
    let idle_session_timeout = entry
        .get("idle_session_timeout")
        .and_then(|v| v.as_str())
        .and_then(parse_go_duration);
    let min_idle_session = entry
        .get("min_idle_session")
        .and_then(|v| v.as_i64())
        .map(|n| u32::try_from(n).unwrap_or(0));
    let client_metadata = get_str(entry, "client_metadata");

    let config = ProtocolConfig::AnyTls(AnyTlsConfig {
        idle_session_check_interval,
        idle_session_timeout,
        min_idle_session,
        client_metadata,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::AnyTls;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.tls = tls;
    Ok(node)
}

fn parse_snell(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let psk = get_str(entry, "psk").ok_or(ParseError::MissingField("psk"))?;

    let version_n = entry
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or(ParseError::MissingField("version"))?;
    let version = SnellVersion::from_u32(u32::try_from(version_n).unwrap_or(0)).ok_or(
        ParseError::InvalidField {
            field: "version",
            value: version_n.to_string(),
        },
    )?;

    let reuse = get_bool(entry, "reuse");
    // WHY: `userkey` is a sing-box-only field with no canonical home; preserve
    // it in `extras` so sing-box round-trip is lossless.
    let userkey = get_str(entry, "userkey");

    let (obfs, v6_mode) = match version {
        SnellVersion::V4 => {
            let obfs_mode = get_str(entry, "obfs_mode").unwrap_or_else(|| "none".to_owned());
            let obfs = match obfs_mode.as_str() {
                "none" => None,
                "http" => Some(SnellObfs {
                    mode: SnellObfsMode::Http,
                    host: get_str(entry, "obfs_host"),
                    password: None,
                    version: None,
                    alpn: vec![],
                }),
                "tls" => Some(SnellObfs {
                    mode: SnellObfsMode::Tls,
                    host: get_str(entry, "obfs_host"),
                    password: None,
                    version: None,
                    alpn: vec![],
                }),
                other => {
                    return Err(ParseError::InvalidField {
                        field: "obfs_mode",
                        value: other.to_owned(),
                    });
                }
            };
            (obfs, None)
        }
        SnellVersion::V6 => {
            let mode = get_str(entry, "mode")
                .and_then(|s| match s.as_str() {
                    "default" => Some(SnellV6Mode::Default),
                    "unshaped" => Some(SnellV6Mode::Unshaped),
                    "unsafe-raw" => Some(SnellV6Mode::UnsafeRaw),
                    _ => None,
                })
                .unwrap_or(SnellV6Mode::Default);
            (None, Some(mode))
        }
        // WHY: sing-box outbound accepts only v4/v6 (option/snell.go line 71).
        // v1/v2/v3/v5 surface as Unsupported here; the compatibility layer
        // rejects them with `UnsupportedProtocolVersion` when emitting.
        other => {
            return Err(ParseError::InvalidField {
                field: "version",
                value: format!(
                    "sing-box snell outbound does not support v{}",
                    other.as_u32()
                ),
            });
        }
    };

    // WHY: sing-box Snell v4 with `obfs_mode = tls` carries the camouflage SNI
    // in a nested `tls` block (server_name) and obfs_host as fallback.
    let tls = if matches!(obfs.as_ref().map(|o| o.mode), Some(SnellObfsMode::Tls)) {
        let mut t = extract_tls(entry).unwrap_or_else(default_tls_enabled);
        t.enabled = true;
        if t.server_name.is_none() {
            t.server_name = obfs.as_ref().and_then(|o| o.host.clone());
        }
        Some(t)
    } else {
        None
    };

    let config = ProtocolConfig::Snell(SnellConfig {
        version,
        reuse,
        obfs,
        v6_mode,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Snell;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password: psk };
    node.tls = tls;
    if let Some(key) = userkey {
        node.extras
            .insert("snell_userkey".to_owned(), serde_json::Value::String(key));
    }
    Ok(node)
}
