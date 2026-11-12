//! Native plugin field names and types for the verified SIP003 subset.

use deve_sub_domain::{ShadowsocksConfig, ShadowsocksPlugin};

use super::mihomo::yaml_dq;
use crate::EmitError;

pub(super) fn push(config: &ShadowsocksConfig, entry: &mut String) -> Result<(), EmitError> {
    let Some(plugin) = ShadowsocksPlugin::from_config(config)
        .map_err(|error| EmitError::NoEmitter(error.to_string()))?
    else {
        return Ok(());
    };
    match plugin {
        ShadowsocksPlugin::Obfs { mode, host } => {
            let host = host.filter(|h| !h.is_empty()).ok_or_else(|| {
                EmitError::NoEmitter("Mihomo obfs requires an explicit camouflage host".into())
            })?;
            entry.push_str(&format!(
                "\n    plugin: \"obfs\"\n    plugin-opts:\n      mode: {}\n      host: {}",
                yaml_dq(&mode),
                yaml_dq(&host)
            ));
        }
        ShadowsocksPlugin::WebSocket {
            host,
            path,
            tls,
            multiplex,
        } => {
            entry.push_str(&format!("\n    plugin: \"v2ray-plugin\"\n    plugin-opts:\n      mode: \"websocket\"\n      host: {}\n      path: {}\n      tls: {tls}\n      mux: {multiplex}", yaml_dq(&host), yaml_dq(&path)));
        }
    }
    Ok(())
}
