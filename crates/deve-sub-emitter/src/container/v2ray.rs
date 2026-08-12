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
        ProtocolKind::Trojan => trojan(node)?,
        ProtocolKind::Shadowsocks => shadowsocks(node)?,
        ProtocolKind::VMess => vmess(node)?,
        ProtocolKind::Vless => vless(node)?,
        ref other => {
            return Err(EmitError::NoEmitter(format!(
                "v2ray: unsupported protocol {other}"
            )));
        }
    };

    let mut obj = Map::new();
    obj.insert("tag".to_owned(), Value::String(tag));
    obj.insert("protocol".to_owned(), Value::String(protocol.to_owned()));
    obj.insert(
        "settings".to_owned(),
        json!({
            "servers": [{
                "address": server,
                "port": port,
                "users": settings,
            }]
        }),
    );
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

type Emit = Result<(&'static str, Vec<Value>, Option<Value>), EmitError>;

fn trojan(node: &Node) -> Emit {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("trojan password")),
    };
    let users = vec![json!({
        "password": password,
    })];
    let stream = stream_settings(node);
    Ok(("trojan", users, stream))
}

fn shadowsocks(node: &Node) -> Emit {
    let password = match &node.authentication {
        Authentication::Password { password } => password.clone(),
        _ => return Err(EmitError::MissingField("ss password")),
    };
    let method = match &node.config {
        ProtocolConfig::Shadowsocks(cfg) => cfg.method.clone(),
        _ => return Err(EmitError::MissingField("ss method")),
    };
    // V2Ray/Xray shadowsocks uses a single-server settings shape (no users array).
    let settings = vec![json!({
        "method": method,
        "password": password,
    })];
    Ok(("shadowsocks", settings, None))
}

fn vmess(node: &Node) -> Emit {
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
    let users = vec![json!({
        "id": uuid,
        "alterId": alter_id,
        "security": security,
    })];
    let stream = stream_settings(node);
    Ok(("vmess", users, stream))
}

fn vless(node: &Node) -> Emit {
    let uuid = match &node.authentication {
        Authentication::Uuid { uuid } => uuid.clone(),
        _ => return Err(EmitError::MissingField("vless uuid")),
    };
    let mut user = Map::new();
    user.insert("id".to_owned(), Value::String(uuid));
    if let ProtocolConfig::VlessReality(cfg) = &node.config
        && let Some(ref flow) = cfg.flow
    {
        user.insert("flow".to_owned(), Value::String(flow.clone()));
    }
    let users = vec![Value::Object(user)];
    let stream = stream_settings(node);
    Ok(("vless", users, stream))
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
        if tls.enabled {
            stream.insert("security".to_owned(), Value::String("tls".to_owned()));
            if let Some(ref sni) = tls.server_name {
                stream.insert("tlsSettings".to_owned(), json!({ "serverName": sni }));
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
