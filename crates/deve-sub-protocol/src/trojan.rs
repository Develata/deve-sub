//! Trojan URI parser.
//!
//! Parses `trojan://` URIs into canonical [`deve_sub_domain::Node`] values.
//! Trojan always uses TLS; the `tls` field is always `Some`.
//!
//! ## URI format
//!
//! ```text
//! trojan://<password>@<host>:<port>?sni=...&alpn=...&skip-cert-verify=...
//!   &type=tcp&path=...&host=...&packetEncoding=...#<display_name>
//! ```

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, TlsConfig, Transport,
    TransportKind, TrojanConfig,
};

use crate::error::ParseError;
use crate::transport::map_transport_kind;
use crate::uri::{build_common_tls, collect_query, decode_fragment, node_shell, parse_host};

/// Parse a parsed `trojan://` URL into a canonical [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let password = url.username();
    if password.is_empty() {
        return Err(ParseError::MissingField("password"));
    }

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in trojan URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    let transport_kind = query
        .get("type")
        .map(|t| map_transport_kind(t))
        .transpose()?
        .unwrap_or(TransportKind::Tcp);

    // WHY: Trojan always uses TLS; the `tls` field is unconditionally Some.
    let tls = build_common_tls(
        &query,
        &["skip-cert-verify", "allowInsecure", "insecure"],
        None,
    )?
    .unwrap_or_else(|| TlsConfig {
        enabled: true,
        server_name: None,
        skip_cert_verify: None,
        alpn: vec![],
        client_fingerprint: None,
        certificate_pins: vec![],
        reality: None,
    });

    let config = ProtocolConfig::Trojan(TrojanConfig {
        packet_encoding: query.get("packetEncoding").cloned(),
    });

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::Trojan;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password {
        password: password.to_owned(),
    };
    node.transport = Some(Transport {
        kind: transport_kind,
        path: query.get("path").cloned(),
        host: query.get("host").cloned(),
    });
    node.tls = Some(tls);

    Ok(node)
}
