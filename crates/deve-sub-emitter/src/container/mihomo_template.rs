//! Mihomo routing container assembly; node protocol encoding stays in mihomo.

use super::ir::AssembledTemplate;
use super::mihomo::yaml_dq;
use crate::error::EmitError;
use deve_sub_domain::GroupType;
use std::collections::BTreeMap;

pub fn emit_full(template: &AssembledTemplate) -> Result<String, EmitError> {
    if let Some(config) = &template.clash {
        // WHY: application validation already bounded the authored template.
        // This server-serialized config also contains expanded group members,
        // so its generated size must not inherit the authored input limits.
        let mut native = deve_sub_yaml::parse_generated_value(config)
            .map_err(|e| EmitError::Encode(e.to_string()))?;
        let mapping = native
            .as_mapping_mut()
            .ok_or_else(|| EmitError::Encode("native routing template must be a mapping".into()))?;
        mapping.shift_remove(&deve_sub_yaml::Value::String("proxies".into()));
        // WHY: the pool owns proxies. Append its known root block directly:
        // reparsing generated output would impose input limits on large node
        // pools and duplicate the whole proxy tree. Skip an empty native map
        // because a leading `{}` would form a separate YAML root value.
        let mut output = if mapping.is_empty() {
            String::new()
        } else {
            deve_sub_yaml::to_string(&native).map_err(|e| EmitError::Encode(e.to_string()))?
        };
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&super::mihomo::emit(&template.nodes)?);
        if !output.ends_with('\n') {
            output.push('\n');
        }
        return Ok(output);
    }
    let mut out = super::mihomo::emit(&template.nodes)?;

    if !template.groups.is_empty() {
        out.push('\n');
        emit_groups(&template.groups, &mut out)?;
    }

    if !template.rules.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        emit_rules(&template.rules, &mut out)?;
    }

    if !template.dns.is_null() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        emit_json_block("dns", &template.dns, &mut out)?;
    }

    if !template.tun.is_null() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        emit_json_block("tun", &template.tun, &mut out)?;
    }

    Ok(out)
}

fn emit_groups(
    groups: &[crate::container::ir::AssembledGroup],
    out: &mut String,
) -> Result<(), EmitError> {
    out.push_str("proxy-groups:");
    for g in groups {
        let type_str = match g.group_type {
            GroupType::Select => "select",
            GroupType::UrlTest => "url-test",
            GroupType::Fallback => "fallback",
            GroupType::LoadBalance => "load-balance",
            GroupType::Relay => "relay",
            GroupType::Direct => "direct",
            GroupType::Reject => "reject",
        };
        out.push_str("\n  - name: ");
        out.push_str(&yaml_dq(&g.name));
        out.push_str("\n    type: ");
        out.push_str(type_str);
        if !g.members.is_empty() {
            out.push_str("\n    proxies:");
            for m in &g.members {
                out.push_str("\n      - ");
                out.push_str(&yaml_dq(m));
            }
        }
    }
    Ok(())
}

fn emit_rules(rules: &[serde_json::Value], out: &mut String) -> Result<(), EmitError> {
    let yaml = deve_sub_yaml::to_string(&BTreeMap::from([("rules", rules)]))
        .map_err(|e| EmitError::Encode(e.to_string()))?;
    out.push_str(&yaml);
    Ok(())
}

fn emit_json_block(
    key: &str,
    value: &serde_json::Value,
    out: &mut String,
) -> Result<(), EmitError> {
    // WHY: serialize the section root together with its value. Trimming or
    // re-indenting a serialized child can remove meaningful trailing newlines
    // from `|+` scalars; borrowed wrappers avoid cloning the JSON tree.
    let yaml = deve_sub_yaml::to_string(&BTreeMap::from([(key, value)]))
        .map_err(|e| EmitError::Encode(e.to_string()))?;
    out.push_str(&yaml);
    Ok(())
}
