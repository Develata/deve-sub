//! Materialize native groups before emission, preserving policy and group names.
use super::{
    clash,
    clash_filter::{Budget, Matcher},
    error::TemplateAppError,
};
use deve_sub_domain::{Node, NodeFilter, NodePoolRepository, ProtocolConfig, ProtocolKind};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub(super) async fn materialize(
    config: &mut serde_json::Map<String, Value>,
    nodes: &[(Node, i64)],
    original_names: &[String],
    pool: &dyn NodePoolRepository,
    warnings: &mut Vec<String>,
) -> Result<(), TemplateAppError> {
    let groups = config
        .get("proxy-groups")
        .and_then(Value::as_array)
        .ok_or_else(|| clash::invalid("proxy-groups must be a list"))?;
    let mut types: HashMap<String, String> = nodes
        .iter()
        .map(|(n, _)| (n.display_name.clone(), adapter_type(n).to_owned()))
        .collect();
    for group in groups {
        let kind = match group["type"].as_str().unwrap_or_default() {
            "select" => "selector",
            "url-test" => "urltest",
            "load-balance" => "loadbalance",
            other => other,
        };
        types.insert(
            group["name"].as_str().unwrap_or_default().into(),
            kind.into(),
        );
    }
    for policy in clash::POLICIES {
        let kind = match *policy {
            "DIRECT" => "direct",
            "COMPATIBLE" => "compatible",
            "REJECT" => "reject",
            "REJECT-DROP" => "rejectdrop",
            "GLOBAL" => "selector",
            _ => "pass",
        };
        types.insert((*policy).into(), kind.into());
    }
    let wanted: HashSet<String> = groups
        .iter()
        .filter_map(|g| g["proxies"].as_array())
        .flatten()
        .filter_map(Value::as_str)
        .filter(|n| !types.contains_key(*n))
        .map(str::to_owned)
        .collect();
    let mut budget = Budget::new();
    // WHY: removing a same-name peer can restore a survivor's bare name.
    // Resolve its previous ID-qualified reference only within selected nodes.
    let mut aliases = HashMap::new();
    let mut unresolved = HashSet::new();
    if !wanted.is_empty() {
        let identities: HashMap<_, _> = nodes
            .iter()
            .zip(original_names)
            .map(|((node, _), original)| {
                (
                    node.id.to_string(),
                    (original.trim(), node.display_name.as_str()),
                )
            })
            .collect();
        for name in wanted {
            budget.visit(name.len())?;
            if let Some((base, id)) = qualified_name(&name)
                && let Some((original, current)) = identities.get(id)
                && base == *original
            {
                aliases.insert(name, *current);
            } else {
                unresolved.insert(name);
            }
        }
    }
    let unavailable = unavailable_names(pool, &unresolved, &mut budget).await?;
    let mut automatic: Vec<&str> = nodes.iter().map(|(n, _)| n.display_name.as_str()).collect();
    automatic.sort_unstable();
    let groups = config
        .get_mut("proxy-groups")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| clash::invalid("proxy-groups must be a list"))?;
    for group in groups {
        let name = group["name"].as_str().unwrap_or_default().to_owned();
        let include = Matcher::compile(group["filter"].as_str(), true, &mut budget)?;
        let exclude = Matcher::compile(group["exclude-filter"].as_str(), false, &mut budget)?;
        let excluded_types: HashSet<_> = group["exclude-type"]
            .as_str()
            .unwrap_or_default()
            .split('|')
            .map(str::to_ascii_lowercase)
            .collect();
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        for member in group["proxies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            budget.visit(member.len())?;
            let member = if let Some(current) = aliases.get(member) {
                warnings.push(format!(
                    "group '{name}': node reference '{member}' resolved to '{current}'"
                ));
                *current
            } else {
                member
            };
            if !types.contains_key(member) {
                if unavailable.contains(member) {
                    warnings.push(format!(
                        "group '{name}': unavailable node '{member}' removed"
                    ));
                    continue;
                }
                return Err(clash::invalid(format!(
                    "proxy-groups '{name}' references unavailable member '{member}'"
                )));
            }
            if seen.insert(member.to_owned()) {
                members.push(member.to_owned());
            }
        }
        if group["include-all"].as_bool() == Some(true)
            || group["include-all-proxies"].as_bool() == Some(true)
        {
            for member in &automatic {
                budget.visit(member.len())?;
                if include
                    .as_ref()
                    .map(|f| f.matches(member, &mut budget))
                    .transpose()?
                    .unwrap_or(true)
                    && seen.insert((*member).to_owned())
                {
                    members.push((*member).to_owned());
                }
            }
        }
        let mut filtered = Vec::with_capacity(members.len());
        for member in members {
            budget.visit(member.len())?;
            let excluded_type = types
                .get(&member)
                .is_some_and(|kind| excluded_types.contains(kind));
            if !excluded_type
                && !exclude
                    .as_ref()
                    .map(|f| f.matches(&member, &mut budget))
                    .transpose()?
                    .unwrap_or(false)
            {
                filtered.push(member);
            }
        }
        let empty = filtered.is_empty();
        if empty {
            filtered.push("REJECT".into());
            group["type"] = json!("select");
            warnings.push(format!(
                "group '{name}' has no available members; using REJECT"
            ));
        }
        if let Some(default) = group["default-selected"].as_str()
            && !filtered.iter().any(|member| member == default)
            && let Some(group) = group.as_object_mut()
        {
            group.remove("default-selected");
        }
        group["proxies"] = json!(filtered);
        // WHY: client filtering after materialization could remove REJECT and
        // reintroduce Mihomo's COMPATIBLE/DIRECT fallback (v1.19.0 groupbase).
        if let Some(group) = group.as_object_mut() {
            for key in [
                "filter",
                "exclude-filter",
                "exclude-type",
                "include-all",
                "include-all-proxies",
                "include-all-providers",
            ] {
                group.remove(key);
            }
        }
    }
    Ok(())
}

// Matches the formatter's ID suffix, including its secondary collision counter.
fn qualified_name(name: &str) -> Option<(&str, &str)> {
    let (base, qualified) = name.rsplit_once(" [")?;
    let (id, suffix) = qualified.split_once(']')?;
    if !suffix.is_empty() {
        let number = suffix.strip_prefix(' ')?;
        let counter = number.parse::<usize>().ok()?;
        if counter < 2 || counter.to_string() != number {
            return None;
        }
    }
    Some((base, id))
}

async fn unavailable_names(
    pool: &dyn NodePoolRepository,
    wanted: &HashSet<String>,
    budget: &mut Budget,
) -> Result<HashSet<String>, TemplateAppError> {
    let mut names = HashSet::new();
    if wanted.is_empty() {
        return Ok(names);
    }
    let mut qualified: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for name in wanted {
        budget.visit(name.len())?;
        if let Some((base, id)) = qualified_name(name) {
            qualified.entry(id).or_default().push((base, name));
        }
    }
    let mut cursor = None;
    loop {
        budget.visit(0)?;
        let entries = pool
            .list_nodes(&NodeFilter::all(), cursor, 1000)
            .await
            .map_err(|e| TemplateAppError::Storage(e.to_string()))?;
        if entries.is_empty() {
            break;
        }
        cursor = entries.last().map(|e| e.node.id);
        for entry in entries {
            budget.visit(entry.node.display_name.len())?;
            if entry.missing_from_source || !entry.is_active {
                if wanted.contains(&entry.node.display_name) {
                    names.insert(entry.node.display_name.clone());
                }
                if let Some(references) = qualified.get(entry.node.id.to_string().as_str()) {
                    for (base, name) in references {
                        budget.visit(name.len())?;
                        if *base == entry.node.display_name.trim() {
                            names.insert((*name).to_owned());
                        }
                    }
                }
            }
        }
        if names.len() == wanted.len() {
            break;
        }
    }
    Ok(names)
}

// WHY: exclude-type matches Mihomo AdapterType.String(), not the UI protocol
// key (v1.19.0 constant/adapters.go). ShadowTLS emits its inner adapter type.
fn adapter_type(node: &Node) -> &str {
    let kind = match &node.config {
        ProtocolConfig::ShadowTls(config) => &config.inner_protocol,
        _ => &node.protocol,
    };
    match kind {
        ProtocolKind::TuicV5 => "tuic",
        ProtocolKind::HysteriaV1 => "hysteria",
        _ => kind.as_filter_key(),
    }
}
