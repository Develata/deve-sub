//! Hysteria2 URI emitter.
//!
//! Serializes a canonical [`deve_sub_domain::Node`] with
//! `ProtocolKind::Hysteria2` + `ProtocolConfig::Hysteria2` back to a
//! `hysteria2://` share URI.
//!
//! ## Query parameter order
//!
//! `sni`, `alpn`, `insecure`, `pinSHA256`, `obfs`, `obfs-password`, `up`,
//! `down`, `ports`, `hop_interval`, `fast-open`, `lazy`.

use deve_sub_domain::{
    Authentication, CongestionController, Hysteria2Config, Node, ProtocolConfig,
};

use crate::common::{format_bandwidth, format_fragment, format_pins, format_query};
use crate::error::EmitError;

/// Emit a Hysteria2 [`Node`] as a `hysteria2://` share URI.
pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    let password = match &node.authentication {
        Authentication::Password { password } => password,
        _ => return Err(EmitError::MissingField("password authentication")),
    };

    let Hysteria2Config {
        ports,
        hop_interval,
        fast_open,
        lazy,
    } = match &node.config {
        ProtocolConfig::Hysteria2(cfg) => cfg,
        _ => return Err(EmitError::NoEmitter("non-Hysteria2 config".to_owned())),
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
                "insecure".to_owned(),
                if skip { "1" } else { "0" }.to_owned(),
            ));
        }
        if !tls.certificate_pins.is_empty() {
            params.push(("pinSHA256".to_owned(), format_pins(&tls.certificate_pins)));
        }
    }

    if let Some(ref obfs) = node.obfuscation {
        params.push(("obfs".to_owned(), obfs.kind.clone()));
        if let Some(ref obfs_pass) = obfs.password {
            params.push(("obfs-password".to_owned(), obfs_pass.clone()));
        }
    }

    if let Some(ref cong) = node.congestion {
        if let Some(up) = cong.up_bps {
            params.push(("up".to_owned(), format_bandwidth(up)));
        }
        if let Some(down) = cong.down_bps {
            params.push(("down".to_owned(), format_bandwidth(down)));
        }
        // WHY: Hysteria2 uses BBR by default. Only emit the controller name if
        // it differs from the default or was explicitly set to a non-BBR value.
        let controller_name = match &cong.controller {
            CongestionController::Bbr => None,
            CongestionController::Cubic => Some("cubic"),
            CongestionController::NewReno => Some("new_reno"),
            CongestionController::Other(name) => Some(name.as_str()),
        };
        if let Some(name) = controller_name {
            params.push(("congestion-controller".to_owned(), name.to_owned()));
        }
    }

    if let Some(p) = ports {
        params.push(("ports".to_owned(), p.clone()));
    }

    if let Some(hop) = hop_interval {
        let secs = hop.whole_seconds();
        if secs >= 0 {
            params.push(("hop_interval".to_owned(), secs.to_string()));
        }
    }

    if let Some(fo) = fast_open {
        params.push((
            "fast-open".to_owned(),
            if *fo { "1" } else { "0" }.to_owned(),
        ));
    }

    if let Some(lz) = lazy {
        params.push(("lazy".to_owned(), if *lz { "1" } else { "0" }.to_owned()));
    }

    let query = format_query(&params);

    let mut result = format!(
        "hysteria2://{password}@{host}:{port}?{query}",
        host = node.endpoint.host.uri_host(),
        port = node.endpoint.port,
    );
    if !node.display_name.is_empty() {
        result.push('#');
        result.push_str(&format_fragment(&node.display_name));
    }
    Ok(result)
}
