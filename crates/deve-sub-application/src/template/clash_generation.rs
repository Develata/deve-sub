//! Resolve the shared Mihomo proxy/group namespace without mutating pool names.

use super::{clash, error::TemplateAppError};
use deve_sub_domain::{Node, TemplateSpec};
use std::collections::HashSet;

pub(super) fn prepare_nodes(
    nodes: &mut [(Node, i64)],
    spec: &TemplateSpec,
) -> Result<(), TemplateAppError> {
    let native = spec.clash.as_deref().map(clash::config).transpose()?;
    let mut used: HashSet<String> = clash::POLICIES.iter().map(|s| (*s).to_owned()).collect();
    used.extend(spec.proxy_groups.iter().map(|g| g.name.clone()));
    if let Some(config) = &native {
        used.extend(
            config
                .get("proxy-groups")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|g| g.get("name").and_then(serde_json::Value::as_str))
                .map(str::to_owned),
        );
    }
    let mut counts = std::collections::HashMap::new();
    for (node, _) in nodes.iter() {
        *counts.entry(node.display_name.clone()).or_insert(0) += 1;
    }
    // Reserve uncontested names before assigning suffixes, so results do not
    // depend on traversal order and a generated suffix cannot steal a real name.
    let rename: Vec<bool> = nodes
        .iter()
        .map(|(node, _)| {
            node.display_name.trim().is_empty()
                || used.contains(&node.display_name)
                || counts[&node.display_name] > 1
        })
        .collect();
    for ((node, _), rename) in nodes.iter().zip(&rename) {
        if !rename {
            used.insert(node.display_name.clone());
        }
    }
    for ((node, _), rename) in nodes.iter_mut().zip(rename) {
        if rename {
            let base = format!("{} [{}]", node.display_name.trim(), node.id);
            let mut candidate = base.clone();
            let mut suffix = 2;
            while !used.insert(candidate.clone()) {
                candidate = format!("{base} {suffix}");
                suffix += 1;
            }
            node.display_name = candidate;
        }
    }
    if let Some(config) = &native {
        let names = nodes
            .iter()
            .map(|(node, _)| node.display_name.as_str())
            .collect();
        clash::validate(config, Some(&names))?;
    }
    Ok(())
}
