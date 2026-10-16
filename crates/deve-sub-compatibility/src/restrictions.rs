//! Field-level restrictions supplement the coarse protocol/transport matrix.
use crate::{CompatibilityReason, ProfileKind};
use deve_sub_domain::{Node, ProtocolConfig, ProtocolKind, SnellVersion};

pub(super) fn check(node: &Node, profile: ProfileKind) -> Result<(), CompatibilityReason> {
    if profile == ProfileKind::Json {
        return Ok(());
    }
    let reject = |field| Err(CompatibilityReason::UnsupportedField(field));
    if profile == ProfileKind::V2Ray && node.tls.as_ref().is_some_and(|t| t.reality.is_some()) {
        return reject("Reality security on V2Ray");
    }
    if let ProtocolConfig::Snell(cfg) = &node.config {
        if node.extras.contains_key("snell_userkey") && profile != ProfileKind::SingBox {
            return reject("Snell userkey");
        }
        if profile == ProfileKind::Mihomo {
            if cfg.version == SnellVersion::V6 {
                return Err(CompatibilityReason::UnsupportedProtocolVersion {
                    protocol: "snell",
                    version: 6,
                    supported: "1, 2, 3, 4, 5",
                });
            }
            if matches!(cfg.version, SnellVersion::V1 | SnellVersion::V2)
                && node.udp.supported == Some(true)
            {
                return reject("Snell v1/v2 UDP on Mihomo");
            }
        }
        if profile == ProfileKind::UriList
            && (cfg.v6_mode.is_some()
                || !cfg.tls_is_obfs_alias(node.tls.as_ref())
                || cfg.obfs.as_ref().is_some_and(|o| {
                    !matches!(
                        o.mode,
                        deve_sub_domain::SnellObfsMode::Http | deve_sub_domain::SnellObfsMode::Tls
                    ) || o.password.is_some()
                        || o.version.is_some()
                }))
        {
            return reject("Snell options without URI representation");
        }
        if profile == ProfileKind::SingBox {
            if !cfg.tls_is_obfs_alias(node.tls.as_ref()) {
                return reject("Snell TLS settings beyond camouflage");
            }
            if cfg.version == SnellVersion::V6 && cfg.obfs.is_some()
                || cfg.version != SnellVersion::V6 && cfg.v6_mode.is_some()
            {
                return reject("Snell version-specific options");
            }
            if cfg
                .obfs
                .as_ref()
                .is_some_and(|o| o.password.is_some() || o.version.is_some() || !o.alpn.is_empty())
            {
                return reject("Snell obfuscation options beyond mode and host");
            }
        }
    }
    if profile == ProfileKind::SingBox
        && let ProtocolConfig::WireGuard(cfg) = &node.config
    {
        if !cfg.dns.is_empty() {
            return reject("WireGuard interface DNS on sing-box");
        }
        if cfg.peers.iter().any(|p| {
            p.persistent_keepalive.is_some_and(|d| {
                d.whole_seconds() < 0
                    || d.whole_seconds() > i64::from(u16::MAX)
                    || d.subsec_nanoseconds() != 0
            })
        }) {
            return reject("WireGuard keepalive outside uint16 seconds");
        }
    }
    if let ProtocolConfig::ShadowTls(cfg) = &node.config {
        let supported = match profile {
            ProfileKind::Mihomo => matches!(
                cfg.inner_protocol,
                ProtocolKind::Shadowsocks
                    | ProtocolKind::Snell
                    | ProtocolKind::Vless
                    | ProtocolKind::Trojan
                    | ProtocolKind::VMess
                    | ProtocolKind::AnyTls
            ),
            ProfileKind::SingBox => matches!(
                cfg.inner_protocol,
                ProtocolKind::Shadowsocks
                    | ProtocolKind::Trojan
                    | ProtocolKind::VMess
                    | ProtocolKind::Vless
            ),
            _ => false,
        };
        if !supported || matches!(*cfg.inner_config, ProtocolConfig::Unsupported(_)) {
            return reject("ShadowTLS incomplete or unsupported inner protocol");
        }
        let mut inner = node.clone();
        inner.protocol = cfg.inner_protocol.clone();
        inner.config = (*cfg.inner_config).clone();
        inner.tls = None;
        crate::check_node(&inner, &crate::capability_for(profile))?;
    }
    Ok(())
}
