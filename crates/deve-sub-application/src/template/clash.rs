//! Native Clash input is validated server-side and retained without lossy V3 conversion.

use std::collections::{HashMap, HashSet};

use deve_sub_domain::{API_VERSION, KIND, TemplateDocument};
use serde_json::{Map, Value, json};

use super::error::TemplateAppError;

pub(super) const POLICIES: &[&str] = &[
    "DIRECT",
    "REJECT",
    "REJECT-DROP",
    "PASS",
    "COMPATIBLE",
    "GLOBAL",
];

pub(super) fn invalid(message: impl Into<String>) -> TemplateAppError {
    TemplateAppError::InvalidInput(message.into())
}

pub(super) fn parse(raw: serde_yaml::Value) -> Result<TemplateDocument, TemplateAppError> {
    let mut value = raw;
    if value.is_sequence() {
        let mut map = serde_yaml::Mapping::new();
        map.insert(serde_yaml::Value::String("rules".into()), value);
        value = serde_yaml::Value::Mapping(map);
    }
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| invalid("expected Clash routing YAML with rules, or a YAML rule list"))?;
    let groups = serde_yaml::Value::String("proxy-groups".into());
    if !map.contains_key(&groups) {
        map.insert(
            groups,
            serde_yaml::to_value(json!([{
                "name": "PROXY", "type": "select", "include-all-proxies": true
            }]))
            .map_err(|e| invalid(e.to_string()))?,
        );
    }
    // JSON is a temporary validation view only. The retained YAML mapping
    // preserves DNS policy insertion order through storage and emission.
    let native = serde_yaml::to_string(&value).map_err(|e| invalid(e.to_string()))?;
    let map = config(&native)?;
    validate(&map, None)?;
    serde_json::from_value(json!({
        "apiVersion": API_VERSION, "kind": KIND,
        "metadata": {"name": "clash-routing"},
        "spec": {"targetProfiles": ["mihomo"], "nodeSelector": {"mode": "dynamic"}, "clash": native}
    }))
    .map_err(|e| invalid(e.to_string()))
}

pub(super) fn config(yaml: &str) -> Result<Map<String, Value>, TemplateAppError> {
    let raw = super::validation::checked_yaml(yaml)?;
    let value = serde_json::to_value(raw).map_err(|e| invalid(e.to_string()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("native Clash configuration must be a mapping"))
}

/// With no nodes, concrete group members are checked when generation resolves the pool.
pub(super) fn validate(
    config: &Map<String, Value>,
    nodes: Option<&HashSet<&str>>,
) -> Result<(), TemplateAppError> {
    for key in config.keys() {
        if !["proxy-groups", "rules", "rule-providers", "dns", "tun"].contains(&key.as_str()) {
            return Err(invalid(format!(
                "unsupported Clash section '{key}'; use routing sections only, nodes come from the node pool"
            )));
        }
    }
    for key in ["dns", "tun", "rule-providers"] {
        if let Some(value) = config.get(key)
            && !value.is_object()
        {
            return Err(invalid(format!("{key} must be a mapping")));
        }
    }
    let groups = config
        .get("proxy-groups")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("proxy-groups must be a list"))?;
    let mut names = HashSet::new();
    for group in groups {
        let name = required_string(group, "name")?;
        if name.trim().is_empty()
            || name.contains([',', '\n', '\r'])
            || POLICIES.contains(&name)
            || !names.insert(name)
        {
            return Err(invalid(format!(
                "proxy-groups: duplicate, reserved or invalid group name '{name}'"
            )));
        }
        let kind = required_string(group, "type")?;
        if !["select", "url-test", "fallback", "load-balance"].contains(&kind) {
            return Err(invalid(format!(
                "proxy-groups '{name}': unsupported type '{kind}'"
            )));
        }
        for key in [
            "include-all",
            "include-all-proxies",
            "include-all-providers",
            "lazy",
            "disable-udp",
            "hidden",
        ] {
            if let Some(value) = group.get(key)
                && !value.is_boolean()
            {
                return Err(invalid(format!(
                    "proxy-groups '{name}': {key} must be a boolean"
                )));
            }
        }
        for key in ["interval", "timeout", "max-failed-times", "tolerance"] {
            if let Some(value) = group.get(key)
                && value.as_u64().is_none()
            {
                return Err(invalid(format!(
                    "proxy-groups '{name}': {key} must be a nonnegative integer"
                )));
            }
        }
        for key in [
            "url",
            "filter",
            "exclude-filter",
            "exclude-type",
            "strategy",
            "icon",
        ] {
            if let Some(value) = group.get(key)
                && !value.is_string()
            {
                return Err(invalid(format!(
                    "proxy-groups '{name}': {key} must be a string"
                )));
            }
        }
        if group.get("use").is_some()
            || group.get("include-all-providers").and_then(Value::as_bool) == Some(true)
        {
            return Err(invalid(format!(
                "proxy-groups '{name}': external proxy providers are not supported; use include-all-proxies"
            )));
        }
    }
    let mut edges = HashMap::new();
    for group in groups {
        let name = required_string(group, "name")?;
        let members = strings(group.get("proxies"), "proxy-groups.proxies")?;
        if members.is_empty()
            && !["include-all", "include-all-proxies"]
                .iter()
                .any(|key| group.get(key).and_then(Value::as_bool) == Some(true))
        {
            return Err(invalid(format!(
                "proxy-groups '{name}' has no members; add proxies or include-all-proxies: true"
            )));
        }
        if let Some(nodes) = nodes {
            for member in &members {
                if !names.contains(member) && !POLICIES.contains(member) && !nodes.contains(member)
                {
                    return Err(invalid(format!(
                        "proxy-groups '{name}' references unavailable member '{member}'"
                    )));
                }
            }
        }
        edges.insert(
            name,
            members
                .into_iter()
                .filter(|member| names.contains(member))
                .collect::<Vec<_>>(),
        );
    }
    // Iterative topological elimination bounds stack use for long user-authored chains.
    let mut indegree: HashMap<&str, usize> = names.iter().map(|name| (*name, 0)).collect();
    for members in edges.values() {
        for member in members {
            if let Some(degree) = indegree.get_mut(member) {
                *degree += 1;
            }
        }
    }
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(*name))
        .collect();
    let mut visited = 0;
    while let Some(name) = ready.pop() {
        visited += 1;
        for member in &edges[name] {
            if let Some(degree) = indegree.get_mut(member) {
                *degree -= 1;
                if *degree == 0 {
                    ready.push(member);
                }
            }
        }
    }
    if visited != names.len() {
        return Err(invalid("proxy-groups contains a cycle"));
    }
    let providers = config.get("rule-providers").and_then(Value::as_object);
    super::clash_rules::validate_providers(providers)?;
    let rules = config
        .get("rules")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("rules must be a list of Clash rule strings"))?;
    if rules.is_empty() {
        return Err(invalid("rules must not be empty"));
    }
    for (index, rule) in rules.iter().enumerate() {
        let rule = rule
            .as_str()
            .ok_or_else(|| invalid(format!("rules[{}] must be a string", index + 1)))?;
        super::clash_rules::validate_rule(rule, &names, providers)
            .map_err(|e| invalid(format!("rules[{}]: {e}", index + 1)))?;
    }
    Ok(())
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, TemplateAppError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("proxy-groups.{field} must be a string")))
}

fn strings<'a>(value: Option<&'a Value>, field: &str) -> Result<Vec<&'a str>, TemplateAppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| invalid(format!("{field} must be a list")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid(format!("{field} entries must be strings")))
        })
        .collect()
}
