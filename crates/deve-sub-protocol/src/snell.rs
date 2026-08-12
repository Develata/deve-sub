//! Snell URI parser.
//!
//! Parses `snell://` URIs into canonical [`deve_sub_domain::Node`] values.
//! There is no official Snell URI scheme; Deve Sub parses and emits the
//! de-facto sublinkPro format (see ADR-0007 / M9 Slice 3).
//!
//! ## URI format
//!
//! ```text
//! snell://<psk>@<host>:<port>?version=<1-6>&udp=0|1
//!   &obfs=http|tls&obfs-host=<host>&reuse=0|1#<display_name>
//! ```
//!
//! `version` is required. `psk` lives in userinfo; if userinfo is empty the
//! parser falls back to a `psk=` query parameter (some emitters duplicate it).
//! Snell has **no TLS by default**; only `obfs=tls` populates `node.tls` with
//! the camouflage SNI taken from `obfs-host`.

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, SnellConfig, SnellObfs,
    SnellObfsMode, SnellVersion, TlsConfig, UdpCapability,
};

use crate::error::ParseError;
use crate::uri::{collect_query, decode_fragment, node_shell, parse_bool, parse_host};

pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let userinfo_psk = url.username();
    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    let psk = if userinfo_psk.is_empty() {
        query
            .get("psk")
            .cloned()
            .filter(|s| !s.is_empty())
            .ok_or(ParseError::MissingField("psk"))?
    } else {
        userinfo_psk.to_owned()
    };

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in snell URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let version = query
        .get("version")
        .ok_or(ParseError::MissingField("version"))
        .and_then(|v| {
            v.parse::<u32>()
                .map_err(|_| ParseError::InvalidField {
                    field: "version",
                    value: v.clone(),
                })
                .and_then(|n| {
                    SnellVersion::from_u32(n).ok_or(ParseError::InvalidField {
                        field: "version",
                        value: v.clone(),
                    })
                })
        })?;

    let reuse = query.get("reuse").map(|v| parse_bool(v)).transpose()?;

    let udp_supported = query.get("udp").map(|v| parse_bool(v)).transpose()?;

    let obfs = parse_obfs(&query)?;

    let tls = obfs
        .as_ref()
        .filter(|o| o.mode == SnellObfsMode::Tls)
        .map(|o| TlsConfig {
            enabled: true,
            server_name: o.host.clone(),
            skip_cert_verify: None,
            alpn: o.alpn.clone(),
            client_fingerprint: None,
            certificate_pins: vec![],
            reality: None,
        });

    let config = ProtocolConfig::Snell(SnellConfig {
        version,
        reuse,
        obfs,
        v6_mode: None,
    });

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::Snell;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password { password: psk };
    node.udp = UdpCapability {
        supported: udp_supported,
        xudp: None,
    };
    node.tls = tls;

    Ok(node)
}

fn parse_obfs(query: &HashMap<String, String>) -> Result<Option<SnellObfs>, ParseError> {
    let Some(mode_str) = query.get("obfs") else {
        return Ok(None);
    };
    let mode = match mode_str.as_str() {
        "http" => SnellObfsMode::Http,
        "tls" => SnellObfsMode::Tls,
        other => {
            return Err(ParseError::InvalidField {
                field: "obfs",
                value: other.to_owned(),
            });
        }
    };
    let host = query.get("obfs-host").cloned();
    let alpn = query
        .get("alpn")
        .map(|v| {
            if v.is_empty() {
                vec![]
            } else {
                v.split(',').map(String::from).collect()
            }
        })
        .unwrap_or_default();
    Ok(Some(SnellObfs {
        mode,
        host,
        password: None,
        version: None,
        alpn,
    }))
}
