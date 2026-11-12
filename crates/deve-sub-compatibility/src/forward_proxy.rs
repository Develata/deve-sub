//! Field-level forward proxy representability for the pinned target schemas.

use crate::{CompatibilityReason, ProfileKind};
use deve_sub_domain::{Node, ProtocolConfig, ProtocolKind, SocksDnsResolution};

pub(super) fn check(node: &Node, profile: ProfileKind) -> Result<(), CompatibilityReason> {
    if !matches!(node.protocol, ProtocolKind::Socks5 | ProtocolKind::Http) {
        return Ok(());
    }
    let reject = |field| Err(CompatibilityReason::UnsupportedField(field));
    let socks = matches!(node.config, ProtocolConfig::Socks5(_));
    if socks != (node.protocol == ProtocolKind::Socks5)
        || !matches!(
            node.config,
            ProtocolConfig::Socks5(_) | ProtocolConfig::Http(_)
        )
    {
        return Err(CompatibilityReason::UnsupportedConfig);
    }
    if node.transport.is_some()
        || node.multiplex.is_some()
        || node.obfuscation.is_some()
        || node.congestion.is_some()
        || node.udp.xudp.is_some()
    {
        return reject("forward proxy transport extensions");
    }
    if let ProtocolConfig::Socks5(cfg) = &node.config {
        if profile != ProfileKind::UriList && cfg.dns_resolution != SocksDnsResolution::Unspecified
        {
            return reject("explicit SOCKS destination DNS policy");
        }
        if matches!(profile, ProfileKind::Xray | ProfileKind::V2Ray) && node.udp.supported.is_some()
        {
            return reject("explicit SOCKS UDP policy on Xray/V2Ray");
        }
    }
    if let ProtocolConfig::Http(cfg) = &node.config {
        if node.udp.supported == Some(true) {
            return reject("HTTP CONNECT UDP");
        }
        if matches!(
            profile,
            ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray
        ) && node.udp.supported.is_some()
        {
            return reject("explicit HTTP UDP policy without a target representation");
        }
        if profile != ProfileKind::SingBox && cfg.path.is_some() {
            return reject("HTTP CONNECT path");
        }
        if matches!(
            profile,
            ProfileKind::UriList | ProfileKind::Shadowrocket | ProfileKind::V2Ray
        ) && !cfg.headers.is_empty()
        {
            return reject("HTTP headers without a target representation");
        }
        if matches!(profile, ProfileKind::Mihomo | ProfileKind::Xray)
            && cfg.headers.values().any(|values| values.len() != 1)
        {
            return reject("repeated HTTP header values on Mihomo/Xray");
        }
    }
    if let Some(tls) = &node.tls {
        if !tls.enabled
            && (tls.server_name.is_some()
                || tls.skip_cert_verify.is_some()
                || !tls.alpn.is_empty()
                || tls.client_fingerprint.is_some()
                || !tls.certificate_pins.is_empty()
                || tls.reality.is_some())
        {
            return reject("TLS parameters on a plaintext forward proxy");
        }
        if profile == ProfileKind::V2Ray && tls.client_fingerprint.is_some() {
            return reject("TLS fingerprint on V2Ray v4 JSON");
        }
        if tls.reality.is_some() {
            return reject("forward proxy Reality");
        }
        if socks && profile == ProfileKind::SingBox {
            return reject("SOCKS TLS on sing-box");
        }
        if profile == ProfileKind::Mihomo
            && (!tls.alpn.is_empty()
                || tls.client_fingerprint.is_some()
                || socks && tls.server_name.is_some())
        {
            return reject("TLS field absent from Mihomo forward proxy schema");
        }
        let pins = &tls.certificate_pins;
        let supported = match profile {
            ProfileKind::Mihomo => {
                pins.len() <= 1
                    && pins
                        .iter()
                        .all(|p| p.as_str().starts_with("mihomoFingerprintSHA256:"))
            }
            ProfileKind::SingBox => pins.iter().all(|p| p.as_str().starts_with("spkiSHA256:")),
            ProfileKind::UriList => pins.iter().all(|p| {
                p.as_str().starts_with("pinSHA256:") || p.as_str().starts_with("spkiSHA256:")
            }),
            _ => pins.is_empty(),
        };
        if !supported {
            return reject("certificate pin representation");
        }
    }
    Ok(())
}
