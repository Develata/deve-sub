//! Xray and V2Ray JSON parser.
//!
//! Both Xray and V2Ray use the same JSON config structure with an
//! `outbounds` array. Each outbound has a `protocol`, `tag`, `settings`
//! (containing server/port/credentials), and `streamSettings` (transport
//! and TLS). Xray adds Reality support; V2Ray does not, but the parser
//! handles both identically.
//!
//! Supported protocols: `vless`, `vmess`, `trojan`, `shadowsocks`. Unknown
//! protocols are preserved as `UnsupportedNode` (constraint #7).
//!
//! See `docs/plan/05-protocol-engine.md` §"Input formats vs protocols".

use serde_json::Value;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, RealityConfig, ShadowsocksConfig,
    TlsConfig, Transport, TransportKind, TrojanConfig, VMessConfig, VlessRealityConfig,
    WireGuardConfig, WireGuardPeer,
};

use crate::error::ParseError;

use super::{
    default_tls_enabled, get_bool, get_str, get_str_array, node_shell_container, parse_host_str,
    unsupported_entry,
};

/// Parse an Xray JSON config into a list of [`Node`] values.
///
/// # Errors
/// Returns [`ParseError::InvalidJson`] if the JSON is malformed.
/// Returns [`ParseError::MissingContainerKey`] if the `outbounds` key is absent.
pub fn parse_xray_json(text: &str) -> Result<Vec<Node>, ParseError> {
    parse_xray_v2ray(text, "xray-json")
}

/// Parse a V2Ray JSON config into a list of [`Node`] values.
///
/// # Errors
/// Returns [`ParseError::InvalidJson`] if the JSON is malformed.
/// Returns [`ParseError::MissingContainerKey`] if the `outbounds` key is absent.
pub fn parse_v2ray_json(text: &str) -> Result<Vec<Node>, ParseError> {
    parse_xray_v2ray(text, "v2ray-json")
}

/// Shared parser for Xray and V2Ray JSON configs.
fn parse_xray_v2ray(text: &str, raw_format: &str) -> Result<Vec<Node>, ParseError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| ParseError::InvalidJson(e.to_string()))?;

    let outbounds = value
        .get("outbounds")
        .ok_or(ParseError::MissingContainerKey("outbounds"))?
        .as_array()
        .ok_or(ParseError::MissingContainerKey("outbounds (not a list)"))?;

    Ok(outbounds
        .iter()
        .map(|e| parse_outbound(e, raw_format))
        .collect())
}

/// Dispatch a single outbound entry to the appropriate protocol mapper.
fn parse_outbound(entry: &Value, raw_format: &str) -> Node {
    let protocol = match get_str(entry, "protocol") {
        Some(p) => p,
        None => {
            return unsupported_entry(
                entry,
                raw_format,
                ProtocolKind::Unknown(String::new()),
                "missing 'protocol' field".to_owned(),
            );
        }
    };

    let result = match protocol.as_str() {
        "vless" => parse_vless(entry, raw_format),
        "vmess" => parse_vmess(entry),
        "trojan" => parse_trojan(entry),
        "shadowsocks" => parse_shadowsocks(entry),
        "wireguard" => parse_wireguard(entry),
        other => Err(ParseError::UnsupportedProxyType(other.to_owned())),
    };

    match result {
        Ok(node) => node,
        Err(e) => unsupported_entry(
            entry,
            raw_format,
            ProtocolKind::Unknown(protocol),
            e.to_string(),
        ),
    }
}

/// Extract server and port from `settings.vnext[0]` (VLESS/VMess) or
/// `settings.servers[0]` (Trojan/Shadowsocks).
fn extract_server_port(entry: &Value) -> Result<(String, u16), ParseError> {
    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;

    // VLESS/VMess use `vnext[0].{address,port}`.
    if let Some(vnext) = settings.get("vnext").and_then(|v| v.as_array()) {
        let first = vnext.first().ok_or(ParseError::MissingField("vnext[0]"))?;
        let address = get_str(first, "address").ok_or(ParseError::MissingField("address"))?;
        let port = first
            .get("port")
            .and_then(|p| p.as_u64())
            .ok_or(ParseError::MissingField("port"))?;
        let port = u16::try_from(port).map_err(|_| ParseError::InvalidPort(port.to_string()))?;
        return Ok((address, port));
    }

    // Trojan/Shadowsocks use `servers[0].{address,port}`.
    if let Some(servers) = settings.get("servers").and_then(|v| v.as_array()) {
        let first = servers
            .first()
            .ok_or(ParseError::MissingField("servers[0]"))?;
        let address = get_str(first, "address").ok_or(ParseError::MissingField("address"))?;
        let port = first
            .get("port")
            .and_then(|p| p.as_u64())
            .ok_or(ParseError::MissingField("port"))?;
        let port = u16::try_from(port).map_err(|_| ParseError::InvalidPort(port.to_string()))?;
        return Ok((address, port));
    }

    Err(ParseError::MissingField("vnext or servers"))
}

/// Extract display name from `tag` field.
fn extract_tag(entry: &Value) -> String {
    get_str(entry, "tag").unwrap_or_default()
}

/// Extract TLS config from `streamSettings`.
fn extract_tls(stream: &Value) -> Option<TlsConfig> {
    let security = get_str(stream, "security")?;
    if security == "none" {
        return None;
    }

    let tls_settings = match security.as_str() {
        "tls" => stream.get("tlsSettings")?,
        "reality" => stream.get("realitySettings")?,
        _ => return None,
    };

    let server_name =
        get_str(tls_settings, "serverName").or_else(|| get_str(tls_settings, "server_name"));
    let allow_insecure = get_bool(tls_settings, "allowInsecure");

    let alpn = get_str_array(tls_settings, "alpn");

    let fingerprint = get_str(tls_settings, "fingerprint");

    let reality = if security == "reality" {
        Some(RealityConfig {
            public_key: get_str(tls_settings, "publicKey")
                .or_else(|| get_str(tls_settings, "public_key"))
                .unwrap_or_default(),
            short_id: get_str(tls_settings, "shortId")
                .or_else(|| get_str(tls_settings, "short_id"))
                .unwrap_or_default(),
            spider_x: None,
        })
    } else {
        None
    };

    Some(TlsConfig {
        enabled: true,
        server_name,
        skip_cert_verify: allow_insecure,
        alpn,
        client_fingerprint: fingerprint,
        certificate_pins: vec![],
        reality,
    })
}

/// Extract transport from `streamSettings`.
fn extract_transport(stream: &Value) -> Option<Transport> {
    let network = get_str(stream, "network")?;
    let kind = match network.as_str() {
        "tcp" => TransportKind::Tcp,
        "ws" => TransportKind::Ws,
        "grpc" => TransportKind::Grpc,
        "h2" | "http" => TransportKind::H2,
        "kcp" => TransportKind::Kcp,
        "quic" => TransportKind::Quic,
        "httpupgrade" => TransportKind::HttpUpgrade,
        _ => return None,
    };

    let (path, host) = match kind {
        TransportKind::Ws => {
            let ws = stream.get("wsSettings");
            let path = ws.and_then(|w| get_str(w, "path"));
            let host = ws
                .and_then(|w| w.get("headers"))
                .and_then(|h| get_str(h, "Host"));
            (path, host)
        }
        TransportKind::Grpc => {
            let grpc = stream.get("grpcSettings");
            (grpc.and_then(|g| get_str(g, "serviceName")), None)
        }
        TransportKind::H2 => {
            let h2 = stream.get("httpSettings");
            let path = h2.and_then(|h| get_str(h, "path"));
            let host = h2
                .and_then(|h| h.get("host"))
                .and_then(|h| h.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(String::from);
            (path, host)
        }
        _ => (None, None),
    };

    Some(Transport { kind, path, host })
}

// --- Per-protocol mappers ---

fn parse_vless(entry: &Value, raw_format: &str) -> Result<Node, ParseError> {
    let name = extract_tag(entry);
    let (server, port) = extract_server_port(entry)?;
    let host = parse_host_str(&server)?;

    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;
    let vnext = settings
        .get("vnext")
        .and_then(|v| v.as_array())
        .ok_or(ParseError::MissingField("vnext"))?;
    let first = vnext.first().ok_or(ParseError::MissingField("vnext[0]"))?;
    let users = first
        .get("users")
        .and_then(|u| u.as_array())
        .ok_or(ParseError::MissingField("users"))?;
    let user = users.first().ok_or(ParseError::MissingField("users[0]"))?;
    let uuid = get_str(user, "id").ok_or(ParseError::MissingField("id"))?;

    let stream = entry.get("streamSettings");
    let (tls, transport) = match stream {
        Some(s) => (extract_tls(s), extract_transport(s)),
        None => (None, None),
    };

    let is_reality = tls.as_ref().is_some_and(|t| t.reality.is_some());

    let config = if is_reality {
        ProtocolConfig::VlessReality(VlessRealityConfig {
            encryption: get_str(user, "encryption"),
            flow: get_str(user, "flow"),
            packet_encoding: None,
        })
    } else {
        ProtocolConfig::Unsupported(deve_sub_domain::UnsupportedNode {
            raw: entry.clone(),
            raw_format: Some(raw_format.to_owned()),
            reason: "VLESS without Reality is not P0".to_owned(),
        })
    };

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Vless;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Uuid { uuid };
    node.tls = tls;
    node.transport = transport;
    Ok(node)
}

fn parse_vmess(entry: &Value) -> Result<Node, ParseError> {
    let name = extract_tag(entry);
    let (server, port) = extract_server_port(entry)?;
    let host = parse_host_str(&server)?;

    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;
    let vnext = settings
        .get("vnext")
        .and_then(|v| v.as_array())
        .ok_or(ParseError::MissingField("vnext"))?;
    let first = vnext.first().ok_or(ParseError::MissingField("vnext[0]"))?;
    let users = first
        .get("users")
        .and_then(|u| u.as_array())
        .ok_or(ParseError::MissingField("users"))?;
    let user = users.first().ok_or(ParseError::MissingField("users[0]"))?;
    let uuid = get_str(user, "id").ok_or(ParseError::MissingField("id"))?;

    let alter_id = user
        .get("alterId")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));
    let security = get_str(user, "security");

    let stream = entry.get("streamSettings");
    let (tls, transport) = match stream {
        Some(s) => (extract_tls(s), extract_transport(s)),
        None => (None, None),
    };

    let config = ProtocolConfig::VMess(VMessConfig {
        alter_id,
        security,
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::VMess;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Uuid { uuid };
    node.tls = tls;
    node.transport = transport;
    Ok(node)
}

fn parse_trojan(entry: &Value) -> Result<Node, ParseError> {
    let name = extract_tag(entry);
    let (server, port) = extract_server_port(entry)?;
    let host = parse_host_str(&server)?;

    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;
    let servers = settings
        .get("servers")
        .and_then(|v| v.as_array())
        .ok_or(ParseError::MissingField("servers"))?;
    let first = servers
        .first()
        .ok_or(ParseError::MissingField("servers[0]"))?;
    let password = get_str(first, "password").ok_or(ParseError::MissingField("password"))?;

    let stream = entry.get("streamSettings");
    let (tls, transport) = match stream {
        Some(s) => (extract_tls(s), extract_transport(s)),
        None => (None, None),
    };

    let config = ProtocolConfig::Trojan(TrojanConfig {
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Trojan;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password { password };
    node.tls = tls.or_else(|| Some(default_tls_enabled()));
    node.transport = transport;
    Ok(node)
}

fn parse_shadowsocks(entry: &Value) -> Result<Node, ParseError> {
    let name = extract_tag(entry);
    let (server, port) = extract_server_port(entry)?;
    let host = parse_host_str(&server)?;

    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;
    let servers = settings
        .get("servers")
        .and_then(|v| v.as_array())
        .ok_or(ParseError::MissingField("servers"))?;
    let first = servers
        .first()
        .ok_or(ParseError::MissingField("servers[0]"))?;
    let password = get_str(first, "password").ok_or(ParseError::MissingField("password"))?;
    let method = get_str(first, "method").ok_or(ParseError::MissingField("method"))?;

    let config = ProtocolConfig::Shadowsocks(ShadowsocksConfig {
        method,
        plugin: None,
        plugin_opts: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Shadowsocks;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password { password };
    Ok(node)
}

fn parse_wireguard(entry: &Value) -> Result<Node, ParseError> {
    let name = extract_tag(entry);
    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;

    let secret_key = get_str(settings, "secretKey").ok_or(ParseError::MissingField("secretKey"))?;

    let address = get_str_array(settings, "address");

    let mtu = settings
        .get("mtu")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let reserved = settings
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

    let peers_arr = settings
        .get("peers")
        .and_then(|p| p.as_array())
        .ok_or(ParseError::MissingField("peers"))?;

    let peers: Vec<WireGuardPeer> = peers_arr
        .iter()
        .map(|p| {
            let public_key =
                get_str(p, "publicKey").ok_or(ParseError::MissingField("peer publicKey"))?;
            let pre_shared_key = get_str(p, "preSharedKey");
            let allowed_ips = get_str_array(p, "allowedIPs");
            let keepalive = p
                .get("keepAlive")
                .and_then(|v| v.as_u64())
                .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0)));
            Ok(WireGuardPeer {
                public_key,
                pre_shared_key,
                allowed_ips,
                reserved,
                persistent_keepalive: keepalive,
            })
        })
        .collect::<Result<Vec<_>, ParseError>>()?;

    let (server, port) = peers_arr
        .first()
        .and_then(|p| {
            let endpoint = get_str(p, "endpoint")?;
            let (host_str, port_str) = endpoint.rsplit_once(':')?;
            let port: u16 = port_str.parse().ok()?;
            Some((host_str.to_owned(), port))
        })
        .ok_or(ParseError::MissingField("peer endpoint"))?;
    let host = parse_host_str(&server)?;

    let config = ProtocolConfig::WireGuard(WireGuardConfig {
        private_key: secret_key,
        address,
        peers,
        mtu,
        workers: None,
        dns: vec![],
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::WireGuard;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::None;
    Ok(node)
}
