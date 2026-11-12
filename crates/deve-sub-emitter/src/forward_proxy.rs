//! Forward proxy share URIs, preserving destination DNS intent and TLS.

use deve_sub_domain::{Authentication, Node, ProtocolConfig, SocksDnsResolution};

use crate::{
    EmitError,
    common::{encode_userinfo, format_fragment, format_query},
};

pub(crate) fn emit(node: &Node) -> Result<String, EmitError> {
    validate_tls_state(node)?;
    let tls_enabled = node.tls.as_ref().is_some_and(|tls| tls.enabled);
    let scheme = match &node.config {
        ProtocolConfig::Socks5(cfg) => match cfg.dns_resolution {
            SocksDnsResolution::Unspecified => "socks",
            SocksDnsResolution::Local => "socks5",
            SocksDnsResolution::Remote => "socks5h",
        },
        ProtocolConfig::Http(cfg) if cfg.path.is_none() && cfg.headers.is_empty() => {
            if tls_enabled { "https" } else { "http" }
        }
        _ => return Err(unsupported("proxy URI cannot preserve this configuration")),
    };
    if node.transport.is_some()
        || node.multiplex.is_some()
        || node.obfuscation.is_some()
        || node.congestion.is_some()
        || node.udp.xudp.is_some()
    {
        return Err(unsupported(
            "proxy URI cannot preserve transport extensions",
        ));
    }
    let auth = match &node.authentication {
        Authentication::None => String::new(),
        Authentication::UserPassword { username, password } => format!(
            "{}:{}@",
            encode_userinfo(username),
            encode_userinfo(password)
        ),
        _ => return Err(unsupported("proxy authentication")),
    };
    let mut params = Vec::new();
    if let Some(tls) = &node.tls {
        if tls.reality.is_some() {
            return Err(unsupported("forward proxy Reality"));
        }
        if matches!(node.config, ProtocolConfig::Socks5(_)) || !tls.enabled {
            params.push(("tls".into(), u8::from(tls.enabled).to_string()));
        }
        if let Some(value) = &tls.server_name {
            params.push(("sni".into(), value.clone()));
        }
        if let Some(value) = tls.skip_cert_verify {
            params.push(("skip-cert-verify".into(), u8::from(value).to_string()));
        }
        if !tls.alpn.is_empty() {
            params.push(("alpn".into(), tls.alpn.join(",")));
        }
        if let Some(value) = &tls.client_fingerprint {
            params.push(("fp".into(), value.clone()));
        }
        for (prefix, key) in [("pinSHA256:", "pinSHA256"), ("spkiSHA256:", "spkiSHA256")] {
            let pins: Vec<_> = tls
                .certificate_pins
                .iter()
                .filter_map(|p| p.as_str().strip_prefix(prefix))
                .collect();
            if !pins.is_empty() {
                params.push((key.into(), pins.join(",")));
            }
        }
        if tls.certificate_pins.iter().any(|p| {
            !p.as_str().starts_with("pinSHA256:") && !p.as_str().starts_with("spkiSHA256:")
        }) {
            return Err(unsupported("unknown certificate pin type"));
        }
    }
    if let Some(value) = node.udp.supported {
        params.push(("udp".into(), u8::from(value).to_string()));
    }
    let mut uri = format!(
        "{scheme}://{auth}{}:{}",
        node.endpoint.host.uri_host(),
        node.endpoint.port
    );
    let query = format_query(&params);
    if !query.is_empty() {
        uri.push('?');
        uri.push_str(&query);
    }
    if !node.display_name.is_empty() {
        uri.push('#');
        uri.push_str(&format_fragment(&node.display_name));
    }
    Ok(uri)
}

/// Prevent disabled TLS from silently discarding explicitly supplied security fields.
pub(crate) fn validate_tls_state(node: &Node) -> Result<(), EmitError> {
    if let Some(tls) = &node.tls
        && !tls.enabled
        && (tls.server_name.is_some()
            || tls.skip_cert_verify.is_some()
            || !tls.alpn.is_empty()
            || tls.client_fingerprint.is_some()
            || !tls.certificate_pins.is_empty()
            || tls.reality.is_some())
    {
        return Err(unsupported("TLS parameters on a plaintext forward proxy"));
    }
    Ok(())
}

pub(crate) fn unsupported(reason: &str) -> EmitError {
    EmitError::InvalidField {
        field: "forward proxy",
        value: reason.to_owned(),
    }
}
