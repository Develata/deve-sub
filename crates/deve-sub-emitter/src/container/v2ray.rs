//! V2Ray JSON container emitter (shared with Xray).
//!
//! Emits an `outbounds` array with one entry per compatible node. The full
//! document (routing, DNS, inbounds) is assembled in Slice 5.

use deve_sub_domain::{
    Authentication, Node, ProtocolConfig, ProtocolKind, Transport, TransportKind,
};
use serde_json::{Map, Value, json};

use crate::error::EmitError;

pub fn emit(nodes: &[Node]) -> Result<String, EmitError> {
    let outbounds: Result<Vec<Value>, EmitError> = nodes.iter().map(emit_outbound).collect();
    let doc = json!({
        "outbounds": outbounds?,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| EmitError::Encode(e.to_string()))
}

fn emit_outbound(node: &Node) -> Result<Value, EmitError> {
    let server = node.endpoint.host.uri_host();
    let port = node.endpoint.port;
    let tag = node.display_name.clone();

    if node.protocol == ProtocolKind::WireGuard {
        return emit_wireguard(node, tag);
    }

    let (protocol, settings, stream) = match node.protocol {
        ProtocolKind::Trojan => trojan(node, &server, port)?,
        ProtocolKind::Shadowsocks => shadowsocks(node, &server, port)?,
        ProtocolKind::VMess => vmess(node, &server, port)?,
        ProtocolKind::Vless => vless(node, &server, port)?,
        ref other => {
            return Err(EmitError::NoEmitter(format!(
                "v2ray: unsupported protocol {other}"
            )));
        }
    };

    let mut obj = Map::new();
    obj.insert("tag".to_owned(), Value::String(tag));
    obj.insert("protocol".to_owned(), Value::String(protocol.to_owned()));
    obj.insert("settings".to_owned(), settings);
    if let Some(s) = stream {
        obj.insert("streamSettings".to_owned(), s);
    }
    Ok(Value::Object(obj))
}

fn emit_wireguard(node: &Node, tag: String) -> Result<Value, EmitError> {
    let cfg = match &node.config {
        ProtocolConfig::WireGuard(c) => c,
        _ => return Err(EmitError::MissingField("wireguard config")),
    };

    let server = node.endpoint.host.uri_host();
    let port = node.endpoint.port;

    let peers: Vec<Value> = cfg
        .peers
        .iter()
        .map(|p| {
            let mut peer = Map::new();
            peer.insert("publicKey".to_owned(), Value::String(p.public_key.clone()));
            peer.insert(
                "endpoint".to_owned(),
                Value::String(format!("{server}:{port}")),
            );
            if let Some(ref psk) = p.pre_shared_key {
                peer.insert("preSharedKey".to_owned(), Value::String(psk.clone()));
            }
            if !p.allowed_ips.is_empty() {
                peer.insert(
                    "allowedIPs".to_owned(),
                    Value::Array(
                        p.allowed_ips
                            .iter()
                            .map(|ip| Value::String(ip.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(ka) = p.persistent_keepalive {
                let secs = ka.whole_seconds();
                if secs >= 0 {
                    peer.insert("keepAlive".to_owned(), json!(secs));
                }
            }
            Value::Object(peer)
        })
        .collect();

    let mut settings = Map::new();
    settings.insert(
        "secretKey".to_owned(),
        Value::String(cfg.private_key.clone()),
    );
    if !cfg.address.is_empty() {
        settings.insert(
            "address".to_owned(),
            Value::Array(
                cfg.address
                    .iter()
                    .map(|a| Value::String(a.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(mtu) = cfg.mtu {
        settings.insert("mtu".to_owned(), json!(mtu));
    }
    if let Some(reserved) = cfg.peers.first().and_then(|p| p.reserved) {
        settings.insert(
            "reserved".to_owned(),
            Value::Array(
                reserved
                    .iter()
                    .map(|b| Value::Number((*b).into()))
                    .collect(),
            ),
        );
    }
    settings.insert("peers".to_owned(), Value::Array(peers));

    let mut obj = Map::new();
    obj.insert("tag".to_owned(), Value::String(tag));
    obj.insert("protocol".to_owned(), Value::String("wireguard".to_owned()));
    obj.insert("settings".to_owned(), Value::Object(settings));
    Ok(Value::Object(obj))
}

// WHY: Xray-core uses different `settings` shapes per protocol: trojan and
// shadowsocks store credentials directly on `servers[0]`, while vmess and
// vless nest users under `vnext[0].users[0]`. Each per-protocol builder
// returns the complete `settings` object so `emit_outbound` does not need
// protocol-specific knowledge. Verified against Xray-core `infra/conf/*.go`
// at commit 7d214f8 (constraint #18: official format).
type Emit = Result<(&'static str, Value, Option<Value>), EmitError>;

fn trojan(node: &Node, server: &str, port: u16) -> Emit {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("trojan password")),
    };
    let settings = json!({
        "servers": [{
            "address": server,
            "port": port,
            "password": password,
        }]
    });
    let stream = stream_settings(node);
    Ok(("trojan", settings, stream))
}

fn shadowsocks(node: &Node, server: &str, port: u16) -> Emit {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("ss password")),
    };
    let method = match &node.config {
        ProtocolConfig::Shadowsocks(cfg) => cfg.method.clone(),
        _ => return Err(EmitError::MissingField("ss method")),
    };
    let settings = json!({
        "servers": [{
            "address": server,
            "port": port,
            "method": method,
            "password": password,
        }]
    });
    Ok(("shadowsocks", settings, None))
}

fn vmess(node: &Node, server: &str, port: u16) -> Emit {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid.clone(),
        _ => return Err(EmitError::MissingField("vmess uuid")),
    };
    let alter_id = match &node.config {
        ProtocolConfig::VMess(cfg) => cfg.alter_id.unwrap_or(0),
        _ => 0,
    };
    let security = match &node.config {
        ProtocolConfig::VMess(cfg) => cfg.security.clone().unwrap_or_else(|| "auto".to_owned()),
        _ => "auto".to_owned(),
    };
    let settings = json!({
        "vnext": [{
            "address": server,
            "port": port,
            "users": [{
                "id": uuid,
                "alterId": alter_id,
                "security": security,
            }]
        }]
    });
    let stream = stream_settings(node);
    Ok(("vmess", settings, stream))
}

fn vless(node: &Node, server: &str, port: u16) -> Emit {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid.clone(),
        _ => return Err(EmitError::MissingField("vless uuid")),
    };
    let mut user = Map::new();
    user.insert("id".to_owned(), Value::String(uuid));
    // WHY: Xray requires `encryption: "none"` on every VLESS outbound user;
    // `Build()` rejects the config otherwise (infra/conf/vless.go L370-L374).
    user.insert("encryption".to_owned(), Value::String("none".to_owned()));
    if let ProtocolConfig::VlessReality(cfg) = &node.config
        && let Some(ref flow) = cfg.flow
    {
        user.insert("flow".to_owned(), Value::String(flow.clone()));
    }
    let settings = json!({
        "vnext": [{
            "address": server,
            "port": port,
            "users": [Value::Object(user)],
        }]
    });
    let stream = stream_settings(node);
    Ok(("vless", settings, stream))
}

fn stream_settings(node: &Node) -> Option<Value> {
    let transport = node.transport.as_ref()?;
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
    let mut stream = Map::new();
    stream.insert("network".to_owned(), Value::String(network.to_owned()));
    if let Some(ref tls) = node.tls {
        if let Some(ref reality) = tls.reality {
            stream.insert("security".to_owned(), Value::String("reality".to_owned()));
            let mut rs = Map::new();
            if let Some(ref sni) = tls.server_name {
                rs.insert("serverName".to_owned(), Value::String(sni.clone()));
            }
            if !reality.public_key.is_empty() {
                rs.insert(
                    "publicKey".to_owned(),
                    Value::String(reality.public_key.clone()),
                );
            }
            if !reality.short_id.is_empty() {
                rs.insert(
                    "shortId".to_owned(),
                    Value::String(reality.short_id.clone()),
                );
            }
            if let Some(ref fp) = tls.client_fingerprint {
                rs.insert("fingerprint".to_owned(), Value::String(fp.clone()));
            }
            stream.insert("realitySettings".to_owned(), Value::Object(rs));
        } else if tls.enabled {
            stream.insert("security".to_owned(), Value::String("tls".to_owned()));
            let mut ts = Map::new();
            if let Some(ref sni) = tls.server_name {
                ts.insert("serverName".to_owned(), Value::String(sni.clone()));
            }
            if let Some(skip) = tls.skip_cert_verify {
                ts.insert("allowInsecure".to_owned(), Value::Bool(skip));
            }
            if !tls.alpn.is_empty() {
                ts.insert(
                    "alpn".to_owned(),
                    Value::Array(tls.alpn.iter().map(|a| Value::String(a.clone())).collect()),
                );
            }
            if let Some(ref fp) = tls.client_fingerprint {
                ts.insert("fingerprint".to_owned(), Value::String(fp.clone()));
            }
            if !ts.is_empty() {
                stream.insert("tlsSettings".to_owned(), Value::Object(ts));
            }
        } else {
            stream.insert("security".to_owned(), Value::String("none".to_owned()));
        }
    } else {
        stream.insert("security".to_owned(), Value::String("none".to_owned()));
    }
    if let Some(opts) = transport_settings(transport) {
        let key = format!("{network}Settings");
        stream.insert(key, opts);
    }
    Some(Value::Object(stream))
}

fn transport_settings(transport: &Transport) -> Option<Value> {
    match transport.kind {
        TransportKind::Ws | TransportKind::HttpUpgrade => {
            let mut obj = Map::new();
            if let Some(ref path) = transport.path {
                obj.insert("path".to_owned(), Value::String(path.clone()));
            }
            if let Some(ref host) = transport.host {
                obj.insert("headers".to_owned(), json!({ "Host": host }));
            }
            Some(Value::Object(obj))
        }
        TransportKind::Grpc => {
            let mut obj = Map::new();
            if let Some(ref path) = transport.path {
                obj.insert("serviceName".to_owned(), Value::String(path.clone()));
            }
            Some(Value::Object(obj))
        }
        TransportKind::H2 => {
            let mut obj = Map::new();
            if let Some(ref path) = transport.path {
                obj.insert("path".to_owned(), Value::String(path.clone()));
            }
            if let Some(ref host) = transport.host {
                obj.insert("host".to_owned(), json!([host]));
            }
            Some(Value::Object(obj))
        }
        TransportKind::Xhttp => {
            let mut obj = Map::new();
            if let Some(ref path) = transport.path {
                obj.insert("path".to_owned(), Value::String(path.clone()));
            }
            if let Some(ref host) = transport.host {
                obj.insert("host".to_owned(), Value::String(host.clone()));
            }
            let mode = transport.xhttp_mode.unwrap_or_default();
            if mode != deve_sub_domain::XhttpMode::Auto {
                obj.insert("mode".to_owned(), Value::String(mode.as_str().to_owned()));
            }
            Some(Value::Object(obj))
        }
        _ => None,
    }
}
