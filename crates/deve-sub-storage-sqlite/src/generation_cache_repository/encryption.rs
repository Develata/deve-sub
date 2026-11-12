//! The payload envelope is bound to its cache identity as well as its column.

use deve_sub_domain::TemplateError;
use deve_sub_security::{MasterKey, envelope};

fn context(id: &str, cache_key: &str) -> String {
    format!("generation_cache.content\0{id}\0{cache_key}")
}

pub(super) fn seal(
    key: &MasterKey,
    id: &str,
    cache_key: &str,
    content: &str,
) -> Result<String, TemplateError> {
    envelope::seal(
        key.as_bytes(),
        context(id, cache_key).as_bytes(),
        content.as_bytes(),
    )
    .map_err(|_| TemplateError::Storage("generation cache encryption failed".into()))
}

pub(super) fn open(
    key: &MasterKey,
    id: &str,
    cache_key: &str,
    encrypted: Option<&str>,
    legacy: &str,
) -> Result<String, TemplateError> {
    // WHY: storage version comes from the separate column, never from guessing
    // whether credential-bearing output resembles a valid envelope (ADR-0007).
    if !legacy.is_empty() {
        return Err(TemplateError::Storage(
            "generation cache requires keyed migration; run `deve-sub migrate --key-path`".into(),
        ));
    }
    let encrypted = encrypted.ok_or_else(|| {
        TemplateError::Storage("generation cache encrypted content is missing".into())
    })?;
    let bytes = envelope::open(key.as_bytes(), context(id, cache_key).as_bytes(), encrypted)
        .map_err(|_| TemplateError::Storage("generation cache decryption failed".into()))?;
    String::from_utf8(bytes)
        .map_err(|_| TemplateError::Storage("generation cache content is not UTF-8".into()))
}
