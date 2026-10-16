//! Profile compatibility matrix for target output formats.
//!
//! Each target profile (Mihomo, sing-box, Xray, V2Ray, Shadowrocket,
//! `uri_list`) has a [`ProfileCapability`] describing which protocols,
//! transports, and group types it supports. [`check_node`] tests whether a
//! canonical node is compatible with a profile; incompatible nodes are
//! excluded from generation and reported with a [`CompatibilityReason`]
//! (constraint #7: no silent dropping).
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Compatibility
//! matrix".

#![cfg_attr(test, allow(clippy::expect_used))]

use std::collections::HashSet;

use deve_sub_domain::{GroupType, Node, ProtocolKind, SnellObfsMode, SnellVersion, TransportKind};
use thiserror::Error;

mod restrictions;

/// Target output profile identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileKind {
    Mihomo,
    SingBox,
    Xray,
    V2Ray,
    Shadowrocket,
    UriList,
    /// Canonical JSON serialization of the full node model. Full-fidelity
    /// profile: accepts all protocols and transports (see M9 Slice 5).
    Json,
}

impl ProfileKind {
    /// Parse a profile identifier from its kebab-case string.
    #[must_use]
    pub fn from_kebab(s: &str) -> Option<Self> {
        match s {
            "mihomo" => Some(Self::Mihomo),
            "sing-box" => Some(Self::SingBox),
            "xray" => Some(Self::Xray),
            "v2ray" => Some(Self::V2Ray),
            "shadowrocket" => Some(Self::Shadowrocket),
            "uri_list" => Some(Self::UriList),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// Return the kebab-case string for this profile.
    #[must_use]
    pub fn as_kebab(&self) -> &'static str {
        match self {
            Self::Mihomo => "mihomo",
            Self::SingBox => "sing-box",
            Self::Xray => "xray",
            Self::V2Ray => "v2ray",
            Self::Shadowrocket => "shadowrocket",
            Self::UriList => "uri_list",
            Self::Json => "json",
        }
    }
}

/// The capability set of a single target profile.
#[derive(Debug, Clone)]
pub struct ProfileCapability {
    pub profile: ProfileKind,
    pub supported_protocols: HashSet<ProtocolKind>,
    pub supported_transports: HashSet<TransportKind>,
    pub chain_support: bool,
    pub supported_group_types: HashSet<GroupType>,
}

/// Why a node or group is incompatible with a profile.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CompatibilityReason {
    #[error("unsupported field: {0}")]
    UnsupportedField(&'static str),
    #[error("unsupported protocol '{0}' for profile")]
    UnsupportedProtocol(String),
    #[error("unsupported transport '{0}' for profile")]
    UnsupportedTransport(String),
    #[error("unsupported group type '{0}' for profile")]
    UnsupportedGroupType(String),
    #[error("node has no typed protocol config (unsupported)")]
    UnsupportedConfig,
    #[error("unsupported protocol version '{version}' for {protocol}; supported: {supported}")]
    UnsupportedProtocolVersion {
        protocol: &'static str,
        version: u32,
        supported: &'static str,
    },
    // WHY: sing-box snell V4 supports only http/tls obfs modes
    // (option/snell.go); shadow-tls/restls/jls are mihomo-only projections.
    // Without this check the node passes compatibility but kills the entire
    // generation when the sing-box emitter returns `NoEmitter` (constraint #7:
    // no silent dropping of incompatible nodes).
    #[error("unsupported obfs mode '{mode}' for {protocol} on {profile}; supported: {supported}")]
    UnsupportedObfsMode {
        protocol: &'static str,
        profile: &'static str,
        mode: &'static str,
        supported: &'static str,
    },
}

/// Look up the capability matrix for a profile.
#[must_use]
pub fn capability_for(profile: ProfileKind) -> ProfileCapability {
    match profile {
        ProfileKind::Mihomo => ProfileCapability {
            profile,
            supported_protocols: [
                ProtocolKind::Vless,
                ProtocolKind::VMess,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
                ProtocolKind::Hysteria2,
                ProtocolKind::TuicV5,
                ProtocolKind::WireGuard,
                ProtocolKind::AnyTls,
                ProtocolKind::Snell,
                ProtocolKind::ShadowTls,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
                TransportKind::HttpUpgrade,
                TransportKind::Xhttp,
            ]
            .into_iter()
            .collect(),
            chain_support: true,
            supported_group_types: [
                GroupType::Select,
                GroupType::UrlTest,
                GroupType::Fallback,
                GroupType::LoadBalance,
                GroupType::Relay,
            ]
            .into_iter()
            .collect(),
        },
        ProfileKind::SingBox => ProfileCapability {
            profile,
            supported_protocols: [
                ProtocolKind::Vless,
                ProtocolKind::VMess,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
                ProtocolKind::Hysteria2,
                ProtocolKind::TuicV5,
                ProtocolKind::WireGuard,
                ProtocolKind::AnyTls,
                ProtocolKind::Snell,
                ProtocolKind::ShadowTls,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
                TransportKind::HttpUpgrade,
            ]
            .into_iter()
            .collect(),
            chain_support: true,
            supported_group_types: [GroupType::Select, GroupType::UrlTest, GroupType::Fallback]
                .into_iter()
                .collect(),
        },
        ProfileKind::Xray => ProfileCapability {
            profile,
            supported_protocols: [
                ProtocolKind::Vless,
                ProtocolKind::VMess,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
                ProtocolKind::WireGuard,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
                TransportKind::HttpUpgrade,
                TransportKind::Xtls,
                TransportKind::Xhttp,
            ]
            .into_iter()
            .collect(),
            chain_support: false,
            supported_group_types: [GroupType::Select].into_iter().collect(),
        },
        ProfileKind::V2Ray => ProfileCapability {
            profile,
            supported_protocols: [
                ProtocolKind::VMess,
                ProtocolKind::Vless,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
            ]
            .into_iter()
            .collect(),
            chain_support: false,
            supported_group_types: [GroupType::Select].into_iter().collect(),
        },
        ProfileKind::Shadowrocket => ProfileCapability {
            profile,
            supported_protocols: [
                ProtocolKind::Vless,
                ProtocolKind::VMess,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
                ProtocolKind::Hysteria2,
                ProtocolKind::TuicV5,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
            ]
            .into_iter()
            .collect(),
            chain_support: false,
            supported_group_types: [GroupType::Select, GroupType::UrlTest]
                .into_iter()
                .collect(),
        },
        ProfileKind::UriList => ProfileCapability {
            profile,
            supported_protocols: [
                ProtocolKind::Vless,
                ProtocolKind::VMess,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
                ProtocolKind::Hysteria2,
                ProtocolKind::TuicV5,
                ProtocolKind::NaiveProxy,
                ProtocolKind::WireGuard,
                ProtocolKind::AnyTls,
                ProtocolKind::Snell,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
                TransportKind::HttpUpgrade,
                TransportKind::Kcp,
                TransportKind::Xtls,
                TransportKind::Xhttp,
            ]
            .into_iter()
            .collect(),
            chain_support: false,
            supported_group_types: HashSet::new(),
        },
        ProfileKind::Json => ProfileCapability {
            profile,
            // WHY: JSON profile is full-fidelity — it serializes the
            // canonical Node model verbatim via serde, so every protocol
            // and transport is accepted. No filtering, no exclusion.
            supported_protocols: [
                ProtocolKind::Vless,
                ProtocolKind::VMess,
                ProtocolKind::Trojan,
                ProtocolKind::Shadowsocks,
                ProtocolKind::Hysteria2,
                ProtocolKind::TuicV5,
                ProtocolKind::NaiveProxy,
                ProtocolKind::WireGuard,
                ProtocolKind::AnyTls,
                ProtocolKind::Snell,
                ProtocolKind::ShadowTls,
            ]
            .into_iter()
            .collect(),
            supported_transports: [
                TransportKind::Tcp,
                TransportKind::Ws,
                TransportKind::H2,
                TransportKind::Grpc,
                TransportKind::Quic,
                TransportKind::HttpUpgrade,
                TransportKind::Kcp,
                TransportKind::Xtls,
                TransportKind::Xhttp,
            ]
            .into_iter()
            .collect(),
            chain_support: false,
            supported_group_types: HashSet::new(),
        },
    }
}

/// Check whether a node is compatible with a profile.
///
/// Returns `Ok(())` if the node's protocol, transport, and config are all
/// supported by the profile, or `Err(CompatibilityReason)` explaining why
/// the node must be excluded.
pub fn check_node(node: &Node, cap: &ProfileCapability) -> Result<(), CompatibilityReason> {
    if !cap.supported_protocols.contains(&node.protocol) {
        return Err(CompatibilityReason::UnsupportedProtocol(
            node.protocol.to_string(),
        ));
    }

    if matches!(node.config, deve_sub_domain::ProtocolConfig::Unsupported(_)) {
        return Err(CompatibilityReason::UnsupportedConfig);
    }

    if let Some(ref transport) = node.transport
        && !cap.supported_transports.contains(&transport.kind)
    {
        return Err(CompatibilityReason::UnsupportedTransport(
            transport.kind.to_string(),
        ));
    }

    restrictions::check(node, cap.profile)?;

    // WHY: sing-box supports Snell V4 and V6 only; V1/V2/V3/V5 must be
    // excluded with a version-specific reason (constraint #7 + M9 §Failure/
    // recovery). Mihomo version restrictions are checked separately; URI output
    // preserves the authored version or reject Snell entirely via `supported_protocols` (xray,
    // v2ray, shadowrocket).
    if cap.profile == ProfileKind::SingBox
        && let deve_sub_domain::ProtocolConfig::Snell(cfg) = &node.config
    {
        match cfg.version {
            SnellVersion::V4 | SnellVersion::V6 => {}
            other => {
                return Err(CompatibilityReason::UnsupportedProtocolVersion {
                    protocol: "snell",
                    version: other.as_u32(),
                    supported: "4, 6",
                });
            }
        }
        // WHY: sing-box snell V4 supports only http/tls obfs modes
        // (option/snell.go); shadow-tls/restls/jls are mihomo-only
        // projections. Rejecting here reports the node as incompatible
        // instead of letting the sing-box emitter return `NoEmitter` and
        // kill the entire generation (constraint #7: no silent dropping).
        if cfg.version == SnellVersion::V4
            && let Some(ref obfs) = cfg.obfs
            && !matches!(obfs.mode, SnellObfsMode::Http | SnellObfsMode::Tls)
        {
            let mode_str = match obfs.mode {
                SnellObfsMode::ShadowTls => "shadow-tls",
                SnellObfsMode::Restls => "restls",
                SnellObfsMode::Jls => "jls",
                SnellObfsMode::Http | SnellObfsMode::Tls => unreachable!(),
            };
            return Err(CompatibilityReason::UnsupportedObfsMode {
                protocol: "snell",
                profile: "sing-box",
                mode: mode_str,
                supported: "http, tls",
            });
        }
    }

    Ok(())
}

/// Check whether a group type is compatible with a profile.
pub fn check_group_type(
    group_type: GroupType,
    cap: &ProfileCapability,
) -> Result<(), CompatibilityReason> {
    if cap.supported_group_types.is_empty() {
        return Ok(());
    }
    if cap.supported_group_types.contains(&group_type) {
        Ok(())
    } else {
        Err(CompatibilityReason::UnsupportedGroupType(
            group_type.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
