//! Mihomo (Clash Meta) YAML container emitter.
//!
//! Emits a `proxies:` array with one entry per compatible node. Each entry
//! follows the Mihomo proxy schema. The full template (proxy-groups, rules,
//! dns) is assembled in Slice 5.

use deve_sub_domain::{
    Authentication, GroupType, Node, ProtocolConfig, ProtocolKind, SnellObfsMode, Transport,
    TransportKind, XhttpMode,
};

use crate::container::ir::AssembledTemplate;
use crate::error::EmitError;

pub fn emit(nodes: &[Node]) -> Result<String, EmitError> {
    let mut out = String::with_capacity(nodes.len() * 256 + 16);
    out.push_str("proxies:");
    for node in nodes {
        emit_proxy(node, &mut out)?;
    }
    Ok(out)
}

pub fn emit_full(template: &AssembledTemplate) -> Result<String, EmitError> {
    let mut out = String::with_capacity(template.nodes.len() * 256 + 1024);
    out.push_str("proxies:");
    for node in &template.nodes {
        emit_proxy(node, &mut out)?;
    }

    if !template.groups.is_empty() {
        out.push('\n');
        emit_groups(&template.groups, &mut out)?;
    }

    if !template.rules.is_empty() {
        out.push('\n');
        emit_rules(&template.rules, &mut out)?;
    }

    if !template.dns.is_null() {
        out.push('\n');
        emit_json_block("dns", &template.dns, &mut out)?;
    }

    if !template.tun.is_null() {
        out.push('\n');
        emit_json_block("tun", &template.tun, &mut out)?;
    }

    Ok(out)
}

/// Escape a string for a YAML double-quoted scalar.
///
/// WHY: mihomo proxy fields (name, password, sni, path, host) are
/// user-controlled and may contain `"`, `\`, or control characters. Emitting
/// them raw into a double-quoted YAML scalar produces invalid YAML or allows
/// field injection. This helper escapes per YAML 1.1 double-quoted rules.
fn yaml_dq(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{ch:04X}", ch = c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn emit_groups(
    groups: &[crate::container::ir::AssembledGroup],
    out: &mut String,
) -> Result<(), EmitError> {
    out.push_str("proxy-groups:");
    for g in groups {
        let type_str = match g.group_type {
            GroupType::Select => "select",
            GroupType::UrlTest => "url-test",
            GroupType::Fallback => "fallback",
            GroupType::LoadBalance => "load-balance",
            GroupType::Relay => "relay",
            GroupType::Direct => "direct",
            GroupType::Reject => "reject",
        };
        out.push_str("\n  - name: ");
        out.push_str(&yaml_dq(&g.name));
        out.push_str("\n    type: ");
        out.push_str(type_str);
        if !g.members.is_empty() {
            out.push_str("\n    proxies:");
            for m in &g.members {
                out.push_str("\n      - ");
                out.push_str(&yaml_dq(m));
            }
        }
    }
    Ok(())
}

fn emit_rules(rules: &[serde_json::Value], out: &mut String) -> Result<(), EmitError> {
    out.push_str("rules:");
    for rule in rules {
        let yaml = json_to_yaml_line(rule)?;
        for (i, l) in yaml.lines().enumerate() {
            if i == 0 {
                out.push_str("\n  - ");
                out.push_str(l);
            } else {
                out.push_str("\n    ");
                out.push_str(l);
            }
        }
    }
    Ok(())
}

fn emit_json_block(
    key: &str,
    value: &serde_json::Value,
    out: &mut String,
) -> Result<(), EmitError> {
    let yaml =
        serde_yaml::to_string(value).map_err(|e| EmitError::Encode(format!("serde_yaml: {e}")))?;
    // WHY: serde_yaml emits a leading "---\n" document marker and column-0
    // content; we strip the marker and re-indent under the section key.
    let body = yaml
        .strip_prefix("---\n")
        .unwrap_or(&yaml)
        .trim_end_matches('\n');
    out.push_str(key);
    out.push(':');
    for l in body.lines() {
        out.push_str("\n  ");
        out.push_str(l);
    }
    Ok(())
}

fn json_to_yaml_line(value: &serde_json::Value) -> Result<String, EmitError> {
    let yaml =
        serde_yaml::to_string(value).map_err(|e| EmitError::Encode(format!("serde_yaml: {e}")))?;
    Ok(yaml
        .strip_prefix("---\n")
        .unwrap_or(&yaml)
        .trim_end()
        .to_owned())
}

fn emit_proxy(node: &Node, out: &mut String) -> Result<(), EmitError> {
    let server = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let name = &node.display_name;

    match node.protocol {
        ProtocolKind::Trojan => emit_trojan(node, &server, port, name, out),
        ProtocolKind::Shadowsocks => emit_ss(node, &server, port, name, out),
        ProtocolKind::VMess => emit_vmess(node, &server, port, name, out),
        ProtocolKind::Vless => emit_vless(node, &server, port, name, out),
        ProtocolKind::Hysteria2 => emit_hysteria2(node, &server, port, name, out),
        ProtocolKind::TuicV5 => emit_tuic_v5(node, &server, port, name, out),
        ProtocolKind::WireGuard => emit_wireguard(node, &server, port, name, out),
        ProtocolKind::AnyTls => emit_anytls(node, &server, port, name, out),
        ProtocolKind::Snell => emit_snell(node, &server, port, name, out),
        ProtocolKind::ShadowTls => emit_shadowtls(node, &server, port, name, out),
        ref other => {
            return Err(EmitError::NoEmitter(format!(
                "mihomo: unsupported protocol {other}"
            )));
        }
    }?;
    Ok(())
}

fn yaml_entry(name: &str, ptype: &str, server: &str, port: u16) -> String {
    format!(
        "  - name: {}\n    type: {ptype}\n    server: {}\n    port: {port}",
        yaml_dq(name),
        yaml_dq(server),
    )
}

fn emit_trojan(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
) -> Result<(), EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("trojan password")),
    };
    let mut entry = yaml_entry(name, "trojan", server, port);
    entry.push_str(&format!("\n    password: {}", yaml_dq(password)));
    push_tls(node, &mut entry);
    push_network(node, &mut entry);
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_ss(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
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
        "{}\n    cipher: {method}\n    password: {}",
        yaml_entry(name, "ss", server, port),
        yaml_dq(password),
    );
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_vmess(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
) -> Result<(), EmitError> {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(EmitError::MissingField("vmess uuid")),
    };
    let mut entry = yaml_entry(name, "vmess", server, port);
    entry.push_str(&format!("\n    uuid: {}", yaml_dq(uuid)));
    if let ProtocolConfig::VMess(cfg) = &node.config {
        if let Some(aid) = cfg.alter_id {
            entry.push_str(&format!("\n    alterId: {aid}"));
        }
        // WHY: mihomo requires `cipher` on every vmess proxy; default to
        // "auto" when the source did not specify a security method (same
        // default the xray emitter uses). Caught by OUT-001.
        let cipher = cfg.security.clone().unwrap_or_else(|| "auto".to_owned());
        entry.push_str(&format!("\n    cipher: {cipher}"));
    }
    push_tls(node, &mut entry);
    push_network(node, &mut entry);
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_vless(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
) -> Result<(), EmitError> {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid,
        _ => return Err(EmitError::MissingField("vless uuid")),
    };
    let mut entry = yaml_entry(name, "vless", server, port);
    entry.push_str(&format!("\n    uuid: {}", yaml_dq(uuid)));
    if let ProtocolConfig::VlessReality(cfg) = &node.config
        && let Some(ref flow) = cfg.flow
    {
        entry.push_str(&format!("\n    flow: {flow}"));
    }
    push_tls(node, &mut entry);
    push_network(node, &mut entry);
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_hysteria2(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
) -> Result<(), EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("hysteria2 password")),
    };
    let mut entry = yaml_entry(name, "hysteria2", server, port);
    entry.push_str(&format!("\n    password: {}", yaml_dq(password)));
    push_tls(node, &mut entry);
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_tuic_v5(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
) -> Result<(), EmitError> {
    let (uuid, password) = match &node.authentication {
        Authentication::UuidPassword { uuid, password } => (uuid, password),
        _ => return Err(EmitError::MissingField("tuic v5 uuid+password")),
    };
    let mut entry = yaml_entry(name, "tuic", server, port);
    entry.push_str(&format!("\n    uuid: {}", yaml_dq(uuid)));
    entry.push_str(&format!("\n    password: {}", yaml_dq(password)));
    push_tls(node, &mut entry);
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_wireguard(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
) -> Result<(), EmitError> {
    let cfg = match &node.config {
        ProtocolConfig::WireGuard(c) => c,
        _ => return Err(EmitError::MissingField("wireguard config")),
    };
    let mut entry = yaml_entry(name, "wireguard", server, port);
    entry.push_str(&format!("\n    private-key: {}", yaml_dq(&cfg.private_key)));

    // Mihomo splits WireGuard interface addresses into `ip` (IPv4) and
    // `ipv6` (IPv6), each a single string. Emit by simple colon heuristic.
    if !cfg.address.is_empty() {
        let v4 = cfg.address.iter().find(|a| !a.contains(':'));
        let v6 = cfg.address.iter().find(|a| a.contains(':'));
        if let Some(ip) = v4 {
            entry.push_str(&format!("\n    ip: {}", yaml_dq(ip)));
        }
        if let Some(ipv6) = v6 {
            entry.push_str(&format!("\n    ipv6: {}", yaml_dq(ipv6)));
        }
    }
    if let Some(mtu) = cfg.mtu {
        entry.push_str(&format!("\n    mtu: {mtu}"));
    }
    if let Some(workers) = cfg.workers {
        entry.push_str(&format!("\n    workers: {workers}"));
    }
    if !cfg.dns.is_empty() {
        let dns: Vec<String> = cfg.dns.iter().map(|d| yaml_dq(d)).collect();
        entry.push_str(&format!("\n    dns: [{}]", dns.join(", ")));
    }

    if let Some(peer) = cfg.peers.first() {
        entry.push_str("\n    peers:");
        entry.push_str(&format!("\n      - server: {}", yaml_dq(server)));
        entry.push_str(&format!("\n        port: {port}"));
        entry.push_str(&format!(
            "\n        public-key: {}",
            yaml_dq(&peer.public_key)
        ));
        if let Some(ref psk) = peer.pre_shared_key {
            entry.push_str(&format!("\n        pre-shared-key: {}", yaml_dq(psk)));
        }
        if !peer.allowed_ips.is_empty() {
            let ips: Vec<String> = peer.allowed_ips.iter().map(|ip| yaml_dq(ip)).collect();
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

    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_anytls(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
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
    entry.push_str(&format!("\n    password: {}", yaml_dq(password)));
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
    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn emit_snell(
    node: &Node,
    server: &str,
    port: u16,
    name: &str,
    out: &mut String,
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
    entry.push_str(&format!("\n    psk: {}", yaml_dq(psk)));
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
            entry.push_str(&format!("\n      host: {}", yaml_dq(host)));
        }
        if let Some(ref pw) = obfs.password {
            entry.push_str(&format!("\n      password: {}", yaml_dq(pw)));
        }
        if let Some(v) = obfs.version {
            entry.push_str(&format!("\n      version: {v}"));
        }
        if !obfs.alpn.is_empty() {
            let alpn: Vec<String> = obfs.alpn.iter().map(|a| yaml_dq(a)).collect();
            entry.push_str(&format!("\n      alpn: [{}]", alpn.join(", ")));
        }
    }
    // WHY: when obfs.mode=tls the TLS-shaped fields live on `node.tls`; emit
    // them at top-level (mihomo Snell reads sni/alpn/skip-cert-verify there).
    if matches!(cfg.obfs.as_ref().map(|o| o.mode), Some(SnellObfsMode::Tls)) {
        push_tls(node, &mut entry);
    }
    out.push('\n');
    out.push_str(&entry);
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
    out: &mut String,
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
    // the ShadowTLS obfs fields before pushing to `out`.
    let mut inner_buf = String::new();
    emit_proxy(&inner_node, &mut inner_buf)?;
    // WHY: emit_proxy pushes "\n<entry>"; strip the leading newline to get
    // the raw entry, append shadowtls fields, then re-push with separator.
    let inner_entry = inner_buf.strip_prefix('\n').unwrap_or(&inner_buf);
    if inner_entry.is_empty() {
        return Err(EmitError::NoEmitter(
            "shadowtls: inner emitter produced no output".to_owned(),
        ));
    }

    let mut entry = inner_entry.to_owned();

    match cfg.inner_protocol {
        ProtocolKind::Shadowsocks => {
            // ss uses `plugin: shadow-tls` + `plugin-opts`.
            entry.push_str("\n    plugin: shadow-tls");
            entry.push_str("\n    plugin-opts:");
            entry.push_str(&format!("\n      version: {}", cfg.version.as_u32()));
            if let Some(ref pw) = cfg.password {
                entry.push_str(&format!("\n      password: {}", yaml_dq(pw)));
            }
            if let Some(ref tls) = node.tls
                && let Some(ref sni) = tls.server_name
            {
                entry.push_str(&format!("\n      host: {}", yaml_dq(sni)));
            }
        }
        ProtocolKind::Snell => {
            // snell uses `obfs-opts.mode: shadow-tls`.
            entry.push_str("\n    obfs-opts:");
            entry.push_str("\n      mode: shadow-tls");
            entry.push_str(&format!("\n      version: {}", cfg.version.as_u32()));
            if let Some(ref pw) = cfg.password {
                entry.push_str(&format!("\n      password: {}", yaml_dq(pw)));
            }
            if let Some(ref tls) = node.tls
                && let Some(ref sni) = tls.server_name
            {
                entry.push_str(&format!("\n      host: {}", yaml_dq(sni)));
            }
        }
        ProtocolKind::Vless | ProtocolKind::Trojan | ProtocolKind::VMess | ProtocolKind::AnyTls => {
            // vless/trojan/vmess/anytls use `shadow-tls-opts`.
            entry.push_str("\n    shadow-tls-opts:");
            entry.push_str(&format!("\n      version: {}", cfg.version.as_u32()));
            if let Some(ref pw) = cfg.password {
                entry.push_str(&format!("\n      password: {}", yaml_dq(pw)));
            }
            if let Some(ref tls) = node.tls
                && let Some(ref sni) = tls.server_name
            {
                entry.push_str(&format!("\n      sni: {}", yaml_dq(sni)));
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

    out.push('\n');
    out.push_str(&entry);
    Ok(())
}

fn push_tls(node: &Node, entry: &mut String) {
    if let Some(ref tls) = node.tls {
        if tls.enabled {
            entry.push_str("\n    tls: true");
        }
        if let Some(ref sni) = tls.server_name {
            entry.push_str(&format!("\n    sni: {}", yaml_dq(sni)));
        }
        if let Some(skip) = tls.skip_cert_verify {
            entry.push_str(&format!("\n    skip-cert-verify: {skip}"));
        }
        if !tls.alpn.is_empty() {
            let alpn: Vec<String> = tls.alpn.iter().map(|a| yaml_dq(a)).collect();
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
            TransportKind::Xhttp => "xhttp",
        };
        entry.push_str(&format!("\n    network: {network}"));
        push_transport_opts(transport, entry);
    }
}

fn push_transport_opts(transport: &Transport, entry: &mut String) {
    match transport.kind {
        TransportKind::Ws | TransportKind::HttpUpgrade => {
            if let Some(ref path) = transport.path {
                entry.push_str(&format!("\n    ws-opts:\n      path: {}", yaml_dq(path)));
            }
        }
        TransportKind::Grpc => {
            if let Some(ref path) = transport.path {
                entry.push_str(&format!(
                    "\n    grpc-opts:\n      grpc-service-name: {}",
                    yaml_dq(path)
                ));
            }
        }
        TransportKind::H2 => {
            if let Some(ref path) = transport.path {
                entry.push_str(&format!("\n    h2-opts:\n      path: {}", yaml_dq(path)));
            }
        }
        TransportKind::Xhttp => {
            let has_path = transport.path.is_some();
            let has_host = transport.host.is_some();
            let mode = transport.xhttp_mode.unwrap_or_default();
            let has_non_default_mode = mode != XhttpMode::Auto;
            if has_path || has_host || has_non_default_mode {
                entry.push_str("\n    xhttp-opts:");
                if let Some(ref path) = transport.path {
                    entry.push_str(&format!("\n      path: {}", yaml_dq(path)));
                }
                if let Some(ref host) = transport.host {
                    entry.push_str(&format!("\n      host: {}", yaml_dq(host)));
                }
                if has_non_default_mode {
                    entry.push_str(&format!("\n      mode: {}", mode.as_str()));
                }
            }
        }
        _ => {}
    }
}
