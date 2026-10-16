//! Reject Snell input settings that cannot survive typed normalization.
use crate::ParseError;
use serde_json::Value;

pub(super) fn validate(entry: &Value) -> Result<(), ParseError> {
    let invalid = |field| ParseError::InvalidField {
        field,
        value: "unsupported or malformed Snell option".into(),
    };
    let fields = entry.as_object().ok_or_else(|| invalid("snell"))?;
    for key in fields.keys() {
        // Dialer/network constraints have no canonical equivalent. Preserve the
        // raw entry as Unsupported instead of emitting a broader connection.
        if ![
            "type",
            "tag",
            "server",
            "server_port",
            "psk",
            "version",
            "reuse",
            "obfs_mode",
            "obfs_host",
            "userkey",
            "mode",
            "tls",
        ]
        .contains(&key.as_str())
        {
            return Err(invalid("snell options"));
        }
    }
    for key in ["obfs_mode", "obfs_host", "userkey", "mode"] {
        if entry.get(key).is_some_and(|v| !v.is_string()) {
            return Err(invalid(key));
        }
    }
    if entry.get("reuse").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("reuse"));
    }
    let v6 = entry["version"].as_u64() == Some(6);
    if (v6 && (entry.get("obfs_mode").is_some() || entry.get("obfs_host").is_some()))
        || (!v6 && entry.get("mode").is_some())
    {
        return Err(invalid("version-specific settings"));
    }
    if entry.get("obfs_host").is_some()
        && !matches!(entry["obfs_mode"].as_str(), Some("http" | "tls"))
    {
        return Err(invalid("obfs_host without camouflage"));
    }
    if let Some(tls) = entry.get("tls") {
        // Legacy imports may mirror simple camouflage SNI in TLS. Only that
        // exact alias is representable by the current official Snell schema.
        let tls = tls.as_object().ok_or_else(|| invalid("tls"))?;
        if entry["obfs_mode"] != "tls"
            || tls
                .keys()
                .any(|k| !["enabled", "server_name"].contains(&k.as_str()))
            || tls
                .get("enabled")
                .is_some_and(|v| v.as_bool() != Some(true))
            || tls
                .get("server_name")
                .is_some_and(|v| Some(v) != entry.get("obfs_host"))
        {
            return Err(invalid("TLS beyond camouflage"));
        }
    }
    Ok(())
}
