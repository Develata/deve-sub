//! TUIC v5 URI parser.
//!
//! Parses `tuic://` URIs into canonical [`deve_sub_domain::Node`] values.
//!
//! ## URI format
//!
//! ```text
//! tuic://<uuid>:<password>@<host>:<port>?sni=...&alpn=...
//!   &congestion-controller=bbr&udp-relay-mode=native
//!   &zero-rtt-handshake=1&heartbeat=10000&disable-sni=0
//!   &skip-cert-verify=0#<display_name>
//! ```

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, CongestionConfig, CongestionController, Endpoint, Node, ProtocolConfig,
    ProtocolKind, TuicV5Config, UdpRelayMode,
};

use crate::error::ParseError;
use crate::uri::{
    build_common_tls, collect_query, decode_fragment, node_shell, parse_bool,
    parse_duration_millis, parse_host,
};

/// Parse a parsed `tuic://` URL into a canonical [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let uuid = url.username();
    if uuid.is_empty() {
        return Err(ParseError::MissingField("uuid"));
    }
    let password = url.password().ok_or(ParseError::MissingField("password"))?;

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in tuic URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    let tls = build_common_tls(&query, &["skip-cert-verify", "insecure"], None)?;

    let congestion = query.get("congestion-controller").map(|v| {
        let controller = match v.as_str() {
            "bbr" => CongestionController::Bbr,
            "cubic" => CongestionController::Cubic,
            "new_reno" => CongestionController::NewReno,
            other => CongestionController::Other(other.to_owned()),
        };
        CongestionConfig {
            controller,
            up_bps: None,
            down_bps: None,
        }
    });

    let udp_relay_mode = query
        .get("udp-relay-mode")
        .map(|v| match v.as_str() {
            "native" => Ok(UdpRelayMode::Native),
            "quic" => Ok(UdpRelayMode::Quic),
            _ => Err(ParseError::InvalidField {
                field: "udp-relay-mode",
                value: v.clone(),
            }),
        })
        .transpose()?;

    let zero_rtt_handshake = query
        .get("zero-rtt-handshake")
        .map(|v| parse_bool(v))
        .transpose()?;

    let heartbeat = query
        .get("heartbeat")
        .map(|v| parse_duration_millis(v))
        .transpose()?;

    let disable_sni = query
        .get("disable-sni")
        .map(|v| parse_bool(v))
        .transpose()?;

    let config = ProtocolConfig::TuicV5(TuicV5Config {
        udp_relay_mode,
        zero_rtt_handshake,
        heartbeat,
        disable_sni,
    });

    let mut node = node_shell(raw_uri);
    node.display_name = display_name;
    node.protocol = ProtocolKind::TuicV5;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::UuidPassword {
        uuid: uuid.to_owned(),
        password: password.to_owned(),
    };
    node.tls = tls;
    node.congestion = congestion;

    Ok(node)
}
