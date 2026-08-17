//! sing-box JSON container emitter.
//!
//! Emits an `outbounds` array with one entry per compatible node. The full
//! document (route, DNS, inbounds) is assembled in Slice 5.

use deve_sub_domain::{
    Authentication, Node, ProtocolConfig, ProtocolKind, SnellObfsMode, SnellVersion, Transport,
    TransportKind,
};
use serde_json::{Map, Value, json};

use crate::error::EmitError;

type EmitResult = Result<(&'static str, Vec<(String, Value)>), EmitError>;

pub fn emit(nodes: &[Node]) -> Result<String, EmitError> {
    // WHY: ShadowTLS in sing-box expands to two outbounds — a `shadowtls`
    // outbound and an inner protocol outbound with `detour` pointing to it.
    // All other protocols produce exactly one outbound. Flatten the
    // per-node `Vec<Value>` results into a single `outbounds` array.
    let mut outbounds: Vec<Value> = Vec::new();
    for node in nodes {
        if node.protocol == ProtocolKind::ShadowTls {
            let (stls, inner) = shadowtls_pair(node)?;
            outbounds.push(stls);
            outbounds.push(inner);
        } else {
            outbounds.push(emit_outbound(node)?);
        }
    }
    let doc = json!({
        "outbounds": outbounds,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| EmitError::Encode(e.to_string()))
}

fn emit_outbound(node: &Node) -> Result<Value, EmitError> {
    let server = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let tag = node.display_name.clone();

    let (type_, fields) = match node.protocol {
        ProtocolKind::Trojan => trojan(node)?,
        ProtocolKind::Shadowsocks => shadowsocks(node)?,
        ProtocolKind::VMess => vmess(node)?,
        ProtocolKind::Vless => vless(node)?,
        ProtocolKind::Hysteria2 => hysteria2(node)?,
        ProtocolKind::TuicV5 => tuic_v5(node)?,
        ProtocolKind::WireGuard => wireguard(node)?,
        ProtocolKind::AnyTls => anytls(node)?,
        ProtocolKind::Snell => snell(node)?,
        ref other => {
            return Err(EmitError::NoEmitter(format!(
                "singbox: unsupported protocol {other}"
            )));
        }
    };

    let mut obj = Map::new();
    obj.insert("type".to_owned(), Value::String(type_.to_owned()));
    obj.insert("tag".to_owned(), Value::String(tag));
    obj.insert("server".to_owned(), Value::String(server));
    obj.insert("server_port".to_owned(), json!(port));
    for (k, v) in fields {
        obj.insert(k, v);
    }
    Ok(Value::Object(obj))
}

fn push_tls_fields(fields: &mut Vec<(String, Value)>, node: &Node) {
    let Some(tls) = &node.tls else { return };
    let mut tls_obj = Map::new();
    // WHY: Reality requires tls.enabled=true even if the source did not set
    // it explicitly. The sing-box parser mirrors this (extract_tls forces
    // enabled=true whenever TLS fields are present).
    if tls.enabled || tls.reality.is_some() {
        tls_obj.insert("enabled".to_owned(), Value::Bool(true));
    }
    if let Some(ref sni) = tls.server_name {
        tls_obj.insert("server_name".to_owned(), Value::String(sni.clone()));
    }
    if let Some(true) = tls.skip_cert_verify {
        tls_obj.insert("insecure".to_owned(), Value::Bool(true));
    }
    if !tls.alpn.is_empty() {
        tls_obj.insert(
            "alpn".to_owned(),
            Value::Array(tls.alpn.iter().map(|a| Value::String(a.clone())).collect()),
        );
    }
    if let Some(ref fp) = tls.client_fingerprint {
        tls_obj.insert(
            "utls".to_owned(),
            json!({ "enabled": true, "fingerprint": fp }),
        );
    }
    if let Some(ref reality) = tls.reality {
        let mut reality_obj = Map::new();
        reality_obj.insert("enabled".to_owned(), Value::Bool(true));
        if !reality.public_key.is_empty() {
            reality_obj.insert(
                "public_key".to_owned(),
                Value::String(reality.public_key.clone()),
            );
        }
        if !reality.short_id.is_empty() {
            reality_obj.insert(
                "short_id".to_owned(),
                Value::String(reality.short_id.clone()),
            );
        }
        tls_obj.insert("reality".to_owned(), Value::Object(reality_obj));
    }
    if !tls_obj.is_empty() {
        fields.push(("tls".to_owned(), Value::Object(tls_obj)));
    }
}

fn push_transport_fields(fields: &mut Vec<(String, Value)>, node: &Node) {
    if let Some(ref transport) = node.transport
        && let Some(v) = singbox_transport(transport)
    {
        fields.push(("transport".to_owned(), v));
    }
}

fn trojan(node: &Node) -> EmitResult {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("trojan password")),
    };
    let mut fields = vec![("password".to_owned(), Value::String(password))];
    push_tls_fields(&mut fields, node);
    push_transport_fields(&mut fields, node);
    Ok(("trojan", fields))
}

fn shadowsocks(node: &Node) -> EmitResult {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("ss password")),
    };
    let method = match &node.config {
        ProtocolConfig::Shadowsocks(cfg) => cfg.method.clone(),
        _ => return Err(EmitError::MissingField("ss method")),
    };
    Ok((
        "shadowsocks",
        vec![
            ("method".to_owned(), Value::String(method)),
            ("password".to_owned(), Value::String(password)),
        ],
    ))
}

fn vmess(node: &Node) -> EmitResult {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid.clone(),
        _ => return Err(EmitError::MissingField("vmess uuid")),
    };
    let mut fields = vec![("uuid".to_owned(), Value::String(uuid))];
    if let ProtocolConfig::VMess(cfg) = &node.config
        && let Some(aid) = cfg.alter_id
    {
        fields.push(("alter_id".to_owned(), json!(aid)));
    }
    push_tls_fields(&mut fields, node);
    push_transport_fields(&mut fields, node);
    Ok(("vmess", fields))
}

fn vless(node: &Node) -> EmitResult {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid.clone(),
        _ => return Err(EmitError::MissingField("vless uuid")),
    };
    let mut fields = vec![("uuid".to_owned(), Value::String(uuid))];
    if let ProtocolConfig::VlessReality(cfg) = &node.config
        && let Some(ref flow) = cfg.flow
    {
        fields.push(("flow".to_owned(), Value::String(flow.clone())));
    }
    push_tls_fields(&mut fields, node);
    push_transport_fields(&mut fields, node);
    Ok(("vless", fields))
}

fn hysteria2(node: &Node) -> EmitResult {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("hysteria2 password")),
    };
    let mut fields = vec![("password".to_owned(), Value::String(password))];
    push_tls_fields(&mut fields, node);
    Ok(("hysteria2", fields))
}

fn tuic_v5(node: &Node) -> EmitResult {
    let (uuid, password) = match &node.authentication {
        Authentication::UuidPassword { uuid, password } => (uuid.clone(), password.clone()),
        _ => return Err(EmitError::MissingField("tuic v5 uuid+password")),
    };
    let mut fields = vec![
        ("uuid".to_owned(), Value::String(uuid)),
        ("password".to_owned(), Value::String(password)),
    ];
    push_tls_fields(&mut fields, node);
    Ok(("tuic", fields))
}

fn wireguard(node: &Node) -> EmitResult {
    let cfg = match &node.config {
        ProtocolConfig::WireGuard(c) => c,
        _ => return Err(EmitError::MissingField("wireguard config")),
    };

    let mut fields: Vec<(String, Value)> = Vec::new();
    fields.push((
        "private_key".to_owned(),
        Value::String(cfg.private_key.clone()),
    ));

    if !cfg.address.is_empty() {
        fields.push((
            "local_address".to_owned(),
            Value::Array(
                cfg.address
                    .iter()
                    .map(|a| Value::String(a.clone()))
                    .collect(),
            ),
        ));
    }
    if let Some(mtu) = cfg.mtu {
        fields.push(("mtu".to_owned(), json!(mtu)));
    }
    if let Some(workers) = cfg.workers {
        fields.push(("workers".to_owned(), json!(workers)));
    }

    let peers: Result<Vec<Value>, EmitError> = cfg
        .peers
        .iter()
        .map(|p| {
            let mut peer = Map::new();
            peer.insert(
                "server".to_owned(),
                Value::String(node.endpoint.host.uri_host()),
            );
            peer.insert("server_port".to_owned(), json!(node.endpoint.port));
            peer.insert("public_key".to_owned(), Value::String(p.public_key.clone()));
            if let Some(ref psk) = p.pre_shared_key {
                peer.insert("pre_shared_key".to_owned(), Value::String(psk.clone()));
            }
            if !p.allowed_ips.is_empty() {
                peer.insert(
                    "allowed_ips".to_owned(),
                    Value::Array(
                        p.allowed_ips
                            .iter()
                            .map(|ip| Value::String(ip.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(reserved) = p.reserved {
                peer.insert(
                    "reserved".to_owned(),
                    Value::Array(
                        reserved
                            .iter()
                            .map(|b| Value::Number((*b).into()))
                            .collect(),
                    ),
                );
            }
            if let Some(ka) = p.persistent_keepalive {
                let secs = ka.whole_seconds();
                if secs >= 0 {
                    peer.insert("persistent_keepalive_interval".to_owned(), json!(secs));
                }
            }
            Ok(Value::Object(peer))
        })
        .collect();
    fields.push(("peers".to_owned(), Value::Array(peers?)));

    Ok(("wireguard", fields))
}

fn anytls(node: &Node) -> EmitResult {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("anytls password")),
    };
    let cfg = match &node.config {
        ProtocolConfig::AnyTls(c) => c,
        _ => return Err(EmitError::MissingField("anytls config")),
    };
    let mut fields = vec![("password".to_owned(), Value::String(password))];
    push_tls_fields(&mut fields, node);
    if let Some(d) = cfg.idle_session_check_interval {
        fields.push((
            "idle_session_check_interval".to_owned(),
            Value::String(format_go_duration(d)),
        ));
    }
    if let Some(d) = cfg.idle_session_timeout {
        fields.push((
            "idle_session_timeout".to_owned(),
            Value::String(format_go_duration(d)),
        ));
    }
    if let Some(n) = cfg.min_idle_session {
        fields.push(("min_idle_session".to_owned(), json!(n)));
    }
    if let Some(ref cm) = cfg.client_metadata {
        fields.push(("client_metadata".to_owned(), Value::String(cm.clone())));
    }
    Ok(("anytls", fields))
}

fn snell(node: &Node) -> EmitResult {
    let psk = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("snell psk")),
    };
    let cfg = match &node.config {
        ProtocolConfig::Snell(c) => c,
        _ => return Err(EmitError::MissingField("snell config")),
    };

    let mut fields: Vec<(String, Value)> = vec![
        ("psk".to_owned(), Value::String(psk)),
        ("version".to_owned(), json!(cfg.version.as_u32())),
    ];

    if let Some(reuse) = cfg.reuse {
        fields.push(("reuse".to_owned(), Value::Bool(reuse)));
    }

    // WHY: sing-box preserves userkey in `extras` (parser stored it under
    // `snell_userkey`); emit it back so sing-box round-trip is lossless.
    if let Some(userkey) = node.extras.get("snell_userkey").and_then(|v| v.as_str()) {
        fields.push(("userkey".to_owned(), Value::String(userkey.to_owned())));
    }

    match cfg.version {
        SnellVersion::V4 => {
            if let Some(ref obfs) = cfg.obfs {
                let mode_str = match obfs.mode {
                    SnellObfsMode::Http => "http",
                    SnellObfsMode::Tls => "tls",
                    SnellObfsMode::ShadowTls | SnellObfsMode::Restls | SnellObfsMode::Jls => {
                        return Err(EmitError::NoEmitter(format!(
                            "singbox: snell obfs mode {:?} not supported by sing-box",
                            obfs.mode
                        )));
                    }
                };
                fields.push(("obfs_mode".to_owned(), Value::String(mode_str.to_owned())));
                if let Some(ref host) = obfs.host {
                    fields.push(("obfs_host".to_owned(), Value::String(host.clone())));
                }
            }
            if matches!(cfg.obfs.as_ref().map(|o| o.mode), Some(SnellObfsMode::Tls)) {
                push_tls_fields(&mut fields, node);
            }
        }
        SnellVersion::V6 => {
            if let Some(mode) = cfg.v6_mode {
                let mode_str = match mode {
                    deve_sub_domain::SnellV6Mode::Default => "default",
                    deve_sub_domain::SnellV6Mode::Unshaped => "unshaped",
                    deve_sub_domain::SnellV6Mode::UnsafeRaw => "unsafe-raw",
                };
                fields.push(("mode".to_owned(), Value::String(mode_str.to_owned())));
            }
        }
        // WHY: sing-box outbound accepts only v4/v6 (option/snell.go:71);
        // other versions must be filtered out by the compatibility layer
        // before reaching this emitter.
        other => {
            return Err(EmitError::NoEmitter(format!(
                "singbox: snell v{} not supported (only v4/v6)",
                other.as_u32()
            )));
        }
    }

    Ok(("snell", fields))
}

fn format_go_duration(d: time::Duration) -> String {
    let total_ms = d.whole_milliseconds();
    if total_ms == 0 {
        return "0s".to_owned();
    }
    let abs_ms = total_ms.unsigned_abs() as i64;
    let hours = abs_ms / 3_600_000;
    let mins = (abs_ms % 3_600_000) / 60_000;
    let secs = (abs_ms % 60_000) / 1_000;
    let ms = abs_ms % 1_000;
    let mut out = String::new();
    if total_ms < 0 {
        out.push('-');
    }
    if hours > 0 {
        out.push_str(&format!("{hours}h"));
    }
    if mins > 0 {
        out.push_str(&format!("{mins}m"));
    }
    if secs > 0 || (hours == 0 && mins == 0 && ms == 0) {
        out.push_str(&format!("{secs}s"));
    }
    if ms > 0 {
        out.push_str(&format!("{ms}ms"));
    }
    out
}

fn singbox_transport(transport: &Transport) -> Option<Value> {
    let type_ = match transport.kind {
        TransportKind::Ws => "ws",
        TransportKind::H2 => "http",
        TransportKind::Grpc => "grpc",
        TransportKind::Quic => "quic",
        TransportKind::HttpUpgrade => "httpupgrade",
        TransportKind::Tcp | TransportKind::Kcp | TransportKind::Xtls | TransportKind::Xhttp => {
            return None;
        }
    };
    let mut obj = Map::new();
    obj.insert("type".to_owned(), Value::String(type_.to_owned()));
    if let Some(ref path) = transport.path {
        obj.insert("path".to_owned(), Value::String(path.clone()));
    }
    if let Some(ref host) = transport.host {
        match transport.kind {
            TransportKind::Ws | TransportKind::HttpUpgrade => {
                obj.insert("headers".to_owned(), json!({ "Host": host }));
            }
            TransportKind::H2 => {
                obj.insert("host".to_owned(), Value::String(host.clone()));
            }
            _ => {}
        }
    }
    Some(Value::Object(obj))
}

/// Emit a ShadowTLS node as a pair of sing-box outbounds: the `shadowtls`
/// wrapper outbound and the inner protocol outbound with `detour`.
///
/// Returns `(shadowtls_outbound, inner_outbound)`.
fn shadowtls_pair(node: &Node) -> Result<(Value, Value), EmitError> {
    let cfg = match &node.config {
        ProtocolConfig::ShadowTls(c) => c,
        _ => return Err(EmitError::MissingField("shadowtls config")),
    };

    let server = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let tag = node.display_name.clone();
    // WHY: derive a stable shadowtls tag from the node tag to keep
    // round-trip deterministic. The parser matches by `detour` reference,
    // so the exact suffix doesn't matter as long as it's unique within the
    // document. Suffixing avoids collisions with the inner outbound tag.
    let shadowtls_tag = format!("{tag}-stls");

    // --- shadowtls outbound ---
    let mut stls_obj = Map::new();
    stls_obj.insert("type".to_owned(), Value::String("shadowtls".to_owned()));
    stls_obj.insert("tag".to_owned(), Value::String(shadowtls_tag.clone()));
    stls_obj.insert("server".to_owned(), Value::String(server));
    stls_obj.insert("server_port".to_owned(), json!(port));
    stls_obj.insert("version".to_owned(), json!(cfg.version.as_u32()));
    if let Some(ref pw) = cfg.password {
        stls_obj.insert("password".to_owned(), Value::String(pw.clone()));
    }
    // Camouflage TLS fields from node.tls.
    let mut stls_fields: Vec<(String, Value)> = Vec::new();
    push_tls_fields(&mut stls_fields, node);
    for (k, v) in stls_fields {
        stls_obj.insert(k, v);
    }

    // --- inner protocol outbound ---
    // WHY: build a synthetic inner Node that carries only the inner
    // protocol config + authentication, then delegate to `emit_outbound`.
    // The inner Node's endpoint/TLS are irrelevant (sing-box routes through
    // the detour), so we reuse the outer endpoint to satisfy the emitter's
    // server/server_port fields. The `detour` field is injected after.
    let inner_node = synthesize_inner_node(node, cfg);
    let mut inner_obj = match emit_outbound_map(&inner_node)? {
        Value::Object(m) => m,
        _ => return Err(EmitError::Encode("inner outbound not an object".to_owned())),
    };
    inner_obj.insert("detour".to_owned(), Value::String(shadowtls_tag));

    Ok((Value::Object(stls_obj), Value::Object(inner_obj)))
}

/// Build a synthetic inner Node from a ShadowTLS node's boxed inner config.
/// The inner Node carries the inner protocol, config, and authentication;
/// endpoint and TLS are placeholder (sing-box routes via detour).
fn synthesize_inner_node(outer: &Node, cfg: &deve_sub_domain::ShadowTlsConfig) -> Node {
    let mut inner = outer.clone();
    inner.protocol = cfg.inner_protocol.clone();
    inner.config = (*cfg.inner_config).clone();
    // WHY: inner endpoint is vestigial in sing-box detour mode, but the
    // emitter requires valid server/server_port. Reuse outer endpoint so
    // the emitted JSON is well-formed without inventing dummy data.
    inner.display_name = outer.display_name.clone();
    // Inner TLS is vestigial (shadowtls handles the real TLS handshake).
    inner.tls = None;
    inner
}

/// Emit a single outbound as a JSON object (shared logic for `emit_outbound`
/// and `shadowtls_pair`'s inner node).
fn emit_outbound_map(node: &Node) -> Result<Value, EmitError> {
    emit_outbound(node)
}
