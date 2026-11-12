//! Lossless container mappings for ordinary SOCKS5 and HTTP CONNECT proxies.

use std::collections::BTreeMap;

use deve_sub_domain::{
    Endpoint, HttpProxyConfig, Node, ProtocolConfig, ProtocolKind, Socks5Config, SocksDnsResolution,
};
use serde_json::Value;

use crate::ParseError;
use crate::forward_proxy::{authentication, invalid};

use super::{get_port, get_str, node_shell_container, parse_host_str};

pub(super) fn mihomo(entry: &Value, kind: &str) -> Result<Node, ParseError> {
    let mut normalized = entry.clone();
    for key in ["name", "username", "password"] {
        if entry.get(key).is_some_and(Value::is_number)
            && let Some(value) = super::get_scalar_text(entry, key)
        {
            normalized[key] = Value::String(value);
        }
    }
    let entry = &normalized;
    allowed(
        entry,
        &[
            "name",
            "type",
            "server",
            "port",
            "username",
            "password",
            "tls",
            "sni",
            "servername",
            "skip-cert-verify",
            "alpn",
            "client-fingerprint",
            "fingerprint",
            "udp",
            "headers",
        ],
    )?;
    let socks = kind.starts_with("socks");
    let mut node = base(entry, "name", "server", "port", socks)?;
    node.tls = super::forward_proxy_tls::mihomo(entry, kind == "https")?;
    node.udp.supported = bool_field(entry, "udp")?;
    if socks {
        node.config = ProtocolConfig::Socks5(Socks5Config {
            dns_resolution: if kind == "socks5h" {
                SocksDnsResolution::Remote
            } else {
                SocksDnsResolution::Unspecified
            },
        });
        if entry.get("headers").is_some() {
            return Err(invalid("HTTP headers are not SOCKS options"));
        }
    } else {
        if node.udp.supported == Some(true) {
            return Err(invalid("HTTP CONNECT does not carry UDP"));
        }
        node.config = ProtocolConfig::Http(HttpProxyConfig {
            headers: headers(entry.get("headers"))?,
            path: None,
        });
    }
    Ok(node)
}

pub(super) fn singbox(entry: &Value, socks: bool) -> Result<Node, ParseError> {
    allowed(
        entry,
        &[
            "type",
            "tag",
            "server",
            "server_port",
            "version",
            "username",
            "password",
            "network",
            "tls",
            "path",
            "headers",
        ],
    )?;
    let mut node = base(entry, "tag", "server", "server_port", socks)?;
    if socks {
        if entry
            .get("version")
            .is_some_and(|v| v.as_str() != Some("5"))
        {
            return Err(invalid("only SOCKS version 5 is supported"));
        }
        if ["tls", "path", "headers"]
            .iter()
            .any(|key| entry.get(key).is_some())
        {
            return Err(invalid("sing-box SOCKS has no TLS or HTTP options"));
        }
        node.udp.supported = socks_network(entry.get("network"))?;
    } else {
        if entry.get("version").is_some() || entry.get("network").is_some() {
            return Err(invalid("HTTP version/network option is not represented"));
        }
        node.tls = super::forward_proxy_tls::singbox(entry.get("tls"))?;
        node.config = ProtocolConfig::Http(HttpProxyConfig {
            path: string(entry, "path")?,
            headers: headers(entry.get("headers"))?,
        });
    }
    Ok(node)
}

pub(super) fn xray(entry: &Value, socks: bool) -> Result<Node, ParseError> {
    allowed(entry, &["tag", "protocol", "settings", "streamSettings"])?;
    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;
    allowed(
        settings,
        &["servers", "address", "port", "user", "pass", "headers"],
    )?;
    let server = if let Some(servers) = settings.get("servers") {
        let servers = servers
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| invalid("exactly one proxy server is required"))?;
        if settings.as_object().is_some_and(|v| {
            v.keys()
                .any(|key| !["servers", "headers"].contains(&key.as_str()))
        }) {
            return Err(invalid("mixed server representations"));
        }
        &servers[0]
    } else {
        settings
    };
    let legacy = settings.get("servers").is_some();
    if legacy {
        allowed(server, &["address", "port", "users"])?;
    } else {
        allowed(server, &["address", "port", "user", "pass", "headers"])?;
    }
    let mut node = base_endpoint(
        get_str(entry, "tag").unwrap_or_default(),
        server,
        "address",
        "port",
        socks,
    )?;
    node.authentication = if let Some(users) = server.get("users") {
        if server.get("user").is_some() || server.get("pass").is_some() {
            return Err(invalid("mixed proxy authentication representations"));
        }
        let users = users
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| invalid("exactly one proxy authentication identity is required"))?;
        allowed(&users[0], &["user", "pass"])?;
        authentication(
            string(&users[0], "user")?,
            string(&users[0], "pass")?,
            socks,
        )?
    } else {
        authentication(string(server, "user")?, string(server, "pass")?, socks)?
    };
    node.tls = super::forward_proxy_tls::xray(entry.get("streamSettings"))?;
    if !socks {
        node.config = ProtocolConfig::Http(HttpProxyConfig {
            path: None,
            headers: headers(settings.get("headers"))?,
        });
    } else if settings.get("headers").is_some() {
        return Err(invalid("SOCKS does not support HTTP headers"));
    }
    Ok(node)
}

fn base(
    entry: &Value,
    name: &str,
    host: &str,
    port: &str,
    socks: bool,
) -> Result<Node, ParseError> {
    let mut node = base_endpoint(
        get_str(entry, name).unwrap_or_default(),
        entry,
        host,
        port,
        socks,
    )?;
    node.authentication = authentication(
        string(entry, "username")?,
        string(entry, "password")?,
        socks,
    )?;
    Ok(node)
}

fn socks_network(value: Option<&Value>) -> Result<Option<bool>, ParseError> {
    let Some(value) = value else { return Ok(None) };
    let networks: Vec<&str> = if let Some(text) = value.as_str() {
        text.split(',').collect()
    } else {
        value
            .as_array()
            .ok_or_else(|| invalid("invalid SOCKS network"))?
            .iter()
            .map(|v| v.as_str().ok_or_else(|| invalid("invalid SOCKS network")))
            .collect::<Result<_, _>>()?
    };
    match networks.as_slice() {
        ["tcp"] => Ok(Some(false)),
        ["tcp", "udp"] | ["udp", "tcp"] => Ok(Some(true)),
        _ => Err(invalid(
            "SOCKS network must include TCP without unrepresented networks",
        )),
    }
}

fn base_endpoint(
    name: String,
    entry: &Value,
    host: &str,
    port: &str,
    socks: bool,
) -> Result<Node, ParseError> {
    let mut node = node_shell_container();
    node.display_name = name;
    node.endpoint = Endpoint {
        host: parse_host_str(&string(entry, host)?.ok_or(ParseError::MissingField("server"))?)?,
        port: get_port(entry, port)?
            .filter(|port| *port != 0)
            .ok_or(ParseError::MissingField("nonzero port"))?,
    };
    node.protocol = if socks {
        ProtocolKind::Socks5
    } else {
        ProtocolKind::Http
    };
    node.config = if socks {
        ProtocolConfig::Socks5(Socks5Config::default())
    } else {
        ProtocolConfig::Http(HttpProxyConfig::default())
    };
    Ok(node)
}

pub(super) fn allowed(value: &Value, keys: &[&str]) -> Result<(), ParseError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("expected an object"))?;
    if object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(invalid("unrepresented option; raw entry retained"));
    }
    Ok(())
}

pub(super) fn string(value: &Value, key: &str) -> Result<Option<String>, ParseError> {
    value
        .get(key)
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("expected a string"))
        })
        .transpose()
}

pub(super) fn bool_field(value: &Value, key: &str) -> Result<Option<bool>, ParseError> {
    value
        .get(key)
        .map(|v| v.as_bool().ok_or_else(|| invalid("expected a boolean")))
        .transpose()
}

fn headers(value: Option<&Value>) -> Result<BTreeMap<String, Vec<String>>, ParseError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| invalid("headers must be an object"))?;
    object
        .iter()
        .map(|(key, value)| {
            if key.is_empty()
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            {
                return Err(invalid("invalid header name"));
            }
            let values = if let Some(value) = value.as_str() {
                vec![value.to_owned()]
            } else {
                value
                    .as_array()
                    .ok_or_else(|| invalid("invalid header values"))?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| invalid("invalid header value"))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            if values.is_empty() || values.iter().any(|v| v.contains(['\r', '\n', '\0'])) {
                return Err(invalid("invalid header value"));
            }
            Ok((key.clone(), values))
        })
        .collect()
}
