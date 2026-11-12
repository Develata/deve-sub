//! Read-only preconditions shared by explicit upgrade and staged restore.
use anyhow::{Context, Result, bail};
use deve_sub_security::MasterKey;
use sqlx::SqlitePool;

use crate::backup::database;

pub(crate) async fn has_rows(pool: &SqlitePool, table: &str) -> Result<bool> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
    )
    .bind(table)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Ok(false);
    }
    Ok(
        sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM {})", quoted(table)))
            .fetch_one(pool)
            .await?,
    )
}

pub(crate) async fn needs_key(pool: &SqlitePool) -> Result<bool> {
    Ok(has_rows(pool, "key_metadata").await?
        || has_rows(pool, "generation_cache").await?
        || has_keyed_state(pool).await?)
}

pub(crate) async fn verify(pool: &SqlitePool, key: Option<&MasterKey>) -> Result<i64> {
    let version = database::current_schema_version(pool).await?;
    database::verify_archived_schema(pool, version).await?;
    if version < 15 {
        for table in ["sources", "nodes", "source_items", "node_source_bindings"] {
            if has_rows(pool, table).await? {
                bail!(
                    "populated pre-0015 sensitive data requires a separately reviewed conversion; schema and data were not upgraded"
                );
            }
        }
    }
    let fingerprint = key.map(MasterKey::fingerprint).transpose()?;
    database::verify_key_binding(pool, fingerprint.as_deref(), None).await?;
    reject_unverified_key_state(pool).await?;
    if has_rows(pool, "generation_cache").await? && key.is_none() {
        bail!("generation cache upgrade requires the original master key; provide --key-path");
    }
    Ok(version)
}

/// Existing encrypted/HMAC state cannot establish its own first-key binding.
pub(crate) async fn reject_unverified_key_state(pool: &SqlitePool) -> Result<()> {
    if !has_rows(pool, "key_metadata").await? && has_keyed_state(pool).await? {
        bail!(
            "unbound legacy ciphertext or HMAC state requires original-key verification before migration or first binding; an arbitrary supplied key cannot establish ownership"
        );
    }
    Ok(())
}

async fn has_keyed_state(pool: &SqlitePool) -> Result<bool> {
    if has_ciphertext(pool).await? {
        return Ok(true);
    }
    for table in [
        "sessions",
        "subscription_tokens",
        "subscription_temp_links",
        "recovery_codes",
    ] {
        if has_rows(pool, table).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn has_ciphertext(pool: &SqlitePool) -> Result<bool> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(pool)
    .await
    .context("inspect secret-bearing tables")?;
    for table in tables {
        let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
            sqlx::query_as(&format!("PRAGMA table_info({})", quoted(&table)))
                .fetch_all(pool)
                .await?;
        for (_, name, _, _, _, _) in columns {
            if name.ends_with("_encrypted")
                || name == "secret_ciphertext"
                || (table == "probe_sources"
                    && matches!(name.as_str(), "auth_config" | "last_counter_snapshot"))
            {
                let sql = format!(
                    "SELECT EXISTS(SELECT 1 FROM {} WHERE {} IS NOT NULL AND length({}) > 0)",
                    quoted(&table),
                    quoted(&name),
                    quoted(&name)
                );
                if sqlx::query_scalar::<_, bool>(&sql).fetch_one(pool).await? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
