//! SQLite implementation of [`TempLinkRepository`].
//!
//! Temp link tokens are CSPRNG-generated and stored as HMAC-SHA256 digests,
//! like permanent delivery tokens. Each temp link has a mandatory expiry and a
//! revocation flag. See `docs/plan/milestones/M6-subscription-distribution.md`
//! §"Slicing" Slice 3.

use async_trait::async_trait;
use deve_sub_domain::{SubscriptionError, TempLink, TempLinkRepository};
use deve_sub_kernel::{SubscriptionId, TempLinkId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed temp link repository.
pub struct SqliteTempLinkRepository {
    pool: SqlitePool,
}

impl SqliteTempLinkRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct TempLinkRow {
    id: String,
    subscription_id: String,
    token_digest: String,
    expires_at: String,
    revoked: i64,
    created_at: String,
}

impl TempLinkRow {
    fn to_domain(&self) -> Result<TempLink, SubscriptionError> {
        Ok(TempLink {
            id: TempLinkId::parse(&self.id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            subscription_id: SubscriptionId::parse(&self.subscription_id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            token_digest: self.token_digest.clone(),
            expires_at: parse_ts(&self.expires_at).map_err(SubscriptionError::Storage)?,
            revoked: self.revoked != 0,
            created_at: parse_ts(&self.created_at).map_err(SubscriptionError::Storage)?,
        })
    }
}

#[async_trait]
impl TempLinkRepository for SqliteTempLinkRepository {
    async fn create(&self, temp_link: &TempLink) -> Result<(), SubscriptionError> {
        let expires_at = format_ts(temp_link.expires_at).map_err(SubscriptionError::Storage)?;
        let created_at = format_ts(temp_link.created_at).map_err(SubscriptionError::Storage)?;
        sqlx::query(
            "INSERT INTO subscription_temp_links \
             (id, subscription_id, token_digest, expires_at, revoked, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(temp_link.id.to_string())
        .bind(temp_link.subscription_id.to_string())
        .bind(&temp_link.token_digest)
        .bind(expires_at)
        .bind(temp_link.revoked as i64)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<TempLink>, SubscriptionError> {
        let row: Option<TempLinkRow> = sqlx::query_as(
            "SELECT id, subscription_id, token_digest, expires_at, revoked, created_at \
             FROM subscription_temp_links WHERE token_digest = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn revoke(&self, id: TempLinkId) -> Result<(), SubscriptionError> {
        let result = sqlx::query("UPDATE subscription_temp_links SET revoked = 1 WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SubscriptionError::TempLinkNotFound);
        }
        Ok(())
    }

    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError> {
        sqlx::query("DELETE FROM subscription_temp_links WHERE subscription_id = ?")
            .bind(subscription_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_id(&self, id: TempLinkId) -> Result<Option<TempLink>, SubscriptionError> {
        let row: Option<TempLinkRow> = sqlx::query_as(
            "SELECT id, subscription_id, token_digest, expires_at, revoked, created_at \
             FROM subscription_temp_links WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }
}
