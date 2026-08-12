//! Mihomo (Clash Meta) YAML container emitter.
//!
//! Emits a `proxies:` array with one entry per compatible node. Each entry
//! follows the Mihomo proxy schema. The full template (proxy-groups, rules,
//! dns) is assembled in Slice 5.

use deve_sub_domain::{
    Authentication, Node, ProtocolConfig, ProtocolKind, SnellObfsMode, Transport, TransportKind,
};

use crate::error::EmitError;

pub fn emit(nodes: &[Node]) -> Result<String, EmitError> {
    let mut lines = Vec::new();
    lines.push("proxies:".to_owned());
    for node in nodes {
        emit_proxy(node, &mut lines)?;
    }
    Ok(lines.join("\n"))
}

fn emit_proxy(node: &Node, lines: &mut Vec<String>) -> Result<(), EmitError> {
    let server = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let name = &node.display_name;

    match node.protocol {
        ProtocolKind::Trojan => emit_trojan(node, &server, port, name, lines),
        ProtocolKind::Shadowsocks => emit_ss(node, &server, port, name, lines),
        ProtocolKind::VMess => emit_vmess(node, &server, port, name, lines),
        ProtocolKind::Vless => emit_vless(node, &server, port, name, lines),
        ProtocolKind::Hysteria2 => emit_hysteria2(node, &server, port, name, lines),
        ProtocolKind::TuicV5 => emit_tuic_v5(node, &server, port, name, lines),
        ProtocolKind::WireGuard => emit_wireguard(node, &server, port, name, lines),
        ProtocolKind::AnyTls => emit_anytls(node, &server, port, name, lines),
        ProtocolKind::Snell => emit_snell(node, &server, port, name, lines),
        ProtocolKind::ShadowTls => emit_shadowtls(node, &server, port, name, lines),
        ref other => {
            return Err(EmitError::NoEmitter(format!(
                "mihomo: unsupported protocol {other}"
            )));
        }
    }?;
    Ok(())
}

fn yaml_entry(name: &str, ptype: &str, server: &str, port: u16) -> String {
    format!("  - name: \"{name}\"\n    type: {ptype}\n    server: {server}\n    port: {port}")
}

fn emit_trojan(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("trojan password")),
    };
    let mut entry = yaml_entry(name, "trojan", server, port);
    entry.push_str(&format!("\n    password: \"{password}\""));
    push_tls(node, &mut entry);
    push_network(node, &mut entry);
    lines.push(entry);
    Ok(())
}

fn emit_ss(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("ss password")),
    };
    let method = match &node.config {
        ProtocolConfig::Shadowsocks(cfg) => &cfg.method,
        _ => return Err(EmitError::MissingField("ss method")),
    };
    let entry = format!(
        "{}\n    cipher: {method}\n    password: \"{password}\"",
        yaml_entry(name, "ss", server, port)
    );
    lines.push(entry);
    Ok(())
}

fn emit_vmess(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(EmitError::MissingField("vmess uuid")),
    };
    let mut entry = yaml_entry(name, "vmess", server, port);
    entry.push_str(&format!("\n    uuid: \"{uuid}\""));
    if let ProtocolConfig::VMess(cfg) = &node.config {
        if let Some(aid) = cfg.alter_id {
            entry.push_str(&format!("\n    alterId: {aid}"));
        }
        if let Some(ref sec) = cfg.security {
            entry.push_str(&format!("\n    cipher: {sec}"));
        }
    }
    push_tls(node, &mut entry);
    push_network(node, &mut entry);
    lines.push(entry);
    Ok(())
}

fn emit_vless(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(EmitError::MissingField("vless uuid")),
    };
    let mut entry = yaml_entry(name, "vless", server, port);
    entry.push_str(&format!("\n    uuid: \"{uuid}\""));
    if let ProtocolConfig::VlessReality(cfg) = &node.config
        && let Some(ref flow) = cfg.flow
    {
        entry.push_str(&format!("\n    flow: {flow}"));
    }
    push_tls(node, &mut entry);
    push_network(node, &mut entry);
    lines.push(entry);
    Ok(())
}

fn emit_hysteria2(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("hysteria2 password")),
    };
    let mut entry = yaml_entry(name, "hysteria2", server, port);
    entry.push_str(&format!("\n    password: \"{password}\""));
    push_tls(node, &mut entry);
    lines.push(entry);
    Ok(())
}

fn emit_tuic_v5(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let (uuid, password) = match &node.authentication {
        Authentication::UuidPassword { uuid, password } => (uuid, password),
        _ => return Err(EmitError::MissingField("tuic v5 uuid+password")),
    };
    let mut entry = yaml_entry(name, "tuic", server, port);
    entry.push_str(&format!("\n    uuid: \"{uuid}\""));
    entry.push_str(&format!("\n    password: \"{password}\""));
    push_tls(node, &mut entry);
    lines.push(entry);
    Ok(())
}

fn emit_wireguard(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let cfg = match &node.config {
        ProtocolConfig::WireGuard(c) => c,
        _ => return Err(EmitError::MissingField("wireguard config")),
    };
    let mut entry = yaml_entry(name, "wireguard", server, port);
    entry.push_str(&format!("\n    private-key: \"{}\"", cfg.private_key));

    // Mihomo splits WireGuard interface addresses into `ip` (IPv4) and
    // `ipv6` (IPv6), each a single string. Emit by simple colon heuristic.
    if !cfg.address.is_empty() {
        let v4 = cfg.address.iter().find(|a| !a.contains(':'));
        let v6 = cfg.address.iter().find(|a| a.contains(':'));
        if let Some(ip) = v4 {
            entry.push_str(&format!("\n    ip: \"{ip}\""));
        }
        if let Some(ipv6) = v6 {
            entry.push_str(&format!("\n    ipv6: \"{ipv6}\""));
        }
    }
    if let Some(mtu) = cfg.mtu {
        entry.push_str(&format!("\n    mtu: {mtu}"));
    }
    if let Some(workers) = cfg.workers {
        entry.push_str(&format!("\n    workers: {workers}"));
    }
    if !cfg.dns.is_empty() {
        let dns: Vec<String> = cfg.dns.iter().map(|d| format!("\"{d}\"")).collect();
        entry.push_str(&format!("\n    dns: [{}]", dns.join(", ")));
    }

    if let Some(peer) = cfg.peers.first() {
        entry.push_str("\n    peers:");
        entry.push_str(&format!("\n      - server: {server}"));
        entry.push_str(&format!("\n        port: {port}"));
        entry.push_str(&format!("\n        public-key: \"{}\"", peer.public_key));
        if let Some(ref psk) = peer.pre_shared_key {
            entry.push_str(&format!("\n        pre-shared-key: \"{psk}\""));
        }
        if !peer.allowed_ips.is_empty() {
            let ips: Vec<String> = peer
                .allowed_ips
                .iter()
                .map(|ip| format!("\"{ip}\""))
                .collect();
            entry.push_str(&format!("\n        allowed-ips: [{}]", ips.join(", ")));
        }
        if let Some(reserved) = peer.reserved {
            entry.push_str(&format!(
                "\n        reserved: [{}, {}, {}]",
                reserved[0], reserved[1], reserved[2]
            ));
        }
        if let Some(ka) = peer.persistent_keepalive {
            let secs = ka.whole_seconds();
            if secs >= 0 {
                entry.push_str(&format!("\n        persistent-keepalive: {secs}"));
            }
        }
    }

    lines.push(entry);
    Ok(())
}

fn emit_anytls(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("anytls password")),
    };
    let cfg = match &node.config {
        ProtocolConfig::AnyTls(c) => c,
        _ => return Err(EmitError::MissingField("anytls config")),
    };
    let mut entry = yaml_entry(name, "anytls", server, port);
    entry.push_str(&format!("\n    password: \"{password}\""));
    push_tls(node, &mut entry);
    if let Some(d) = cfg.idle_session_check_interval {
        entry.push_str(&format!(
            "\n    idle-session-check-interval: {}",
            d.whole_seconds()
        ));
    }
    if let Some(d) = cfg.idle_session_timeout {
        entry.push_str(&format!(
            "\n    idle-session-timeout: {}",
            d.whole_seconds()
        ));
    }
    if let Some(n) = cfg.min_idle_session {
        entry.push_str(&format!("\n    min-idle-session: {n}"));
    }
    lines.push(entry);
    Ok(())
}

fn emit_snell(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let psk = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("snell psk")),
    };
    let cfg = match &node.config {
        ProtocolConfig::Snell(c) => c,
        _ => return Err(EmitError::MissingField("snell config")),
    };
    let mut entry = yaml_entry(name, "snell", server, port);
    entry.push_str(&format!("\n    psk: \"{psk}\""));
    entry.push_str(&format!("\n    version: {}", cfg.version.as_u32()));
    if let Some(supported) = node.udp.supported {
        entry.push_str(&format!("\n    udp: {supported}"));
    }
    if let Some(reuse) = cfg.reuse {
        entry.push_str(&format!("\n    reuse: {reuse}"));
    }
    if let Some(ref obfs) = cfg.obfs {
        let mode_str = match obfs.mode {
            SnellObfsMode::Tls => "tls",
            SnellObfsMode::Http => "http",
            SnellObfsMode::ShadowTls => "shadow-tls",
            SnellObfsMode::Restls => "restls",
            SnellObfsMode::Jls => "jls",
        };
        entry.push_str(&format!("\n    obfs-opts:\n      mode: {mode_str}"));
        if let Some(ref host) = obfs.host {
            entry.push_str(&format!("\n      host: \"{host}\""));
        }
        if let Some(ref pw) = obfs.password {
            entry.push_str(&format!("\n      password: \"{pw}\""));
        }
        if let Some(v) = obfs.version {
            entry.push_str(&format!("\n      version: {v}"));
        }
        if !obfs.alpn.is_empty() {
            let alpn: Vec<String> = obfs.alpn.iter().map(|a| format!("\"{a}\"")).collect();
            entry.push_str(&format!("\n      alpn: [{}]", alpn.join(", ")));
        }
    }
    // WHY: when obfs.mode=tls the TLS-shaped fields live on `node.tls`; emit
    // them at top-level (mihomo Snell reads sni/alpn/skip-cert-verify there).
    if matches!(cfg.obfs.as_ref().map(|o| o.mode), Some(SnellObfsMode::Tls)) {
        push_tls(node, &mut entry);
    }
    lines.push(entry);
    Ok(())
}

/// Emit a ShadowTLS node by projecting it back under the inner protocol
/// type with a ShadowTLS obfuscation layer (mihomo pattern).
///
/// - inner = Shadowsocks → `type: ss` + `plugin: shadow-tls` + `plugin-opts`
/// - inner = Snell → `type: snell` + `obfs-opts: { mode: shadow-tls }`
/// - inner = VLESS/Trojan/VMess/AnyTLS → `type: <inner>` + `shadow-tls-opts`
fn emit_shadowtls(
    node: &Node,
    _server: &str,
    _port: u16,
    _name: &str,
    lines: &mut Vec<String>,
) -> Result<(), EmitError> {
    let cfg = match &node.config {
        ProtocolConfig::ShadowTls(c) => c,
        _ => return Err(EmitError::MissingField("shadowtls config")),
    };

    // WHY: synthesize an inner Node carrying only the inner protocol config
    // + authentication, then delegate to the inner protocol's emitter. The
    // ShadowTLS obfs layer is injected after, as `shadow-tls-opts` /
    // `plugin-opts` / `obfs-opts` depending on the inner protocol type.
    let mut inner_node = node.clone();
    inner_node.protocol = cfg.inner_protocol.clone();
    inner_node.config = (*cfg.inner_config).clone();
    // Inner TLS is vestigial; shadowtls camouflage TLS lives on node.tls.
    inner_node.tls = None;

    // Capture the inner emitter's output into a temp buffer, then append
    // the ShadowTLS obfs fields before pushing to `lines`.
    let mut inner_lines: Vec<String> = Vec::new();
    emit_proxy(&inner_node, &mut inner_lines)?;
    let inner_entry = inner_lines.into_iter().next().ok_or_else(|| {
        EmitError::NoEmitter("shadowtls: inner emitter produced no output".to_owned())
    })?;

    let mut entry = inner_entry;

    // Inject ShadowTLS obfs layer based on inner protocol type.
    match cfg.inner_protocol {
        ProtocolKind::Shadowsocks => {
            // ss uses `plugin: shadow-tls` + `plugin-opts`.
            entry.push_str("\n    plugin: shadow-tls");
            entry.push_str("\n    plugin-opts:");
            entry.push_str(&format!("\n      version: {}", cfg.version.as_u32()));
            if let Some(ref pw) = cfg.password {
                entry.push_str(&format!("\n      password: \"{pw}\""));
            }
            if let Some(ref tls) = node.tls
                && let Some(ref sni) = tls.server_name
            {
                entry.push_str(&format!("\n      host: \"{sni}\""));
            }
        }
        ProtocolKind::Snell => {
            // snell uses `obfs-opts.mode: shadow-tls`.
            entry.push_str("\n    obfs-opts:");
            entry.push_str("\n      mode: shadow-tls");
            entry.push_str(&format!("\n      version: {}", cfg.version.as_u32()));
            if let Some(ref pw) = cfg.password {
                entry.push_str(&format!("\n      password: \"{pw}\""));
            }
            if let Some(ref tls) = node.tls
                && let Some(ref sni) = tls.server_name
            {
                entry.push_str(&format!("\n      host: \"{sni}\""));
            }
        }
        ProtocolKind::Vless | ProtocolKind::Trojan | ProtocolKind::VMess | ProtocolKind::AnyTls => {
            // vless/trojan/vmess/anytls use `shadow-tls-opts`.
            entry.push_str("\n    shadow-tls-opts:");
            entry.push_str(&format!("\n      version: {}", cfg.version.as_u32()));
            if let Some(ref pw) = cfg.password {
                entry.push_str(&format!("\n      password: \"{pw}\""));
            }
            if let Some(ref tls) = node.tls
                && let Some(ref sni) = tls.server_name
            {
                entry.push_str(&format!("\n      sni: \"{sni}\""));
            }
            // WHY: camouflage TLS fields (skip-cert-verify, alpn) are emitted
            // at top-level via push_tls, since mihomo reads them there for
            // the shadowtls handshake target.
            push_tls(node, &mut entry);
        }
        ref other => {
            return Err(EmitError::NoEmitter(format!(
                "mihomo shadowtls: inner protocol {other} not supported"
            )));
        }
    }

    lines.push(entry);
    Ok(())
}

fn push_tls(node: &Node, entry: &mut String) {
    if let Some(ref tls) = node.tls {
        if tls.enabled {
            entry.push_str("\n    tls: true");
        }
        if let Some(ref sni) = tls.server_name {
            entry.push_str(&format!("\n    sni: {sni}"));
        }
        if let Some(skip) = tls.skip_cert_verify {
            entry.push_str(&format!("\n    skip-cert-verify: {skip}"));
        }
        if !tls.alpn.is_empty() {
            let alpn: Vec<String> = tls.alpn.iter().map(|a| format!("\"{a}\"")).collect();
            entry.push_str(&format!("\n    alpn: [{}]", alpn.join(", ")));
        }
    }
}

fn push_network(node: &Node, entry: &mut String) {
    if let Some(ref transport) = node.transport {
        let network = match transport.kind {
            TransportKind::Tcp => "tcp",
            TransportKind::Ws => "ws",
            TransportKind::H2 => "h2",
            TransportKind::Grpc => "grpc",
            TransportKind::Quic => "quic",
            TransportKind::HttpUpgrade => "httpupgrade",
            TransportKind::Kcp => "kcp",
            TransportKind::Xtls => "xtls",
        };
        entry.push_str(&format!("\n    network: {network}"));
        push_transport_opts(transport, entry);
    }
}

fn push_transport_opts(transport: &Transport, entry: &mut String) {
    match transport.kind {
        TransportKind::Ws | TransportKind::HttpUpgrade => {
            if let Some(ref path) = transport.path {
                entry.push_str(&format!("\n    ws-opts:\n      path: \"{path}\""));
            }
        }
        TransportKind::Grpc => {
            if let Some(ref path) = transport.path {
                entry.push_str(&format!(
                    "\n    grpc-opts:\n      grpc-service-name: \"{path}\""
                ));
            }
        }
        TransportKind::H2 => {
            if let Some(ref path) = transport.path {
                entry.push_str(&format!("\n    h2-opts:\n      path: \"{path}\""));
            }
        }
        _ => {}
    }
}
