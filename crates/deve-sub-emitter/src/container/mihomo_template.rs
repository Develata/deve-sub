//! Mihomo routing container assembly; node protocol encoding stays in mihomo.

use super::ir::AssembledTemplate;
use super::mihomo::yaml_dq;
use crate::error::EmitError;
use deve_sub_domain::GroupType;

pub fn emit_full(template: &AssembledTemplate) -> Result<String, EmitError> {
    if let Some(config) = &template.clash {
        let mut output: serde_yaml::Mapping =
            serde_yaml::from_str(config).map_err(|e| EmitError::Encode(e.to_string()))?;
        let proxies: serde_yaml::Value =
            serde_yaml::from_str(&super::mihomo::emit(&template.nodes)?)
                .map_err(|e| EmitError::Encode(e.to_string()))?;
        output.insert(
            serde_yaml::Value::String("proxies".into()),
            proxies["proxies"].clone(),
        );
        return serde_yaml::to_string(&output).map_err(|e| EmitError::Encode(e.to_string()));
    }
    let mut out = super::mihomo::emit(&template.nodes)?;

    if !template.groups.is_empty() {
        out.push('\n');
        emit_groups(&template.groups, &mut out)?;
    }

    if !template.rules.is_empty() {
        out.push('\n');
        emit_rules(&template.rules, &mut out)?;
    }

    if !template.dns.is_null() {
        out.push('\n');
        emit_json_block("dns", &template.dns, &mut out)?;
    }

    if !template.tun.is_null() {
        out.push('\n');
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
    out.push_str("rules:");
    for rule in rules {
        let yaml = json_to_yaml_line(rule)?;
        for (i, l) in yaml.lines().enumerate() {
            if i == 0 {
                out.push_str("\n  - ");
                out.push_str(l);
            } else {
                out.push_str("\n    ");
                out.push_str(l);
            }
        }
    }
    Ok(())
}

fn emit_json_block(
    key: &str,
    value: &serde_json::Value,
    out: &mut String,
) -> Result<(), EmitError> {
    let yaml =
        serde_yaml::to_string(value).map_err(|e| EmitError::Encode(format!("serde_yaml: {e}")))?;
    // WHY: serde_yaml emits a leading "---\n" document marker and column-0
    // content; we strip the marker and re-indent under the section key.
    let body = yaml
        .strip_prefix("---\n")
        .unwrap_or(&yaml)
        .trim_end_matches('\n');
    out.push_str(key);
    out.push(':');
    for l in body.lines() {
        out.push_str("\n  ");
        out.push_str(l);
    }
    Ok(())
}

fn json_to_yaml_line(value: &serde_json::Value) -> Result<String, EmitError> {
    let yaml =
        serde_yaml::to_string(value).map_err(|e| EmitError::Encode(format!("serde_yaml: {e}")))?;
    Ok(yaml
        .strip_prefix("---\n")
        .unwrap_or(&yaml)
        .trim_end()
        .to_owned())
}
