//! The lossless SIP003 option subset understood by native configuration targets.
//!
//! URI storage retains arbitrary plugin text. Native conversion uses this
//! typed view so an unknown flag cannot be silently ignored by a target core.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::ShadowsocksConfig;

/// Supported SIP003 semantics, independent of the target's field spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowsocksPlugin {
    /// simple-obfs HTTP or TLS camouflage; absent host remains absent.
    Obfs { mode: String, host: Option<String> },
    /// The shared WebSocket subset of v2ray-plugin.
    WebSocket {
        host: String,
        path: String,
        tls: bool,
        multiplex: bool,
    },
}

/// Plugin syntax or semantics without a verified native mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("unsupported Shadowsocks plugin setting: {0}")]
pub struct PluginError(pub &'static str);

impl ShadowsocksPlugin {
    /// Decode the verified native subset without changing stored URI text.
    ///
    /// Escaped separators, duplicate keys, unknown keys, QUIC, and implicit
    /// WebSocket host defaults are deliberately rejected. Client defaults differ.
    pub fn from_config(config: &ShadowsocksConfig) -> Result<Option<Self>, PluginError> {
        let Some(name) = config.plugin.as_deref() else {
            return if config.plugin_opts.is_some() {
                Err(PluginError("options without a plugin"))
            } else {
                Ok(None)
            };
        };
        let mut opts = options(config.plugin_opts.as_deref().unwrap_or(""))?;
        let result = match name {
            "obfs-local" => {
                let mode = opts.remove("obfs").unwrap_or_else(|| "http".into());
                if !matches!(mode.as_str(), "http" | "tls") {
                    return Err(PluginError("obfs mode"));
                }
                Self::Obfs {
                    mode,
                    host: opts.remove("obfs-host"),
                }
            }
            "v2ray-plugin" => {
                let mode = opts.remove("mode").unwrap_or_else(|| "websocket".into());
                if mode != "websocket" {
                    return Err(PluginError("v2ray-plugin mode"));
                }
                let host = opts
                    .remove("host")
                    .filter(|h| !h.is_empty())
                    .ok_or(PluginError("v2ray-plugin requires an explicit host"))?;
                let path = opts.remove("path").unwrap_or_else(|| "/".into());
                // WHY: SIP003 v2ray-plugin tests flag presence, including
                // `tls=false`; converting that spelling to false is a downgrade.
                let tls = opts.remove("tls").is_some();
                let multiplex = match opts.remove("mux").as_deref() {
                    None | Some("1") => true,
                    Some("0") => false,
                    _ => return Err(PluginError("v2ray-plugin mux")),
                };
                Self::WebSocket {
                    host,
                    path,
                    tls,
                    multiplex,
                }
            }
            _ => return Err(PluginError("name")),
        };
        if !opts.is_empty() {
            return Err(PluginError("unknown option"));
        }
        Ok(Some(result))
    }

    /// Emit a SIP003 name and options for the typed subset.
    #[must_use]
    pub fn sip003(&self) -> (&'static str, String) {
        match self {
            Self::Obfs { mode, host } => {
                let mut opts = format!("obfs={mode}");
                if let Some(host) = host {
                    opts.push_str(&format!(";obfs-host={host}"));
                }
                ("obfs-local", opts)
            }
            Self::WebSocket {
                host,
                path,
                tls,
                multiplex,
            } => {
                let mut opts = format!(
                    "mode=websocket;host={host};path={path};mux={}",
                    u8::from(*multiplex)
                );
                if *tls {
                    opts.push_str(";tls");
                }
                ("v2ray-plugin", opts)
            }
        }
    }

    /// Whether the plugin's absent-host semantics survive a target that fills
    /// an implicit camouflage host.
    #[must_use]
    pub fn has_explicit_host(&self) -> bool {
        !matches!(self, Self::Obfs { host, .. } if host.as_ref().is_none_or(String::is_empty))
    }
}

fn options(text: &str) -> Result<BTreeMap<String, String>, PluginError> {
    if text.contains('\\') {
        return Err(PluginError("escaped option separators"));
    }
    let mut options = BTreeMap::new();
    for pair in text.split(';').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key.is_empty() || options.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(PluginError("empty or duplicate option"));
        }
    }
    Ok(options)
}
