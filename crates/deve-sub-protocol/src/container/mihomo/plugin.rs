//! Mihomo plugin maps projected into the verified SIP003 subset.

use deve_sub_domain::{ShadowsocksConfig, ShadowsocksPlugin};
use serde_json::Value;

use super::super::get_str;
use crate::{ParseError, forward_proxy::invalid};

pub(super) fn parse(entry: &Value) -> Result<(Option<String>, Option<String>), ParseError> {
    let Some(name) = get_str(entry, "plugin") else {
        if entry.get("plugin-opts").is_some() {
            return Err(invalid("Shadowsocks plugin options without a plugin"));
        }
        return Ok((None, None));
    };
    let opts = entry
        .get("plugin-opts")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("Shadowsocks plugin options must be a map"))?;
    let allowed: &[&str] = match name.as_str() {
        "obfs" => &["mode", "host"],
        "v2ray-plugin" => &["mode", "host", "path", "tls", "mux"],
        "shadow-tls" => &["version", "password", "host"],
        _ => return Err(invalid("unsupported Shadowsocks plugin")),
    };
    if opts.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("unsupported Shadowsocks plugin option"));
    }
    // ShadowTLS becomes its own wrapper; the inner Shadowsocks has no plugin.
    if name == "shadow-tls" {
        return Ok((None, None));
    }
    let text = |key: &str, default: &str| -> Result<String, ParseError> {
        let value = match opts.get(key) {
            None => default,
            Some(Value::String(value)) => value,
            _ => return Err(invalid("expected a text plugin option")),
        };
        // WHY: this subset does not implement SIP003 escaping. Never turn a
        // literal native value into a second option or a different flag.
        if value.contains([';', '\\']) {
            return Err(invalid(
                "plugin option requires unsupported SIP003 escaping",
            ));
        }
        Ok(value.to_owned())
    };
    let boolean = |key| -> Result<Option<bool>, ParseError> {
        opts.get(key)
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| invalid("expected a boolean plugin option"))
            })
            .transpose()
    };
    let host = text("host", "bing.com")?;
    // WHY: the client applies this default only when host is absent. This
    // subset cannot preserve every plugin's explicit-empty host behavior.
    if host.is_empty() {
        return Err(invalid(
            "explicit empty plugin host mapping is not implemented",
        ));
    }
    let mode = text("mode", "")?;
    let plugin = if name == "obfs" {
        if !matches!(mode.as_str(), "http" | "tls") {
            return Err(invalid("unsupported obfs mode"));
        }
        ShadowsocksPlugin::Obfs {
            mode,
            host: Some(host),
        }
    } else {
        if mode != "websocket" {
            return Err(invalid("unsupported v2ray-plugin mode"));
        }
        ShadowsocksPlugin::WebSocket {
            host,
            path: text("path", "/")?,
            tls: boolean("tls")?.unwrap_or(false),
            multiplex: boolean("mux")?.unwrap_or(true),
        }
    };
    let (name, opts) = plugin.sip003();
    // Keep native input and future canonical emitters on one validated subset.
    let config = ShadowsocksConfig {
        method: String::new(),
        plugin: Some(name.into()),
        plugin_opts: Some(opts.clone()),
    };
    ShadowsocksPlugin::from_config(&config).map_err(|_| invalid("unsupported plugin options"))?;
    Ok((Some(name.into()), Some(opts)))
}
