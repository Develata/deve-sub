//! Reject fields before native emission when the target cannot preserve them.

use deve_sub_domain::{Node, ProtocolConfig, ProtocolKind};

use crate::EmitError;

#[derive(Clone, Copy)]
pub(super) enum Target {
    Mihomo,
    SingBox,
    Xray,
}

pub(super) fn check(node: &Node, target: Target) -> Result<(), EmitError> {
    if let ProtocolConfig::ShadowTls(cfg) = &node.config {
        if !cfg.has_valid_password() {
            return Err(EmitError::NoEmitter(
                "ShadowTLS V2/V3 wrapper password".into(),
            ));
        }
        if node.tls.as_ref().is_some_and(|tls| tls.reality.is_some()) {
            return Err(EmitError::NoEmitter(
                "ShadowTLS Reality configuration".into(),
            ));
        }
        if matches!(target, Target::Mihomo) {
            if cfg.has_conflicting_inner_obfuscation(node.tls.as_ref()) {
                return Err(EmitError::NoEmitter(
                    "ShadowTLS conflicts with inner obfuscation".into(),
                ));
            }
            if matches!(
                cfg.inner_protocol,
                ProtocolKind::Shadowsocks | ProtocolKind::Snell
            ) && node.tls.as_ref().is_some_and(|tls| {
                !tls.enabled
                    || tls.skip_cert_verify.is_some()
                    || !tls.alpn.is_empty()
                    || tls.client_fingerprint.is_some()
                    || !tls.certificate_pins.is_empty()
            }) {
                return Err(EmitError::NoEmitter(
                    "Mihomo SS/Snell ShadowTLS extra TLS policy".into(),
                ));
            }
        }
    }
    if let Some(tls) = &node.tls
        && !tls.certificate_pins.is_empty()
    {
        let native_tls = matches!(
            node.protocol,
            ProtocolKind::Vless
                | ProtocolKind::VMess
                | ProtocolKind::Trojan
                | ProtocolKind::Hysteria2
                | ProtocolKind::TuicV5
                | ProtocolKind::AnyTls
        );
        let supported = tls.enabled
            && tls.reality.is_none()
            && native_tls
            && match target {
                Target::Mihomo => {
                    tls.certificate_pins.len() == 1
                        && tls.certificate_pins[0]
                            .as_str()
                            .starts_with("mihomoFingerprintSHA256:")
                }
                Target::SingBox => tls
                    .certificate_pins
                    .iter()
                    .all(|p| p.as_str().starts_with("spkiSHA256:")),
                Target::Xray => false,
            };
        // Forward proxy builders independently check their narrower TLS schema.
        if !supported && !matches!(node.protocol, ProtocolKind::Socks5 | ProtocolKind::Http) {
            return Err(EmitError::NoEmitter(
                "certificate pin representation".into(),
            ));
        }
    }
    if matches!(target, Target::SingBox)
        && node.protocol == ProtocolKind::Hysteria2
        && node.congestion.as_ref().is_some_and(|c| {
            [c.up_bps, c.down_bps]
                .into_iter()
                .flatten()
                .any(|bps| bps % 1_000_000 != 0)
        })
    {
        // WHY: native integer Mbps cannot encode a fractional Mbps value;
        // rounding to zero would select a different congestion-control default.
        return Err(EmitError::NoEmitter(
            "sing-box requires whole Mbps bandwidth".into(),
        ));
    }
    if let ProtocolConfig::Shadowsocks(config) = &node.config {
        if matches!(target, Target::Xray)
            && (config.plugin.is_some() || config.plugin_opts.is_some())
        {
            return Err(EmitError::NoEmitter("SIP003 plugins on Xray/V2Ray".into()));
        }
        let plugin = deve_sub_domain::ShadowsocksPlugin::from_config(config)
            .map_err(|error| EmitError::NoEmitter(error.to_string()))?;
        if matches!(target, Target::Mihomo) && plugin.is_some_and(|p| !p.has_explicit_host()) {
            return Err(EmitError::NoEmitter(
                "Mihomo obfs requires an explicit camouflage host".into(),
            ));
        }
    }
    Ok(())
}
