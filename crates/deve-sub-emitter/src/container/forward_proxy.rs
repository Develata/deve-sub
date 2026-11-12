//! Native forward proxy output. Every optional field is mapped or rejected.

use deve_sub_domain::{Authentication, Node, ProtocolConfig, SocksDnsResolution};
use serde_json::{Map, Value, json};

use crate::{EmitError, forward_proxy::unsupported};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Mihomo,
    SingBox,
    Xray,
}

pub(super) fn mihomo(node: &Node, out: &mut String) -> Result<(), EmitError> {
    let socks = validate(node, Target::Mihomo)?;
    let mut object = base(node, "name", "server", "port");
    object.insert("type".into(), json!(if socks { "socks5" } else { "http" }));
    auth(&mut object, node, "username", "password")?;
    if let Some(udp) = node.udp.supported {
        object.insert("udp".into(), json!(udp));
    }
    if let ProtocolConfig::Http(cfg) = &node.config {
        let headers: Map<String, Value> = cfg
            .headers
            .iter()
            .map(|(key, values)| (key.clone(), json!(values[0])))
            .collect();
        if !headers.is_empty() {
            object.insert("headers".into(), Value::Object(headers));
        }
    }
    if let Some(tls) = &node.tls {
        object.insert("tls".into(), json!(tls.enabled));
        optional(&mut object, "sni", &tls.server_name);
        if let Some(skip) = tls.skip_cert_verify {
            object.insert("skip-cert-verify".into(), json!(skip));
        }
        if let Some(pin) = tls.certificate_pins.first() {
            object.insert(
                "fingerprint".into(),
                json!(pin.as_str().trim_start_matches("mihomoFingerprintSHA256:")),
            );
        }
    }
    for (index, (key, value)) in object.into_iter().enumerate() {
        out.push_str(if index == 0 { "\n  - " } else { "\n    " });
        // JSON strings and collections are valid YAML flow values.
        out.push_str(&format!("{}: {value}", json!(key)));
    }
    Ok(())
}

pub(super) fn singbox(node: &Node) -> Result<Value, EmitError> {
    let socks = validate(node, Target::SingBox)?;
    let mut object = base(node, "tag", "server", "server_port");
    object.insert("type".into(), json!(if socks { "socks" } else { "http" }));
    auth(&mut object, node, "username", "password")?;
    if socks {
        object.insert("version".into(), json!("5"));
        if let Some(udp) = node.udp.supported {
            object.insert(
                "network".into(),
                if udp {
                    json!(["tcp", "udp"])
                } else {
                    json!("tcp")
                },
            );
        }
    }
    if let ProtocolConfig::Http(cfg) = &node.config {
        optional(&mut object, "path", &cfg.path);
        if !cfg.headers.is_empty() {
            object.insert("headers".into(), json!(cfg.headers));
        }
    }
    if let Some(tls) = &node.tls {
        let mut value = Map::new();
        value.insert("enabled".into(), json!(tls.enabled));
        optional(&mut value, "server_name", &tls.server_name);
        if let Some(skip) = tls.skip_cert_verify {
            value.insert("insecure".into(), json!(skip));
        }
        if !tls.alpn.is_empty() {
            value.insert("alpn".into(), json!(tls.alpn));
        }
        if let Some(fp) = &tls.client_fingerprint {
            value.insert("utls".into(), json!({"enabled":true,"fingerprint":fp}));
        }
        if !tls.certificate_pins.is_empty() {
            value.insert(
                "certificate_public_key_sha256".into(),
                json!(
                    tls.certificate_pins
                        .iter()
                        .map(|pin| pin.as_str().trim_start_matches("spkiSHA256:"))
                        .collect::<Vec<_>>()
                ),
            );
        }
        object.insert("tls".into(), Value::Object(value));
    }
    Ok(Value::Object(object))
}

pub(super) fn xray(node: &Node) -> Result<Value, EmitError> {
    let socks = validate(node, Target::Xray)?;
    let mut server = Map::new();
    server.insert("address".into(), json!(host(node)));
    server.insert("port".into(), json!(node.endpoint.port));
    if !matches!(node.authentication, Authentication::None) {
        let mut user = Map::new();
        auth(&mut user, node, "user", "pass")?;
        server.insert("users".into(), json!([user]));
    }
    let mut object = Map::new();
    object.insert("tag".into(), json!(node.display_name));
    object.insert(
        "protocol".into(),
        json!(if socks { "socks" } else { "http" }),
    );
    let mut settings = json!({"servers":[server]});
    if let ProtocolConfig::Http(cfg) = &node.config
        && !cfg.headers.is_empty()
    {
        settings["headers"] = Value::Object(
            cfg.headers
                .iter()
                .map(|(key, values)| (key.clone(), json!(values[0])))
                .collect(),
        );
    }
    object.insert("settings".into(), settings);
    if let Some(tls) = &node.tls {
        if tls.enabled {
            let mut value = Map::new();
            optional(&mut value, "serverName", &tls.server_name);
            if let Some(skip) = tls.skip_cert_verify {
                value.insert("allowInsecure".into(), json!(skip));
            }
            if !tls.alpn.is_empty() {
                value.insert("alpn".into(), json!(tls.alpn));
            }
            optional(&mut value, "fingerprint", &tls.client_fingerprint);
            object.insert(
                "streamSettings".into(),
                json!({"network":"tcp","security":"tls","tlsSettings":value}),
            );
        } else {
            object.insert(
                "streamSettings".into(),
                json!({"network":"tcp","security":"none"}),
            );
        }
    }
    Ok(Value::Object(object))
}

fn validate(node: &Node, target: Target) -> Result<bool, EmitError> {
    crate::forward_proxy::validate_tls_state(node)?;
    let socks = match &node.config {
        ProtocolConfig::Socks5(cfg) => {
            if cfg.dns_resolution != SocksDnsResolution::Unspecified {
                return Err(unsupported("explicit SOCKS destination DNS policy"));
            }
            true
        }
        ProtocolConfig::Http(cfg) => {
            if target != Target::SingBox && cfg.path.is_some() {
                return Err(unsupported("HTTP CONNECT path"));
            }
            if target != Target::SingBox && cfg.headers.values().any(|v| v.len() != 1) {
                return Err(unsupported("repeated HTTP header values"));
            }
            if node.udp.supported == Some(true) {
                return Err(unsupported("HTTP UDP"));
            }
            if target != Target::Mihomo && node.udp.supported.is_some() {
                return Err(unsupported(
                    "explicit HTTP UDP policy without a target representation",
                ));
            }
            false
        }
        _ => return Err(unsupported("untyped forward proxy")),
    };
    if node.transport.is_some()
        || node.multiplex.is_some()
        || node.obfuscation.is_some()
        || node.congestion.is_some()
        || node.udp.xudp.is_some()
    {
        return Err(unsupported("forward proxy transport extension"));
    }
    if socks && target == Target::Xray && node.udp.supported.is_some() {
        return Err(unsupported("explicit SOCKS UDP policy on Xray/V2Ray"));
    }
    if let Some(tls) = &node.tls {
        if socks && target == Target::SingBox {
            return Err(unsupported("SOCKS TLS on sing-box"));
        }
        if tls.reality.is_some() {
            return Err(unsupported("forward proxy Reality"));
        }
        if target == Target::Mihomo
            && (!tls.alpn.is_empty()
                || tls.client_fingerprint.is_some()
                || socks && tls.server_name.is_some())
        {
            return Err(unsupported(
                "TLS field absent from Mihomo forward proxy schema",
            ));
        }
        let pins = &tls.certificate_pins;
        let pins_ok = match target {
            Target::Mihomo => {
                pins.len() <= 1
                    && pins
                        .iter()
                        .all(|p| p.as_str().starts_with("mihomoFingerprintSHA256:"))
            }
            Target::SingBox => pins.iter().all(|p| p.as_str().starts_with("spkiSHA256:")),
            Target::Xray => pins.is_empty(),
        };
        if !pins_ok {
            return Err(unsupported("certificate pin representation"));
        }
    }
    Ok(socks)
}

fn base(node: &Node, name: &str, server: &str, port: &str) -> Map<String, Value> {
    Map::from_iter([
        (name.into(), json!(node.display_name)),
        (server.into(), json!(host(node))),
        (port.into(), json!(node.endpoint.port)),
    ])
}

fn host(node: &Node) -> String {
    node.endpoint.host.address_host()
}

fn auth(
    object: &mut Map<String, Value>,
    node: &Node,
    user: &str,
    pass: &str,
) -> Result<(), EmitError> {
    match &node.authentication {
        Authentication::None => {}
        Authentication::UserPassword { username, password } => {
            object.insert(user.into(), json!(username));
            object.insert(pass.into(), json!(password));
        }
        _ => return Err(unsupported("forward proxy authentication")),
    }
    Ok(())
}

fn optional(object: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        object.insert(key.into(), json!(value));
    }
}
