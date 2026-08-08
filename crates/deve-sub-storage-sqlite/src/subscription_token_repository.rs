//! SQLite implementation of [`SubscriptionTokenRepository`].
//!
//! Subscription delivery tokens are stored as HMAC-SHA256 digests only; the
//! plaintext is never persisted. During rotation grace, the previous digest is
//! retained in `previous_token_digest` so both old and new tokens remain valid
//! until the grace window expires. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security" and
//! `docs/plan/milestones/M6-subscription-distribution.md` §"Token and
//! short-code security model".

use async_trait::async_trait;
use deve_sub_domain::{SubscriptionError, SubscriptionToken, SubscriptionTokenRepository};
use deve_sub_kernel::{SubscriptionId, SubscriptionTokenId, Timestamp};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed subscription token repository.
pub struct SqliteSubscriptionTokenRepository {
    pool: SqlitePool,
}

impl SqliteSubscriptionTokenRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct TokenRow {
    id: String,
    subscription_id: String,
    token_digest: String,
    previous_token_digest: Option<String>,
    rotation_grace_until: Option<String>,
    issued_at: String,
}

impl TokenRow {
    fn to_domain(&self) -> Result<SubscriptionToken, SubscriptionError> {
        Ok(SubscriptionToken {
            id: SubscriptionTokenId::parse(&self.id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            subscription_id: SubscriptionId::parse(&self.subscription_id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            token_digest: self.token_digest.clone(),
            previous_token_digest: self.previous_token_digest.clone(),
            rotation_grace_until: self
                .rotation_grace_until
                .as_deref()
                .map(parse_ts)
                .transpose()
                .map_err(SubscriptionError::Storage)?,
            issued_at: parse_ts(&self.issued_at).map_err(SubscriptionError::Storage)?,
        })
    }
}

#[async_trait]
impl SubscriptionTokenRepository for SqliteSubscriptionTokenRepository {
    async fn create(&self, token: &SubscriptionToken) -> Result<(), SubscriptionError> {
        let issued_at = format_ts(token.issued_at).map_err(SubscriptionError::Storage)?;
        let grace = token
            .rotation_grace_until
            .map(format_ts)
            .transpose()
            .map_err(SubscriptionError::Storage)?;

        sqlx::query(
            "INSERT INTO subscription_tokens \
             (id, subscription_id, token_digest, previous_token_digest, \
              rotation_grace_until, issued_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(token.id.to_string())
        .bind(token.subscription_id.to_string())
        .bind(&token.token_digest)
        .bind(&token.previous_token_digest)
        .bind(grace)
        .bind(issued_at)
        .execute(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<SubscriptionToken>, SubscriptionError> {
        let row: Option<TokenRow> = sqlx::query_as(
            "SELECT id, subscription_id, token_digest, previous_token_digest, \
             rotation_grace_until, issued_at \
             FROM subscription_tokens WHERE token_digest = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_active_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<Option<SubscriptionToken>, SubscriptionError> {
        let row: Option<TokenRow> = sqlx::query_as(
            "SELECT id, subscription_id, token_digest, previous_token_digest, \
             rotation_grace_until, issued_at \
             FROM subscription_tokens WHERE subscription_id = ?",
        )
        .bind(subscription_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn rotate(
        &self,
        subscription_id: SubscriptionId,
        new_token: &SubscriptionToken,
        grace_until: Option<Timestamp>,
    ) -> Result<SubscriptionToken, SubscriptionError> {
        // WHY: load the current row first to capture the old digest as
        // previous_token_digest, then update in place. This keeps a single
        // token row per subscription with a stable id, so the Subscription's
        // token_id reference stays valid across rotations. The whole operation
        // runs in a transaction so concurrent rotations serialize.
        let current = self
            .find_active_for_subscription(subscription_id)
            .await?
            .ok_or(SubscriptionError::TokenNotFound)?;

        let grace_str = grace_until
            .map(format_ts)
            .transpose()
            .map_err(SubscriptionError::Storage)?;
        let issued_at = format_ts(new_token.issued_at).map_err(SubscriptionError::Storage)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        sqlx::query(
            "UPDATE subscription_tokens SET \
               token_digest = ?, \
               previous_token_digest = ?, \
               rotation_grace_until = ?, \
               issued_at = ? \
             WHERE subscription_id = ?",
        )
        .bind(&new_token.token_digest)
        .bind(&current.token_digest)
        .bind(&grace_str)
        .bind(issued_at)
        .bind(subscription_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        Ok(SubscriptionToken {
            id: current.id,
            subscription_id,
            token_digest: new_token.token_digest.clone(),
            previous_token_digest: Some(current.token_digest.clone()),
            rotation_grace_until: grace_until,
            issued_at: new_token.issued_at,
        })
    }

    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError> {
        sqlx::query("DELETE FROM subscription_tokens WHERE subscription_id = ?")
            .bind(subscription_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_id(
        &self,
        id: SubscriptionTokenId,
    ) -> Result<Option<SubscriptionToken>, SubscriptionError> {
        let row: Option<TokenRow> = sqlx::query_as(
            "SELECT id, subscription_id, token_digest, previous_token_digest, \
             rotation_grace_until, issued_at \
             FROM subscription_tokens WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }
}
