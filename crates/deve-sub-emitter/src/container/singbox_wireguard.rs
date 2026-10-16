//! WireGuard is an endpoint in sing-box 1.14, not a legacy outbound.
use crate::error::EmitError;
use deve_sub_domain::{Host, Node, ProtocolConfig};
use serde_json::{Value, json};

pub(super) fn emit(node: &Node) -> Result<Value, EmitError> {
    let ProtocolConfig::WireGuard(cfg) = &node.config else {
        return Err(EmitError::MissingField("wireguard config"));
    };
    if !cfg.dns.is_empty() {
        return Err(EmitError::NoEmitter(
            "sing-box WireGuard endpoint cannot preserve interface DNS".into(),
        ));
    }
    let address = match &node.endpoint.host {
        Host::Ipv6(ip) => ip.to_string(),
        host => host.uri_host(),
    };
    let peers = cfg
        .peers
        .iter()
        .map(|peer| {
            let mut value = json!({
                "address": address, "port": node.endpoint.port,
                "public_key": peer.public_key, "allowed_ips": peer.allowed_ips,
            });
            if let Some(psk) = &peer.pre_shared_key {
                value["pre_shared_key"] = json!(psk)
            }
            if let Some(reserved) = peer.reserved {
                value["reserved"] = json!(reserved)
            }
            if let Some(interval) = peer.persistent_keepalive {
                let seconds = u16::try_from(interval.whole_seconds()).map_err(|_| {
                    EmitError::NoEmitter("WireGuard keepalive must fit uint16 seconds".into())
                })?;
                if interval.subsec_nanoseconds() != 0 {
                    return Err(EmitError::NoEmitter(
                        "WireGuard keepalive must be whole seconds".into(),
                    ));
                }
                value["persistent_keepalive_interval"] = json!(seconds);
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>, EmitError>>()?;
    let mut value = json!({
        "type": "wireguard", "tag": node.display_name, "private_key": cfg.private_key,
        "address": cfg.address, "peers": peers,
    });
    if let Some(mtu) = cfg.mtu {
        value["mtu"] = json!(mtu)
    }
    if let Some(workers) = cfg.workers {
        value["workers"] = json!(workers)
    }
    Ok(value)
}
