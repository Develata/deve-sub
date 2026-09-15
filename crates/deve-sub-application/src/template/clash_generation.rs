//! Resolve the shared Mihomo proxy/group namespace without mutating pool names.

use super::{clash, error::TemplateAppError};
use deve_sub_domain::{Node, NodePoolRepository, TemplateSpec};
use std::collections::HashSet;

pub(super) async fn prepare_nodes(
    nodes: &mut [(Node, i64)],
    spec: &mut TemplateSpec,
    pool: &dyn NodePoolRepository,
    warnings: &mut Vec<String>,
) -> Result<(), TemplateAppError> {
    let native = spec.clash.as_deref().map(clash::config).transpose()?;
    let original_names: Vec<_> = if native.is_some() {
        nodes
            .iter()
            .map(|(node, _)| node.display_name.clone())
            .collect()
    } else {
        Vec::new()
    };
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
    if let Some(mut config) = native {
        super::clash_membership::materialize(&mut config, nodes, &original_names, pool, warnings)
            .await?;
        let names = nodes
            .iter()
            .map(|(node, _)| node.display_name.as_str())
            .collect();
        clash::validate(&config, Some(&names))?;
        // WHY: preserve author ordering, especially DNS nameserver-policy precedence.
        let mut native =
            super::validation::checked_yaml(spec.clash.as_deref().unwrap_or_default())?;
        native["proxy-groups"] = serde_yaml::to_value(&config["proxy-groups"])
            .map_err(|e| TemplateAppError::Emit(e.to_string()))?;
        spec.clash = Some(
            serde_yaml::to_string(&native).map_err(|e| TemplateAppError::Emit(e.to_string()))?,
        );
    }
    Ok(())
}
