//! Native import conveniences are structural tokens, never text substitutions.
use super::{clash, error::TemplateAppError};
use serde_json::{Map, Value};

pub(super) const NODES: &str = "__PROXY_NODES__";
pub(super) const PROVIDERS: &str = "__PROXY_PROVIDERS__";
pub(super) const TOKENS: &[&str] = &[NODES, PROVIDERS];

pub(super) fn validate(config: &Map<String, Value>) -> Result<(), TemplateAppError> {
    if let Some(mode) = config.get("mode")
        && mode.as_str() != Some("rule")
    {
        return Err(clash::invalid("native Clash mode must be rule"));
    }
    if let Some(proxies) = config.get("proxies")
        && !proxies.is_null()
        && !proxies.as_array().is_some_and(Vec::is_empty)
    {
        return Err(clash::invalid(
            "native Clash proxies must be null or an empty list; nodes come from the node pool",
        ));
    }
    Ok(())
}

/// Recompute input-only notices on cache hits without scanning the node pool.
pub(super) fn warnings(native: Option<&str>) -> Result<Vec<String>, TemplateAppError> {
    let Some(native) = native else {
        return Ok(Vec::new());
    };
    let config = clash::config(native)?;
    let mut warnings = Vec::new();
    for group in config["proxy-groups"].as_array().into_iter().flatten() {
        if group["include-all-providers"].as_bool() == Some(true)
            || group["proxies"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v.as_str() == Some(PROVIDERS))
        {
            let name = group["name"].as_str().unwrap_or_default();
            warnings.push(format!(
                "group '{name}': external proxy providers are not configured; provider placeholder and include-all-providers expand to an empty set"
            ));
        }
    }
    Ok(warnings)
}
