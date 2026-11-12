//! Explicit, recoverable transition from plaintext cache payloads.

use deve_sub_security::MasterKey;
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::encryption;
use crate::StorageError;

/// Refuse pending legacy cache payloads before serving or creating a backup.
///
/// This checks storage state only. Every repository read additionally
/// authenticates its envelope with the loaded master key.
pub async fn verify_generation_cache_encryption(pool: &SqlitePool) -> Result<(), StorageError> {
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM generation_cache \
         WHERE content_encrypted IS NULL OR content_legacy <> '' \
         OR substr(content_encrypted, 1, 3) <> 'v2:')",
    )
    .fetch_one(pool)
    .await?;
    if pending {
        return Err(StorageError::CacheEncryption(
            "generation cache requires keyed migration; run `deve-sub migrate --key-path <existing-key>`",
        ));
    }
    Ok(())
}

/// Authenticate all retained cache bytes without writing or establishing a key
/// binding. Encryption readiness and producer qualification remain independent.
pub async fn verify_generation_cache_payloads(
    pool: &SqlitePool,
    key: &MasterKey,
) -> Result<(), StorageError> {
    let mut after: Option<String> = None;
    loop {
        let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, cache_key, content_legacy, content_encrypted FROM generation_cache WHERE (? IS NULL OR id > ?) ORDER BY id LIMIT 64")
            .bind(&after).bind(&after).fetch_all(pool).await?;
        if rows.is_empty() {
            return Ok(());
        }
        for (id, cache_key, legacy, encrypted) in rows {
            encryption::open(key, &id, &cache_key, encrypted.as_deref(), &legacy).map_err(
                |_| StorageError::CacheEncryption("generation cache authentication failed"),
            )?;
            after = Some(id);
        }
    }
}

/// Authenticate encrypted rows and seal all legacy payloads atomically.
///
/// Run only after schema migrations, with the existing master key. A failure
/// leaves every payload and key binding unchanged; retry with the same key
/// after correcting the cause. No cache row or metadata is invalidated.
pub async fn migrate_generation_cache(
    pool: &SqlitePool,
    key: &MasterKey,
) -> Result<u64, StorageError> {
    // WHY: serialize backfill with cache writers, and roll back the whole set
    // on key mismatch, corrupt ciphertext, disk failure, or cancellation.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    bind_key(&mut tx, key).await?;
    let mut migrated = 0;
    let mut after: Option<String> = None;
    loop {
        // Bound working memory while keeping the complete transition atomic.
        let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, cache_key, content_legacy, content_encrypted \
             FROM generation_cache WHERE (? IS NULL OR id > ?) ORDER BY id LIMIT 64",
        )
        .bind(&after)
        .bind(&after)
        .fetch_all(&mut *tx)
        .await?;
        if rows.is_empty() {
            break;
        }
        for (id, cache_key, legacy, encrypted) in rows {
            if let Some(encrypted) = encrypted {
                encryption::open(key, &id, &cache_key, Some(&encrypted), &legacy).map_err(
                    |_| StorageError::CacheEncryption("generation cache authentication failed"),
                )?;
            } else {
                let encrypted = encryption::seal(key, &id, &cache_key, &legacy).map_err(|_| {
                    StorageError::CacheEncryption("generation cache encryption failed")
                })?;
                sqlx::query(
                    "UPDATE generation_cache SET content_encrypted = ?, content_legacy = '' WHERE id = ?",
                )
                .bind(encrypted)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
                migrated += 1;
            }
            after = Some(id);
        }
    }
    tx.commit().await?;
    Ok(migrated)
}

pub(super) async fn bind_key(
    tx: &mut Transaction<'_, Sqlite>,
    key: &MasterKey,
) -> Result<(), StorageError> {
    let actual = key
        .fingerprint()
        .map_err(|_| StorageError::CacheEncryption("master key fingerprint failed"))?;
    let bound: Option<String> =
        sqlx::query_scalar("SELECT current_key_fingerprint FROM key_metadata WHERE id = 1")
            .fetch_optional(&mut **tx)
            .await?;
    match bound {
        Some(expected) if expected != actual => {
            return Err(StorageError::KeyFingerprintMismatch { expected, actual });
        }
        Some(_) => {}
        None => {
            sqlx::query("INSERT INTO key_metadata (id, current_key_fingerprint) VALUES (1, ?)")
                .bind(actual)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}
