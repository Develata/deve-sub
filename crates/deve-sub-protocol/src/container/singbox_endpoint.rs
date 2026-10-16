//! Current sing-box endpoints, with explicit rejection of unrepresentable data.
use super::{get_str, node_shell_container, parse_host_str, unsupported_entry};
use crate::ParseError;
use base64::Engine;
use deve_sub_domain::{
    Authentication, Endpoint, Node, ProtocolConfig, ProtocolKind, WireGuardConfig, WireGuardPeer,
};
use serde_json::Value;

pub(super) fn parse(entry: &Value) -> Node {
    let kind = get_str(entry, "type").unwrap_or_default();
    if kind != "wireguard" {
        return unsupported_entry(
            entry,
            "singbox-json",
            ProtocolKind::Unknown(kind),
            "unsupported endpoint type".into(),
        );
    }
    wireguard(entry).unwrap_or_else(|error| {
        unsupported_entry(
            entry,
            "singbox-json",
            ProtocolKind::WireGuard,
            error.to_string(),
        )
    })
}

/// Accept the old outbound schema while refusing peer endpoint collapse.
pub(super) fn parse_legacy(entry: &Value) -> Result<Node, ParseError> {
    let mut mapped = entry.clone();
    let object = mapped
        .as_object_mut()
        .ok_or_else(|| invalid("wireguard outbound"))?;
    let server = object.remove("server");
    let port = object.remove("server_port");
    if let Some(address) = object.remove("local_address")
        && object.insert("address".into(), address).is_some()
    {
        return Err(invalid("address aliases"));
    }
    let peers = object
        .get_mut("peers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("peers"))?;
    for peer in peers {
        let peer = peer.as_object_mut().ok_or_else(|| invalid("peer"))?;
        for (old, new, fallback) in [
            ("server", "address", &server),
            ("server_port", "port", &port),
        ] {
            if let Some(value) = peer.remove(old).or_else(|| fallback.clone())
                && peer.insert(new.into(), value).is_some()
            {
                return Err(invalid("peer endpoint aliases"));
            }
        }
    }
    wireguard(&mapped)
}

fn invalid(field: &'static str) -> ParseError {
    ParseError::InvalidField {
        field,
        value: "invalid or not representable by the canonical WireGuard model".into(),
    }
}

fn strings(value: Option<&Value>, field: &'static str) -> Result<Vec<String>, ParseError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| invalid(field)))
            .collect(),
        _ => Err(invalid(field)),
    }
}

fn prefixes(value: Option<&Value>, field: &'static str) -> Result<Vec<String>, ParseError> {
    let values = strings(value, field)?;
    for value in &values {
        let (ip, bits) = value.split_once('/').ok_or_else(|| invalid(field))?;
        let ip = ip.parse::<std::net::IpAddr>().map_err(|_| invalid(field))?;
        let bits = bits.parse::<u8>().map_err(|_| invalid(field))?;
        if bits > if ip.is_ipv4() { 32 } else { 128 } {
            return Err(invalid(field));
        }
    }
    Ok(values)
}

fn key(value: &Value, field: &'static str) -> Result<String, ParseError> {
    let key = get_str(value, field).ok_or_else(|| invalid(field))?;
    if base64::engine::general_purpose::STANDARD
        .decode(&key)
        .map_or(true, |bytes| bytes.len() != 32)
    {
        return Err(invalid(field));
    }
    Ok(key)
}

fn uint(value: Option<&Value>, field: &'static str, max: u64) -> Result<Option<u64>, ParseError> {
    value
        .map(|v| {
            v.as_u64()
                .filter(|n| *n <= max)
                .ok_or_else(|| invalid(field))
        })
        .transpose()
}

fn wireguard(entry: &Value) -> Result<Node, ParseError> {
    let object = entry.as_object().ok_or_else(|| invalid("endpoint"))?;
    for key in object.keys() {
        if ![
            "type",
            "tag",
            "system",
            "name",
            "listen_port",
            "mtu",
            "address",
            "private_key",
            "peers",
            "workers",
        ]
        .contains(&key.as_str())
        {
            return Err(invalid("endpoint options"));
        }
    }
    if entry
        .get("system")
        .is_some_and(|v| v.as_bool() != Some(false))
        || entry.get("name").is_some_and(|v| v.as_str() != Some(""))
        || uint(entry.get("listen_port"), "listen_port", u16::MAX.into())?.is_some_and(|v| v != 0)
    {
        return Err(invalid("interface/listener options"));
    }
    let private_key = key(entry, "private_key")?;
    let address = prefixes(entry.get("address"), "address")?;
    if address.is_empty() {
        return Err(invalid("address"));
    }
    let raw_peers = entry
        .get("peers")
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| invalid("peers"))?;
    let mut endpoint = None;
    let mut peers = Vec::with_capacity(raw_peers.len());
    for peer in raw_peers {
        for key in peer.as_object().ok_or_else(|| invalid("peer"))?.keys() {
            if ![
                "address",
                "port",
                "public_key",
                "pre_shared_key",
                "allowed_ips",
                "persistent_keepalive_interval",
                "reserved",
            ]
            .contains(&key.as_str())
            {
                return Err(invalid("peer options"));
            }
        }
        let peer_endpoint = Endpoint {
            host: parse_host_str(
                &get_str(peer, "address").ok_or(ParseError::MissingField("peer.address"))?,
            )?,
            port: uint(peer.get("port"), "peer.port", u16::MAX.into())?
                .filter(|p| *p != 0)
                .ok_or_else(|| invalid("peer.port"))? as u16,
        };
        // WHY: the current model has one endpoint per node, not per peer.
        if endpoint.as_ref().is_some_and(|e| e != &peer_endpoint) {
            return Err(invalid("distinct peer endpoints"));
        }
        endpoint = Some(peer_endpoint);
        let reserved = peer
            .get("reserved")
            .map(|v| {
                let a = v
                    .as_array()
                    .filter(|a| a.len() == 3)
                    .ok_or_else(|| invalid("reserved"))?;
                let mut bytes = [0; 3];
                for (index, value) in a.iter().enumerate() {
                    bytes[index] = uint(Some(value), "reserved", u8::MAX.into())?
                        .ok_or_else(|| invalid("reserved"))?
                        as u8;
                }
                Ok(bytes)
            })
            .transpose()?;
        let keepalive = uint(
            peer.get("persistent_keepalive_interval"),
            "persistent_keepalive_interval",
            u16::MAX.into(),
        )?;
        if peer.get("pre_shared_key").is_some_and(|v| !v.is_string()) {
            return Err(invalid("pre_shared_key"));
        }
        peers.push(WireGuardPeer {
            public_key: key(peer, "public_key")?,
            pre_shared_key: peer
                .get("pre_shared_key")
                .map(|_| key(peer, "pre_shared_key"))
                .transpose()?,
            allowed_ips: prefixes(peer.get("allowed_ips"), "allowed_ips")?,
            reserved,
            persistent_keepalive: keepalive.map(|s| time::Duration::seconds(s as i64)),
        });
    }
    let mut node = node_shell_container();
    node.display_name = get_str(entry, "tag").unwrap_or_else(|| "wireguard".into());
    node.protocol = ProtocolKind::WireGuard;
    node.endpoint = endpoint.ok_or_else(|| invalid("peers"))?;
    node.authentication = Authentication::None;
    node.config = ProtocolConfig::WireGuard(WireGuardConfig {
        private_key,
        address,
        peers,
        dns: vec![],
        mtu: uint(entry.get("mtu"), "mtu", u32::MAX.into())?.map(|n| n as u32),
        workers: uint(entry.get("workers"), "workers", u32::MAX.into())?.map(|n| n as u32),
    });
    Ok(node)
}
