//! Structural rule checks preserve native strings and comma-containing logical expressions.

use super::clash::{POLICIES, invalid};
use super::error::TemplateAppError;
use serde_json::{Map, Value};
use std::collections::HashSet;

pub(super) fn validate_rule(
    rule: &str,
    groups: &HashSet<&str>,
    providers: Option<&Map<String, Value>>,
) -> Result<(), TemplateAppError> {
    if rule.contains(['\n', '\r', '\0']) {
        return Err(invalid("rule must be a single line"));
    }
    // Parentheses in regexes and process paths are payload, not logic delimiters.
    let kind = rule.split(',').next().unwrap_or_default().trim();
    let parts = if matches!(kind, "AND" | "OR" | "NOT") {
        split(rule)?
    } else {
        rule.split(',').map(str::trim).collect()
    };
    let kind = parts.first().copied().unwrap_or_default();
    let policy_index = if kind == "MATCH" { 1 } else { 2 };
    let policy = parts
        .get(policy_index)
        .ok_or_else(|| invalid("missing rule payload or policy"))?;
    if !POLICIES.contains(policy) && !groups.contains(policy) {
        return Err(invalid(format!(
            "unknown policy '{policy}'; use a proxy group or built-in policy"
        )));
    }
    if kind == "MATCH" {
        if parts.len() != 2 {
            return Err(invalid("MATCH requires exactly one policy"));
        }
    } else {
        validate_condition(kind, parts[1], providers, 0)?;
        if parts[3..]
            .iter()
            .any(|flag| !["no-resolve", "src"].contains(flag))
        {
            return Err(invalid(
                "unsupported rule flag (expected no-resolve or src)",
            ));
        }
    }
    Ok(())
}

fn validate_condition(
    kind: &str,
    payload: &str,
    providers: Option<&Map<String, Value>>,
    depth: usize,
) -> Result<(), TemplateAppError> {
    if depth > 10 {
        return Err(invalid("logical rule nesting exceeds 10"));
    }
    if payload.is_empty() {
        return Err(invalid("empty rule payload"));
    }
    match kind {
        "AND" | "OR" | "NOT" => {
            let inner = unwrap_parentheses(payload)?;
            let children = split(inner)?;
            if kind == "NOT" && children.len() != 1 {
                return Err(invalid("NOT requires one condition"));
            }
            for child in children {
                let fields = split(unwrap_parentheses(child)?)?;
                if fields.len() < 2
                    || fields[2..]
                        .iter()
                        .any(|flag| !["no-resolve", "src"].contains(flag))
                {
                    return Err(invalid("invalid logical condition"));
                }
                validate_condition(fields[0], fields[1], providers, depth + 1)?;
            }
        }
        "RULE-SET" => {
            if !providers.is_some_and(|map| map.contains_key(payload)) {
                return Err(invalid(format!("unknown rule-provider '{payload}'")));
            }
        }
        "IP-CIDR" | "IP-CIDR6" | "SRC-IP-CIDR" => {
            let (ip, prefix) = payload
                .split_once('/')
                .ok_or_else(|| invalid("CIDR requires address/prefix"))?;
            let ip: std::net::IpAddr = ip.parse().map_err(|_| invalid("invalid CIDR address"))?;
            let prefix: u8 = prefix.parse().map_err(|_| invalid("invalid CIDR prefix"))?;
            if prefix > if ip.is_ipv4() { 32 } else { 128 } {
                return Err(invalid("CIDR prefix is out of range"));
            }
        }
        "NETWORK" => {
            if !matches!(payload, "tcp" | "udp" | "TCP" | "UDP") {
                return Err(invalid("NETWORK must be tcp or udp"));
            }
        }
        "DOMAIN"
        | "DOMAIN-SUFFIX"
        | "DOMAIN-KEYWORD"
        | "DOMAIN-WILDCARD"
        | "DOMAIN-REGEX"
        | "GEOSITE"
        | "IP-SUFFIX"
        | "IP-ASN"
        | "GEOIP"
        | "SRC-GEOIP"
        | "SRC-IP-ASN"
        | "SRC-IP-SUFFIX"
        | "DST-PORT"
        | "SRC-PORT"
        | "IN-PORT"
        | "IN-TYPE"
        | "IN-USER"
        | "IN-NAME"
        | "PROCESS-PATH"
        | "PROCESS-PATH-WILDCARD"
        | "PROCESS-PATH-REGEX"
        | "PROCESS-NAME"
        | "PROCESS-NAME-WILDCARD"
        | "PROCESS-NAME-REGEX"
        | "UID"
        | "DSCP" => {}
        _ => return Err(invalid(format!("unsupported rule type '{kind}'"))),
    }
    Ok(())
}

fn unwrap_parentheses(value: &str) -> Result<&str, TemplateAppError> {
    value
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| invalid("logical conditions require parentheses"))
}

fn split(value: &str) -> Result<Vec<&str>, TemplateAppError> {
    let mut depth = 0_u32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (offset, ch) in value.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                if depth > 32 {
                    return Err(invalid("rule parentheses nesting exceeds 32"));
                }
            }
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("unbalanced rule parentheses"))?
            }
            ',' if depth == 0 => {
                parts.push(value[start..offset].trim());
                start = offset + 1;
            }
            '\n' | '\r' | '\0' => return Err(invalid("rule must be a single line")),
            _ => {}
        }
    }
    if depth != 0 {
        return Err(invalid("unbalanced rule parentheses"));
    }
    parts.push(value[start..].trim());
    Ok(parts)
}

pub(super) fn validate_providers(
    providers: Option<&Map<String, Value>>,
) -> Result<(), TemplateAppError> {
    let Some(providers) = providers else {
        return Ok(());
    };
    for (name, provider) in providers {
        if !provider.is_object() {
            return Err(invalid(format!("rule-providers.{name} must be a mapping")));
        }
        let kind = provider.get("type").and_then(Value::as_str);
        if !matches!(kind, Some("http" | "file" | "inline")) {
            return Err(invalid(format!(
                "rule-providers.{name}.type must be http, file or inline"
            )));
        }
        if !matches!(
            provider.get("behavior").and_then(Value::as_str),
            Some("domain" | "ipcidr" | "classical")
        ) {
            return Err(invalid(format!(
                "rule-providers.{name}.behavior must be domain, ipcidr or classical"
            )));
        }
        let field = match kind {
            Some("http") => "url",
            Some("file") => "path",
            _ => "payload",
        };
        let valid = if field == "payload" {
            provider
                .get(field)
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().all(Value::is_string))
        } else {
            provider
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
        };
        if !valid {
            return Err(invalid(format!(
                "rule-providers.{name}.{field} is missing or invalid"
            )));
        }
    }
    Ok(())
}
