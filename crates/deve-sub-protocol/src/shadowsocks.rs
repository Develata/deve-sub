//! Shadowsocks URI parser.
//!
//! Parses `ss://` URIs into canonical [`deve_sub_domain::Node`] values.
//! Supports both SIP002 (modern) and legacy Base64 formats.
//!
//! ## SIP002 format (modern)
//!
//! ```text
//! ss://BASE64URL(method:password)@host:port/?plugin=...#<display_name>
//! ```
//!
//! ## Legacy format
//!
//! ```text
//! ss://BASE64(method:password@host:port)#<display_name>
//! ```
//!
//! PARSE-010: Base64 with and without padding are both accepted.

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, ShadowsocksConfig,
};

use crate::error::ParseError;
use crate::uri::{decode_base64_flexible, decode_fragment_parts, node_shell, parse_host_port};

/// Parse a raw `ss://` URI into a canonical [`Node`].
pub(crate) fn parse(uri: &str) -> Result<Node, ParseError> {
    let (body, fragment) = match uri.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (uri, None),
    };

    let after_scheme = body
        .strip_prefix("ss://")
        .ok_or(ParseError::UnknownScheme("expected ss://".to_owned()))?;

    // Split off query (e.g. `?plugin=obfs-local;obfs=http`).
    let (authority_and_path, query) = match after_scheme.split_once('?') {
        Some((a, q)) => (a, Some(q)),
        None => (after_scheme, None),
    };

    // WHY: Check for SIP002 vs legacy *before* stripping path. Legacy Base64
    // uses the standard alphabet which includes `/`, so blindly splitting on
    // `/` would corrupt the Base64 body. Only SIP002 has a path component.
    let (method, password, host, port) =
        if let Some((userinfo, host_port_path)) = authority_and_path.split_once('@') {
            // SIP002: userinfo is Base64URL(method:password)@host:port[/path].
            // Strip path only for SIP002 (Base64URL uses `-`/`_`, never `/`).
            let host_port = host_port_path.split('/').next().unwrap_or(host_port_path);

            let decoded = decode_base64_flexible(userinfo)?;
            let decoded_str =
                String::from_utf8(decoded).map_err(|e| ParseError::InvalidBase64(e.to_string()))?;
            let (method, password) = decoded_str
                .split_once(':')
                .ok_or(ParseError::MissingField("method:password in userinfo"))?;

            let (host, port) = parse_host_port(host_port)?;

            (method.to_owned(), password.to_owned(), host, port)
        } else {
            // Legacy: entire authority is Base64(method:password@host:port).
            // Do NOT strip path — the Base64 body may contain `/`.
            let decoded = decode_base64_flexible(authority_and_path)?;
            let decoded_str =
                String::from_utf8(decoded).map_err(|e| ParseError::InvalidBase64(e.to_string()))?;
            let (userinfo, host_port) = decoded_str.split_once('@').ok_or(
                ParseError::MissingField("method:password@host:port in legacy base64"),
            )?;
            let (method, password) = userinfo
                .split_once(':')
                .ok_or(ParseError::MissingField("method:password"))?;

            let (host, port) = parse_host_port(host_port)?;
            (method.to_owned(), password.to_owned(), host, port)
        };

    // Parse plugin from query parameter `plugin=name;opt1=v1;opt2=v2`.
    let (plugin, plugin_opts) = match query {
        Some(q) => {
            let plugin_value = url::form_urlencoded::parse(q.as_bytes())
                .find(|(k, _)| k == "plugin")
                .map(|(_, v)| v.into_owned());
            match plugin_value {
                Some(p) => match p.split_once(';') {
                    Some((name, opts)) => (Some(name.to_owned()), Some(opts.to_owned())),
                    None => (Some(p), None),
                },
                None => (None, None),
            }
        }
        None => (None, None),
    };

    let config = ProtocolConfig::Shadowsocks(ShadowsocksConfig {
        method,
        plugin,
        plugin_opts,
    });

    let mut node = node_shell(Some(uri));
    node.display_name = decode_fragment_parts(fragment);
    node.protocol = ProtocolKind::Shadowsocks;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password { password };

    Ok(node)
}
