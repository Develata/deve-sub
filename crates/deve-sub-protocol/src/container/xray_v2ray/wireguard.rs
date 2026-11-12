//! Xray WireGuard mapping.
use super::super::{get_str, get_str_array, node_shell_container};
use super::extract_tag;
use crate::error::ParseError;
use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, WireGuardConfig, WireGuardPeer,
};
use serde_json::Value;

pub(super) fn parse_wireguard(entry: &Value) -> Result<Node, ParseError> {
    let name = extract_tag(entry);
    let settings = entry
        .get("settings")
        .ok_or(ParseError::MissingField("settings"))?;

    let secret_key = get_str(settings, "secretKey").ok_or(ParseError::MissingField("secretKey"))?;

    let address = get_str_array(settings, "address");

    let mtu = settings
        .get("mtu")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(0));

    let reserved = settings
        .get("reserved")
        .and_then(|r| r.as_array())
        .and_then(|arr| {
            if arr.len() == 3 {
                let mut bytes = [0u8; 3];
                for (i, v) in arr.iter().enumerate() {
                    bytes[i] = v.as_u64()?.try_into().ok()?;
                }
                Some(bytes)
            } else {
                None
            }
        });

    let peers_arr = settings
        .get("peers")
        .and_then(|p| p.as_array())
        .ok_or(ParseError::MissingField("peers"))?;

    let peers: Vec<WireGuardPeer> = peers_arr
        .iter()
        .map(|p| {
            let public_key =
                get_str(p, "publicKey").ok_or(ParseError::MissingField("peer publicKey"))?;
            let pre_shared_key = get_str(p, "preSharedKey");
            let allowed_ips = get_str_array(p, "allowedIPs");
            let keepalive = p
                .get("keepAlive")
                .and_then(|v| v.as_u64())
                .map(|secs| time::Duration::seconds(i64::try_from(secs).unwrap_or(0)));
            Ok(WireGuardPeer {
                public_key,
                pre_shared_key,
                allowed_ips,
                reserved,
                persistent_keepalive: keepalive,
            })
        })
        .collect::<Result<Vec<_>, ParseError>>()?;

    let (host, port) = peers_arr
        .first()
        .and_then(|p| {
            let endpoint = get_str(p, "endpoint")?;
            crate::uri::parse_host_port(&endpoint).ok()
        })
        .ok_or(ParseError::MissingField("peer endpoint"))?;

    let config = ProtocolConfig::WireGuard(WireGuardConfig {
        private_key: secret_key,
        address,
        peers,
        mtu,
        workers: None,
        dns: vec![],
    });

    let mut node = node_shell_container();
    node.display_name = name;
    node.protocol = ProtocolKind::WireGuard;
    node.config = config;
    node.endpoint = Endpoint { host, port };
    node.authentication = Authentication::None;
    Ok(node)
}
