//! Hysteria2 URI parser.
//!
//! Parses `hysteria2://` and `hy2://` URIs into canonical
//! [`deve_sub_domain::Node`] values.
//!
//! ## URI format
//!
//! ```text
//! hysteria2://<password>@<host>:<port>?sni=...&alpn=...&insecure=...
//!   &pinSHA256=...&obfs=...&obfs-password=...&up=...&down=...
//!   &ports=...&hop_interval=...&fast-open=...&lazy=...#<display_name>
//! ```

use std::collections::HashMap;

use deve_sub_domain::{
    Authentication, CongestionConfig, CongestionController, Endpoint, Hysteria2Config, Node,
    Obfuscation, ProtocolConfig, ProtocolKind, TlsConfig,
};

use crate::error::ParseError;
use crate::uri::{
    build_common_tls, collect_query, decode_fragment, decode_userinfo, node_shell, parse_bandwidth,
    parse_bool, parse_duration_secs, parse_host,
};

/// Parse a parsed `hysteria2://` or `hy2://` URL into a canonical [`Node`].
pub(crate) fn parse(url: &url::Url, raw_uri: &str) -> Result<Node, ParseError> {
    let password = decode_userinfo(url.username());
    if password.is_empty() {
        return Err(ParseError::MissingField("password"));
    }

    let host_str = url
        .host_str()
        .ok_or_else(|| ParseError::InvalidHost("missing host in hysteria2 URI".to_owned()))?;
    let host = parse_host(host_str)?;

    let port = url.port().ok_or(ParseError::MissingField("port"))?;

    let display_name = decode_fragment(url);
    let query: HashMap<String, String> = collect_query(url);

    // WHY: Hysteria2 is QUIC-based and always uses TLS. Even without explicit
    // TLS query params, force `tls` to Some with defaults.
    let tls = Some(
        build_common_tls(&query, &["insecure", "skip-cert-verify"], Some("pinSHA256"))?
            .unwrap_or_else(|| TlsConfig {
                enabled: true,
                server_name: None,
                skip_cert_verify: None,
                alpn: vec![],
                client_fingerprint: None,
                certificate_pins: vec![],
                reality: None,
            }),
    );

    let obfuscation = query.get("obfs").map(|kind| Obfuscation {
        kind: kind.clone(),
        password: query.get("obfs-password").cloned(),
    });

    let up_bps = query.get("up").map(|v| parse_bandwidth(v)).transpose()?;
    let down_bps = query.get("down").map(|v| parse_bandwidth(v)).transpose()?;

    // WHY: Parse congestion-controller from the query instead of hardcoding
    // Bbr. The emitter writes congestion-controller for non-Bbr values, so
    // without this the field is silently lost on round-trip (W-U fix).
    // Also create CongestionConfig when only congestion-controller is
    // present (no up/down), matching the TUIC parser pattern.
    let controller = query
        .get("congestion-controller")
        .map(|v| match v.as_str() {
            "bbr" => CongestionController::Bbr,
            "cubic" => CongestionController::Cubic,
            "new_reno" => CongestionController::NewReno,
            other => CongestionController::Other(other.to_owned()),
        });

    let congestion = if up_bps.is_some() || down_bps.is_some() || controller.is_some() {
        Some(CongestionConfig {
            controller: controller.unwrap_or(CongestionController::Bbr),
            up_bps,
            down_bps,
        })
    } else {
        None
    };

    let hop_interval = query
        .get("hop_interval")
        .map(|v| parse_duration_secs(v))
        .transpose()?;

    let fast_open = query.get("fast-open").map(|v| parse_bool(v)).transpose()?;
    let lazy = query.get("lazy").map(|v| parse_bool(v)).transpose()?;

    let config = ProtocolConfig::Hysteria2(Hysteria2Config {
        ports: query.get("ports").cloned(),
        hop_interval,
        fast_open,
        lazy,
    });

    let mut node = node_shell(Some(raw_uri));
    node.display_name = display_name;
    node.protocol = ProtocolKind::Hysteria2;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::Password {
        password: password.to_owned(),
    };
    node.tls = tls;
    node.obfuscation = obfuscation;
    node.congestion = congestion;

    Ok(node)
}
