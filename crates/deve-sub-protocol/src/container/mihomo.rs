//! Mihomo (Clash) YAML parser.
//!
//! Parses the `proxies` section of a Mihomo YAML config into canonical
//! [`Node`] values. Supported proxy types: `vless`, `vmess`, `trojan`,
//! `ss`, `hysteria2`/`hy2`, `tuic`, `naive`. Unknown types are preserved
//! as `UnsupportedNode` (constraint #7).
//!
//! See `docs/plan/05-protocol-engine.md` §"Input formats vs protocols".

use serde_json::Value;

use deve_sub_domain::{
    Authentication, CongestionConfig, CongestionController, Endpoint, Hysteria2Config,
    NaiveProxyConfig, Node, Obfuscation, ProtocolConfig, ProtocolKind, RealityConfig,
    ShadowsocksConfig, TlsConfig, Transport, TransportKind, TrojanConfig, TuicV5Config,
    UdpCapability, UdpRelayMode, VMessConfig, VlessRealityConfig,
};

use crate::error::ParseError;

use super::{
    default_tls_enabled, get_bool, get_port, get_str, get_str_array, node_shell_container,
    parse_host_str, unsupported_entry,
};

/// Parse a Mihomo YAML config into a list of [`Node`] values.
///
/// # Errors
/// Returns [`ParseError::InvalidYaml`] if the YAML is malformed.
/// Returns [`ParseError::MissingContainerKey`] if the `proxies` key is absent.
pub fn parse_mihomo_yaml(text: &str) -> Result<Vec<Node>, ParseError> {
    let value: Value =
        serde_yaml::from_str(text).map_err(|e| ParseError::InvalidYaml(e.to_string()))?;

    let proxies = value
        .get("proxies")
        .ok_or(ParseError::MissingContainerKey("proxies"))?
        .as_array()
        .ok_or(ParseError::MissingContainerKey("proxies (not a list)"))?;

    Ok(proxies.iter().map(parse_proxy_entry).collect())
}

/// Dispatch a single proxy entry to the appropriate protocol mapper.
fn parse_proxy_entry(entry: &Value) -> Node {
    let proxy_type = match get_str(entry, "type") {
        Some(t) => t,
        None => {
            return unsupported_entry(
                entry,
                "mihomo-yaml",
                ProtocolKind::Unknown(String::new()),
                "missing 'type' field".to_owned(),
            );
        }
    };

    let result = match proxy_type.as_str() {
        "vless" => parse_vless(entry),
        "vmess" => parse_vmess(entry),
        "trojan" => parse_trojan(entry),
        "ss" => parse_shadowsocks(entry),
        "hysteria2" | "hy2" => parse_hysteria2(entry),
        "tuic" => parse_tuic(entry),
        "naive" => parse_naive(entry),
        other => Err(ParseError::UnsupportedProxyType(other.to_owned())),
    };

    match result {
        Ok(node) => node,
        Err(e) => unsupported_entry(
            entry,
            "mihomo-yaml",
            ProtocolKind::Unknown(proxy_type),
            e.to_string(),
        ),
    }
}

/// Extract common fields and build the endpoint + display name.
fn build_base(entry: &Value) -> Result<(String, Endpoint), ParseError> {
    let name = get_str(entry, "name").unwrap_or_default();
    let server = get_str(entry, "server").ok_or(ParseError::MissingField("server"))?;
    let port = get_port(entry, "port")?.ok_or(ParseError::MissingField("port"))?;
    let host = parse_host_str(&server)?;
    Ok((name, Endpoint { host, port }))
}

/// Extract TLS config from a Mihomo proxy entry.
fn extract_tls(entry: &Value) -> Option<TlsConfig> {
    let tls_enabled = get_bool(entry, "tls").unwrap_or(false);
    let server_name = get_str(entry, "servername").or_else(|| get_str(entry, "sni"));
    let skip_cert_verify = get_bool(entry, "skip-cert-verify");
    let alpn = get_str_array(entry, "alpn");
    let fingerprint = get_str(entry, "client-fingerprint");

    let reality = entry.get("reality-opts").map(|ro| RealityConfig {
        public_key: get_str(ro, "public-key").unwrap_or_default(),
        short_id: get_str(ro, "short-id").unwrap_or_default(),
        spider_x: None,
    });

    if !tls_enabled
        && server_name.is_none()
        && skip_cert_verify.is_none()
        && alpn.is_empty()
        && fingerprint.is_none()
        && reality.is_none()
    {
        return None;
    }

    Some(TlsConfig {
        enabled: tls_enabled || reality.is_some(),
        server_name,
        skip_cert_verify,
        alpn,
        client_fingerprint: fingerprint,
        certificate_pins: vec![],
        reality,
    })
}

/// Extract transport from a Mihomo proxy entry.
fn extract_transport(entry: &Value) -> Result<Option<Transport>, ParseError> {
    let network = match get_str(entry, "network") {
        Some(n) => n,
        None => return Ok(None),
    };

    let kind = match network.as_str() {
        "tcp" => TransportKind::Tcp,
        "ws" => TransportKind::Ws,
        "grpc" => TransportKind::Grpc,
        "h2" => TransportKind::H2,
        "kcp" => TransportKind::Kcp,
        "quic" => TransportKind::Quic,
        "httpupgrade" => TransportKind::HttpUpgrade,
        _ => return Ok(None),
    };

    let (path, host) = match kind {
        TransportKind::Ws => {
            let ws_opts = entry.get("ws-opts");
            let path = ws_opts.and_then(|w| get_str(w, "path"));
            let host = ws_opts
                .and_then(|w| w.get("headers"))
                .and_then(|h| get_str(h, "Host"));
            (path, host)
        }
        TransportKind::Grpc => {
            let grpc_opts = entry.get("grpc-opts");
            let path = grpc_opts.and_then(|g| get_str(g, "grpc-service-name"));
            (path, None)
        }
        TransportKind::H2 => {
            let h2_opts = entry.get("h2-opts");
            let path = h2_opts.and_then(|h| get_str(h, "path"));
            let host = h2_opts.and_then(|h| get_str(h, "host"));
            (path, host)
        }
        _ => (None, None),
    };

    Ok(Some(Transport { kind, path, host }))
}

/// Extract UDP capability from a Mihomo proxy entry.
fn extract_udp(entry: &Value) -> UdpCapability {
    UdpCapability {
        supported: get_bool(entry, "udp"),
        xudp: get_bool(entry, "xudp"),
    }
}

// --- Per-protocol mappers ---

fn parse_vless(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let uuid = get_str(entry, "uuid").ok_or(ParseError::MissingField("uuid"))?;

    let tls = extract_tls(entry);
    let transport = extract_transport(entry)?;
    let udp = extract_udp(entry);

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
    node.transport = transport;
    node.tls = tls;
    node.udp = udp;
    Ok(node)
}

fn parse_trojan(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);
    let transport = extract_transport(entry)?;

    let config = ProtocolConfig::Trojan(TrojanConfig {
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Trojan;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.transport = transport;
    node.tls = Some(tls);
    node.udp = extract_udp(entry);
    Ok(node)
}

fn parse_shadowsocks(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;
    let method = get_str(entry, "cipher").ok_or(ParseError::MissingField("cipher"))?;

    let plugin = get_str(entry, "plugin");
    // WHY: Mihomo stores plugin-opts as a YAML map, but ShadowsocksConfig
    // expects SIP003 `k=v;k=v` string format. Converting YAML map → SIP003
    // is non-trivial (key ordering, value escaping); deferred to avoid
    // emitting malformed strings. The plugin name is still preserved.
    let plugin_opts: Option<String> = None;

    let config = ProtocolConfig::Shadowsocks(ShadowsocksConfig {
        method,
        plugin,
        plugin_opts,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Shadowsocks;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.udp = extract_udp(entry);
    Ok(node)
}

fn parse_vmess(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let uuid = get_str(entry, "uuid").ok_or(ParseError::MissingField("uuid"))?;

    let alter_id = entry
        .get("alterId")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));
    let security = get_str(entry, "cipher");

    let tls = extract_tls(entry);
    let transport = extract_transport(entry)?;

    let config = ProtocolConfig::VMess(VMessConfig {
        alter_id,
        security,
        packet_encoding: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::VMess;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Uuid { uuid };
    node.transport = transport;
    node.tls = tls;
    node.udp = extract_udp(entry);
    Ok(node)
}

fn parse_hysteria2(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);

    let obfuscation = get_str(entry, "obfs").map(|kind| Obfuscation {
        kind,
        password: get_str(entry, "obfs-password"),
    });

    let congestion =
        (get_str(entry, "up").is_some() || get_str(entry, "down").is_some()).then(|| {
            CongestionConfig {
                controller: CongestionController::Bbr,
                up_bps: get_str(entry, "up").and_then(|s| crate::uri::parse_bandwidth(&s).ok()),
                down_bps: get_str(entry, "down").and_then(|s| crate::uri::parse_bandwidth(&s).ok()),
            }
        });

    let config = ProtocolConfig::Hysteria2(Hysteria2Config {
        ports: get_str(entry, "ports"),
        hop_interval: None,
        fast_open: get_bool(entry, "fast-open"),
        lazy: get_bool(entry, "lazy"),
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::Hysteria2;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.tls = Some(tls);
    node.obfuscation = obfuscation;
    node.congestion = congestion;
    node.udp = extract_udp(entry);
    Ok(node)
}

fn parse_tuic(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let uuid = get_str(entry, "uuid").ok_or(ParseError::MissingField("uuid"))?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);

    let congestion_controller = get_str(entry, "congestion-controller").map(|c| match c.as_str() {
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

    let udp_relay_mode = get_str(entry, "udp-relay-mode").and_then(|m| match m.as_str() {
        "native" => Some(UdpRelayMode::Native),
        "quic" => Some(UdpRelayMode::Quic),
        _ => None,
    });

    let heartbeat = entry
        .get("heartbeat")
        .and_then(|v| v.as_u64())
        .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX)));

    let config = ProtocolConfig::TuicV5(TuicV5Config {
        udp_relay_mode,
        zero_rtt_handshake: get_bool(entry, "zero-rtt-handshake")
            .or_else(|| get_bool(entry, "reduce-rtt")),
        heartbeat,
        disable_sni: get_bool(entry, "disable-sni"),
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::TuicV5;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::UuidPassword { uuid, password };
    node.tls = Some(tls);
    node.congestion = congestion;
    node.udp = extract_udp(entry);
    Ok(node)
}

fn parse_naive(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let username = get_str(entry, "username").ok_or(ParseError::MissingField("username"))?;
    let password = get_str(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);

    let config = ProtocolConfig::NaiveProxy(NaiveProxyConfig {
        quic: get_bool(entry, "quic"),
        http2: get_bool(entry, "http2"),
        http3: get_bool(entry, "http3"),
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::NaiveProxy;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::UserPassword { username, password };
    node.tls = Some(tls);
    node.udp = extract_udp(entry);
    Ok(node)
}
