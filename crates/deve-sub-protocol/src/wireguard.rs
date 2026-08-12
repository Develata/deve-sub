//! WireGuard URI parser.
//!
//! Parses `wireguard://` URIs into canonical [`deve_sub_domain::Node`] values.
//!
//! ## URI format
//!
//! ```text
//! wireguard://<private-key>@<host>:<port>?publickey=<pk>&address=<cidr>
//!   &presharedkey=<psk>&reserved=<r,g,b>&mtu=<mtu>#<display_name>
//! ```
//!
//! The private key is in the userinfo field. The `publickey` and `address`
//! query parameters are required; others are optional. WireGuard has no TLS
//! layer, so `node.tls` is always `None`.

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, WireGuardConfig, WireGuardPeer,
};

use crate::error::ParseError;
use crate::uri::{collect_query, decode_fragment, node_shell, parse_host};

/// Parse a parsed `wireguard://` URL into a canonical [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let private_key = url.username();
    if private_key.is_empty() {
        return Err(ParseError::MissingField("private-key"));
    }

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in wireguard URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    let public_key = query
        .get("publickey")
        .ok_or(ParseError::MissingField("publickey"))?
        .clone();

    let address_str = query
        .get("address")
        .ok_or(ParseError::MissingField("address"))?;
    let address: Vec<String> = if address_str.is_empty() {
        vec![]
    } else {
        address_str.split(',').map(String::from).collect()
    };

    let pre_shared_key = query.get("presharedkey").cloned();

    let reserved = query
        .get("reserved")
        .map(|v| parse_reserved(v))
        .transpose()?;

    let mtu = query
        .get("mtu")
        .map(|v| v.parse::<u32>())
        .transpose()
        .map_err(|_| ParseError::InvalidField {
            field: "mtu",
            value: query.get("mtu").cloned().unwrap_or_default(),
        })?;

    let persistent_keepalive = query
        .get("keepalive")
        .map(|v| crate::uri::parse_duration_secs(v))
        .transpose()?;

    let allowed_ips = query
        .get("allowedips")
        .map(|v| {
            if v.is_empty() {
                vec![]
            } else {
                v.split(',').map(String::from).collect()
            }
        })
        .unwrap_or_else(|| vec!["0.0.0.0/0".to_owned(), "::/0".to_owned()]);

    let peer = WireGuardPeer {
        public_key,
        pre_shared_key,
        allowed_ips,
        reserved,
        persistent_keepalive,
    };

    let config = ProtocolConfig::WireGuard(WireGuardConfig {
        private_key: private_key.to_owned(),
        address,
        peers: vec![peer],
        mtu,
        workers: None,
        dns: vec![],
    });

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::WireGuard;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::None;

    Ok(node)
}

fn parse_reserved(value: &str) -> Result<[u8; 3], ParseError> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != 3 {
        return Err(ParseError::InvalidField {
            field: "reserved",
            value: value.to_owned(),
        });
    }
    let mut bytes = [0u8; 3];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = part
            .trim()
            .parse::<u8>()
            .map_err(|_| ParseError::InvalidField {
                field: "reserved",
                value: value.to_owned(),
            })?;
    }
    Ok(bytes)
}
