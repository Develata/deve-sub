//! Mihomo (Clash) YAML parser.
//!
//! Parses the `proxies` section of a Mihomo YAML config into canonical
//! [`Node`] values. Supported proxy types: `vless`, `vmess`, `trojan`,
//! `ss`, `hysteria2`/`hy2`, `tuic`, `naive`. Unknown types are preserved
//! as `UnsupportedNode` (constraint #7).
//!
//! See `docs/plan/05-protocol-engine.md` §"Input formats vs protocols".

mod plugin;
mod shadowtls;
mod snell;
use snell::parse_snell;

use serde_json::Value;

use deve_sub_domain::{
    AnyTlsConfig, Authentication, CongestionConfig, CongestionController, Endpoint,
    Hysteria2Config, NaiveProxyConfig, Node, Obfuscation, ProtocolConfig, ProtocolKind,
    RealityConfig, ShadowsocksConfig, TlsConfig, Transport, TransportKind, TrojanConfig,
    TuicV5Config, UdpCapability, UdpRelayMode, VMessConfig, VlessRealityConfig, WireGuardConfig,
    WireGuardPeer, XhttpMode,
};

use crate::error::ParseError;

use super::{
    default_tls_enabled, get_bool, get_port, get_scalar_text, get_str, get_str_array,
    get_str_or_first, node_shell_container, parse_host_str, unsupported_entry,
};

/// Parse a Mihomo YAML config into a list of [`Node`] values.
///
/// # Errors
/// Returns [`ParseError::InvalidYaml`] if the YAML is malformed.
/// Returns [`ParseError::MissingContainerKey`] if the `proxies` key is absent.
pub fn parse_mihomo_yaml(text: &str) -> Result<Vec<Node>, ParseError> {
    let value = deve_sub_yaml::parse_source_json(text)
        .map_err(|e| ParseError::InvalidYaml(e.to_string()))?;
    parse_mihomo_value(&value)
}

/// Reuse an already-decoded JSON projection during automatic format detection.
pub(crate) fn parse_mihomo_value(value: &Value) -> Result<Vec<Node>, ParseError> {
    let proxies = value
        .get("proxies")
        .ok_or(ParseError::MissingContainerKey("proxies"))?
        .as_array()
        .ok_or(ParseError::MissingContainerKey("proxies (not a list)"))?;
    super::check_entry_count(proxies.len())?;

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
    if let Err(error) = super::native_tls::mihomo(entry) {
        return unsupported_entry(
            entry,
            "mihomo-yaml",
            ProtocolKind::Unknown(proxy_type),
            error.to_string(),
        );
    }

    let result = match proxy_type.as_str() {
        "socks" | "socks5" | "socks5h" | "http" | "https" => {
            super::forward_proxy::mihomo(entry, &proxy_type)
        }
        "vless" => parse_vless(entry),
        "vmess" => parse_vmess(entry),
        "trojan" => parse_trojan(entry),
        "ss" => parse_shadowsocks(entry),
        "hysteria2" | "hy2" => parse_hysteria2(entry),
        "tuic" => parse_tuic(entry),
        "naive" => parse_naive(entry),
        "wireguard" => parse_wireguard(entry),
        "anytls" => parse_anytls(entry),
        "snell" => parse_snell(entry),
        other => Err(ParseError::UnsupportedProxyType(other.to_owned())),
    };

    match result {
        Ok(node) => {
            // WHY: mihomo projects ShadowTLS as an obfuscation layer under
            // the inner protocol type — detect `shadow-tls-opts` (vless/
            // trojan/vmess/anytls), `plugin: shadow-tls` (ss), or
            // `obfs-opts.mode: shadow-tls` (snell) and wrap the parsed
            // inner node in `ProtocolConfig::ShadowTls`.
            match shadowtls::project(entry, &node) {
                Ok(Some(wrapper)) => wrapper,
                Ok(None) => node,
                Err(error) => unsupported_entry(
                    entry,
                    "mihomo-yaml",
                    ProtocolKind::ShadowTls,
                    error.to_string(),
                ),
            }
        }
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
    let name = get_scalar_text(entry, "name").unwrap_or_default();
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
        short_id: get_scalar_text(ro, "short-id").unwrap_or_default(),
        spider_x: None,
    });

    let certificate_pins = super::native_tls::mihomo(entry).unwrap_or_default();
    if !tls_enabled
        && server_name.is_none()
        && skip_cert_verify.is_none()
        && alpn.is_empty()
        && fingerprint.is_none()
        && reality.is_none()
        && certificate_pins.is_empty()
    {
        return None;
    }

    Some(TlsConfig {
        enabled: tls_enabled
            || reality.is_some()
            || matches!(
                get_str(entry, "type").as_deref(),
                Some("trojan" | "hysteria2" | "hy2" | "tuic" | "anytls")
            ),
        server_name,
        skip_cert_verify,
        alpn,
        client_fingerprint: fingerprint,
        certificate_pins,
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
        "xhttp" => TransportKind::Xhttp,
        _ => return Ok(None),
    };

    let (path, host, xhttp_mode) = match kind {
        TransportKind::Ws | TransportKind::HttpUpgrade => {
            let ws_opts = entry.get("ws-opts");
            let path = ws_opts.and_then(|w| get_str(w, "path"));
            let host = ws_opts
                .and_then(|w| w.get("headers"))
                .and_then(|h| get_str(h, "Host"));
            (path, host, None)
        }
        TransportKind::Grpc => {
            let grpc_opts = entry.get("grpc-opts");
            let path = grpc_opts.and_then(|g| get_str(g, "grpc-service-name"));
            (path, None, None)
        }
        TransportKind::H2 => {
            let h2_opts = entry.get("h2-opts");
            let path = h2_opts.and_then(|h| get_str(h, "path"));
            let host = h2_opts.and_then(|h| get_str_or_first(h, "host"));
            (path, host, None)
        }
        TransportKind::Xhttp => {
            let xopts = entry.get("xhttp-opts");
            let path = xopts.and_then(|x| get_str(x, "path"));
            let host = xopts.and_then(|x| get_str(x, "host"));
            let mode = xopts
                .and_then(|x| get_str(x, "mode"))
                .as_deref()
                .and_then(XhttpMode::from_str_lossy)
                .unwrap_or(XhttpMode::Auto);
            (path, host, Some(mode))
        }
        _ => (None, None, None),
    };

    Ok(Some(Transport {
        kind,
        path,
        host,
        xhttp_mode,
    }))
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
        packet_encoding: get_str(entry, "packet-encoding"),
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
    let password =
        get_scalar_text(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);
    let transport = extract_transport(entry)?;

    let config = ProtocolConfig::Trojan(TrojanConfig {
        packet_encoding: get_str(entry, "packet-encoding"),
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
    let password =
        get_scalar_text(entry, "password").ok_or(ParseError::MissingField("password"))?;
    let method = get_str(entry, "cipher").ok_or(ParseError::MissingField("cipher"))?;

    let (plugin, plugin_opts) = plugin::parse(entry)?;

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
        packet_encoding: get_str(entry, "packet-encoding"),
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
    let password =
        get_scalar_text(entry, "password").ok_or(ParseError::MissingField("password"))?;

    let tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);

    let obfuscation = get_str(entry, "obfs").map(|kind| Obfuscation {
        kind,
        password: get_str(entry, "obfs-password"),
    });

    let up_bps = super::bandwidth::mihomo(entry, "up")?;
    let down_bps = super::bandwidth::mihomo(entry, "down")?;
    let controller = get_str(entry, "congestion-controller").map(|c| match c.as_str() {
        "bbr" => CongestionController::Bbr,
        "cubic" => CongestionController::Cubic,
        "new_reno" => CongestionController::NewReno,
        other => CongestionController::Other(other.to_owned()),
    });
    // WHY: preserve congestion-controller on round-trip. Create
    // CongestionConfig when any of up/down/controller is present,
    // defaulting to Bbr when only bandwidth is specified.
    let congestion = if up_bps.is_some() || down_bps.is_some() || controller.is_some() {
        Some(CongestionConfig {
            controller: controller.unwrap_or(CongestionController::Bbr),
            up_bps,
            down_bps,
        })
    } else {
        None
    };

    // WHY: mihomo stores `hop-interval` as a string of seconds (min 5,
    // default 30 in mihomo). The canonical model stores a Duration. The
    // field may appear as either a YAML string (`hop-interval: "30"`) or a
    // YAML number (`hop-interval: 30`); our own emitter writes the numeric
    // form, so the parser must accept both to preserve round-trip fidelity
    // (SRC-017). See P3.
    let hop_interval = entry.get("hop-interval").and_then(|v| match v {
        serde_json::Value::String(s) => crate::uri::parse_duration_secs(s).ok(),
        serde_json::Value::Number(n) => {
            let secs = n.as_u64()?;
            let secs_i64 = i64::try_from(secs).ok()?;
            Some(time::Duration::seconds(secs_i64))
        }
        _ => None,
    });

    let config = ProtocolConfig::Hysteria2(Hysteria2Config {
        ports: get_str(entry, "ports"),
        hop_interval,
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
    let password =
        get_scalar_text(entry, "password").ok_or(ParseError::MissingField("password"))?;

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

    if entry.get("heartbeat").is_some() {
        return Err(crate::forward_proxy::invalid(
            "Mihomo TUIC uses heartbeat-interval",
        ));
    }
    let heartbeat = entry
        .get("heartbeat-interval")
        .map(|_| {
            let millis = get_scalar_text(entry, "heartbeat-interval")
                .and_then(|n| n.parse::<u64>().ok())
                .and_then(|n| i64::try_from(n).ok())
                .ok_or_else(|| {
                    crate::forward_proxy::invalid(
                        "TUIC heartbeat requires non-negative integer milliseconds",
                    )
                })?;
            Ok(time::Duration::milliseconds(millis))
        })
        .transpose()?;

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
    let password =
        get_scalar_text(entry, "password").ok_or(ParseError::MissingField("password"))?;

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

fn parse_wireguard(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let private_key =
        get_str(entry, "private-key").ok_or(ParseError::MissingField("private-key"))?;

    let mut address = Vec::new();
    if let Some(ip) = get_str(entry, "ip") {
        address.push(ip);
    }
    if let Some(ipv6) = get_str(entry, "ipv6") {
        address.push(ipv6);
    }

    let mtu = entry
        .get("mtu")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let workers = entry
        .get("workers")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let dns = get_str_array(entry, "dns");

    let peers = if let Some(peers_arr) = entry.get("peers").and_then(|p| p.as_array()) {
        peers_arr
            .iter()
            .map(|p| parse_wireguard_peer(p, &endpoint))
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(pub_key) = get_str(entry, "public-key") {
        vec![WireGuardPeer {
            public_key: pub_key,
            pre_shared_key: get_str(entry, "pre-shared-key"),
            allowed_ips: get_str_array(entry, "allowed-ips"),
            reserved: parse_reserved_array(entry.get("reserved")),
            persistent_keepalive: entry
                .get("persistent-keepalive")
                .and_then(|v| v.as_u64())
                .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0))),
        }]
    } else {
        return Err(ParseError::MissingField("public-key or peers"));
    };

    let config = ProtocolConfig::WireGuard(WireGuardConfig {
        private_key,
        address,
        peers,
        mtu,
        workers,
        dns,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::WireGuard;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::None;
    node.udp = UdpCapability {
        supported: Some(true),
        xudp: None,
    };
    Ok(node)
}

fn parse_wireguard_peer(
    peer: &Value,
    node_endpoint: &Endpoint,
) -> Result<WireGuardPeer, ParseError> {
    let public_key =
        get_str(peer, "public-key").ok_or(ParseError::MissingField("peer public-key"))?;
    let pre_shared_key = get_str(peer, "pre-shared-key");
    let allowed_ips = get_str_array(peer, "allowed-ips");
    let reserved = parse_reserved_array(peer.get("reserved"));
    let persistent_keepalive = peer
        .get("persistent-keepalive")
        .and_then(|v| v.as_u64())
        .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0)));

    let _ = node_endpoint;
    Ok(WireGuardPeer {
        public_key,
        pre_shared_key,
        allowed_ips,
        reserved,
        persistent_keepalive,
    })
}

fn parse_reserved_array(val: Option<&Value>) -> Option<[u8; 3]> {
    let arr = val?.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    let mut bytes = [0u8; 3];
    for (i, v) in arr.iter().enumerate() {
        bytes[i] = v.as_u64()?.try_into().ok()?;
    }
    Some(bytes)
}

fn parse_anytls(entry: &Value) -> Result<Node, ParseError> {
    let (name, endpoint) = build_base(entry)?;
    let password =
        get_scalar_text(entry, "password").ok_or(ParseError::MissingField("password"))?;

    // WHY: AnyTLS always requires TLS; fall back to a default-enabled TLS
    // config when no TLS-related fields are present (matches Trojan handling).
    // `extract_tls` may return Some with `enabled: false` when TLS sub-fields
    // (sni/alpn/fingerprint) are present but `tls: true` is absent — force
    // enabled=true for AnyTLS since the protocol mandates TLS.
    let mut tls = extract_tls(entry).unwrap_or_else(default_tls_enabled);
    tls.enabled = true;

    let idle_session_check_interval = entry
        .get("idle-session-check-interval")
        .and_then(|v| v.as_u64())
        .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0)));
    let idle_session_timeout = entry
        .get("idle-session-timeout")
        .and_then(|v| v.as_u64())
        .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0)));
    let min_idle_session = entry
        .get("min-idle-session")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let config = ProtocolConfig::AnyTls(AnyTlsConfig {
        idle_session_check_interval,
        idle_session_timeout,
        min_idle_session,
        // mihomo does not expose client_metadata; sing-box only.
        client_metadata: None,
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::AnyTls;
    node.config = config;
    node.endpoint = endpoint;
    node.authentication = Authentication::Password { password };
    node.tls = Some(tls);
    node.udp = extract_udp(entry);
    Ok(node)
}
