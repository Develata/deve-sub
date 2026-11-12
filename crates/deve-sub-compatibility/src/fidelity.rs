//! Native field constraints backed by the pinned client schemas.

use crate::{CompatibilityReason, ProfileKind};
use deve_sub_domain::{Node, ProtocolConfig, ProtocolKind, ShadowsocksPlugin};

pub(super) fn check(node: &Node, profile: ProfileKind) -> Result<(), CompatibilityReason> {
    let reject = |field| Err(CompatibilityReason::UnsupportedField(field));
    if !matches!(node.protocol, ProtocolKind::Socks5 | ProtocolKind::Http)
        && let Some(tls) = &node.tls
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
            && match profile {
                ProfileKind::Mihomo => {
                    native_tls
                        && tls.certificate_pins.len() == 1
                        && tls.certificate_pins[0]
                            .as_str()
                            .starts_with("mihomoFingerprintSHA256:")
                }
                ProfileKind::SingBox => {
                    native_tls
                        && tls
                            .certificate_pins
                            .iter()
                            .all(|p| p.as_str().starts_with("spkiSHA256:"))
                }
                ProfileKind::UriList | ProfileKind::Shadowrocket => {
                    matches!(
                        node.protocol,
                        ProtocolKind::Hysteria2 | ProtocolKind::NaiveProxy
                    ) && tls
                        .certificate_pins
                        .iter()
                        .all(|p| p.as_str().starts_with("pinSHA256:"))
                }
                _ => false,
            };
        if !supported {
            return reject("certificate pin representation");
        }
    }
    if profile == ProfileKind::SingBox
        && node.protocol == ProtocolKind::Hysteria2
        && node.congestion.as_ref().is_some_and(|c| {
            [c.up_bps, c.down_bps]
                .into_iter()
                .flatten()
                .any(|bps| bps % 1_000_000 != 0)
        })
    {
        return reject("sing-box requires whole Mbps bandwidth");
    }
    if profile == ProfileKind::Mihomo
        && let ProtocolConfig::TuicV5(config) = &node.config
        && config.heartbeat.is_some_and(|d| {
            d.is_negative()
                || d.subsec_nanoseconds() % 1_000_000 != 0
                || d.whole_milliseconds() > i128::from(i64::MAX)
        })
    {
        return reject("TUIC heartbeat requires non-negative integer milliseconds");
    }
    if matches!(
        profile,
        ProfileKind::Mihomo | ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray
    ) && let ProtocolConfig::Shadowsocks(config) = &node.config
    {
        if matches!(profile, ProfileKind::Xray | ProfileKind::V2Ray)
            && (config.plugin.is_some() || config.plugin_opts.is_some())
        {
            return reject("SIP003 plugins on Xray/V2Ray");
        }
        let plugin = ShadowsocksPlugin::from_config(config)
            .map_err(|_| CompatibilityReason::UnsupportedField("Shadowsocks plugin settings"))?;
        if profile == ProfileKind::Mihomo && plugin.is_some_and(|p| !p.has_explicit_host()) {
            return reject("Mihomo obfs requires an explicit camouflage host");
        }
    }
    Ok(())
}
