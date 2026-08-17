//! TUIC v5 URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::TuicV5` + `ProtocolConfig::TuicV5` back to a
//! `tuic://` share URI.
//!
//! ## Query parameter order
//!
//! `sni`, `alpn`, `skip-cert-verify`, `congestion-controller`,
//! `udp-relay-mode`, `zero-rtt-handshake`, `heartbeat`, `disable-sni`.

use deve_sub_domain::{
    Authentication, CongestionController, Node, ProtocolConfig, TuicV5Config, UdpRelayMode,
};

use crate::common::{encode_userinfo, format_fragment, format_query};
use crate::error::EmitError;

/// Emit a TUIC v5 [`Node`] as a `tuic://` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let (uuid, password) = match &node.authentication {
        Authentication::UuidPassword { uuid, password } => (uuid, password),
        _ => return Err(EmitError::MissingField("uuid+password authentication")),
    };

    let TuicV5Config {
        udp_relay_mode,
        zero_rtt_handshake,
        heartbeat,
        disable_sni,
    } = match &node.config {
        ProtocolConfig::TuicV5(cfg) => cfg,
        _ => return Err(EmitError::NoEmitter("non-TUIC config".to_owned())),
    };

    let mut params: Vec<(String, String)> = Vec::new();

    if let Some(tls) = node.tls.as_ref() {
        if let Some(ref sni) = tls.server_name {
            params.push(("sni".to_owned(), sni.clone()));
        }
        if !tls.alpn.is_empty() {
            params.push(("alpn".to_owned(), tls.alpn.join(",")));
        }
        if let Some(skip) = tls.skip_cert_verify {
            params.push((
                "skip-cert-verify".to_owned(),
                if skip { "1" } else { "0" }.to_owned(),
            ));
        }
    }

    if let Some(ref cong) = node.congestion {
        let name = match &cong.controller {
            CongestionController::Bbr => "bbr",
            CongestionController::Cubic => "cubic",
            CongestionController::NewReno => "new_reno",
            CongestionController::Other(name) => name.as_str(),
        };
        params.push(("congestion-controller".to_owned(), name.to_owned()));
    }

    if let Some(mode) = udp_relay_mode {
        let s = match mode {
            UdpRelayMode::Native => "native",
            UdpRelayMode::Quic => "quic",
        };
        params.push(("udp-relay-mode".to_owned(), s.to_owned()));
    }

    if let Some(zrtt) = zero_rtt_handshake {
        params.push((
            "zero-rtt-handshake".to_owned(),
            if *zrtt { "1" } else { "0" }.to_owned(),
        ));
    }

    if let Some(hb) = heartbeat {
        let ms = hb.whole_milliseconds();
        if ms >= 0 {
            params.push(("heartbeat".to_owned(), ms.to_string()));
        }
    }

    if let Some(ds) = disable_sni {
        params.push((
            "disable-sni".to_owned(),
            if *ds { "1" } else { "0" }.to_owned(),
        ));
    }

    let query = format_query(&params);

    let uuid_enc = encode_userinfo(uuid);
    let pwd_enc = encode_userinfo(password);
    let mut result = format!(
        "tuic://{uuid_enc}:{pwd_enc}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
