//! Shadowsocks URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::Shadowsocks` + `ProtocolConfig::Shadowsocks` back to a
//! SIP002-format `ss://` share URI.
//!
//! ## SIP002 format
//!
//! ```text
//! ss://BASE64URL(method:password)@host:port/?plugin=...#<display_name>
//! ```

use base64::Engine;

use deve_sub_domain::{Authentication, Node, ProtocolConfig, ShadowsocksConfig};

use crate::common::{QUERY_VALUE_ENCODE, format_fragment};
use crate::error::EmitError;

/// Emit a Shadowsocks [`Node`] as a SIP002-format `ss://` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("password authentication")),
    };

    let ShadowsocksConfig {
        method,
        plugin,
        plugin_opts,
    } = match &node.config {
        ProtocolConfig::Shadowsocks(cfg) => cfg,
        _ => return Err(EmitError::NoEmitter("non-Shadowsocks config".to_owned())),
    };

    // SIP002: userinfo is Base64URL(method:password), without padding.
    let userinfo_plain = format!("{method}:{password}");
    let userinfo =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(userinfo_plain.as_bytes());

    let mut result = format!(
        "ss://{userinfo}@{host}:{port}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );

    // Plugin parameter: `plugin=name;opt1=v1;opt2=v2`.
    if let Some(plugin_name) = plugin {
        let plugin_value = match plugin_opts {
            Some(opts) => format!("{plugin_name};{opts}"),
            None => plugin_name.clone(),
        };
        let encoded =
            percent_encoding::utf8_percent_encode(&plugin_value, QUERY_VALUE_ENCODE).to_string();
        result.push_str("/?plugin=");
        result.push_str(&encoded);
    } else {
        // SIP002 still allows a trailing path, but it's optional. Omit it
        // when there's no plugin for cleaner output.
    }

    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }

    Ok(result)
}
