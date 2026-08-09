//! SQLite implementation of [`ShortCodeRepository`].
//!
//! Short codes are CSPRNG-generated base62 strings stored in the clear (they
//! are public lookup keys, not secrets). The `code` column has a UNIQUE
//! constraint for atomic conflict rejection (OUT-013). See
//! `docs/plan/milestones/M6-subscription-distribution.md` §"Token and
//! short-code security model".

use async_trait::async_trait;
use deve_sub_domain::{ShortCode, ShortCodeRepository, SubscriptionError};
use deve_sub_kernel::{ShortCodeId, SubscriptionId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed short code repository.
pub struct SqliteShortCodeRepository {
    pool: SqlitePool,
}

impl SqliteShortCodeRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct ShortCodeRow {
    id: String,
    subscription_id: String,
    code: String,
    created_at: String,
}

impl ShortCodeRow {
    fn to_domain(&self) -> Result<ShortCode, SubscriptionError> {
        Ok(ShortCode {
            id: ShortCodeId::parse(&self.id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            subscription_id: SubscriptionId::parse(&self.subscription_id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            code: self.code.clone(),
            created_at: parse_ts(&self.created_at).map_err(SubscriptionError::Storage)?,
        })
    }
}

#[async_trait]
impl ShortCodeRepository for SqliteShortCodeRepository {
    async fn create(&self, short_code: &ShortCode) -> Result<(), SubscriptionError> {
        let created_at = format_ts(short_code.created_at).map_err(SubscriptionError::Storage)?;
        sqlx::query(
            "INSERT INTO subscription_short_codes \
             (id, subscription_id, code, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(short_code.id.to_string())
        .bind(short_code.subscription_id.to_string())
        .bind(&short_code.code)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            // WHY: UNIQUE(code) is the only unique constraint on this table.
            // A violation means the CSPRNG-generated code collided with an
            // existing one (OUT-013). The application layer retries with a
            // fresh code.
            if msg.contains("UNIQUE") {
                SubscriptionError::ShortCodeExists
            } else {
                SubscriptionError::Storage(msg)
            }
        })?;
        Ok(())
    }

    async fn find_by_code(&self, code: &str) -> Result<Option<ShortCode>, SubscriptionError> {
        let row: Option<ShortCodeRow> = sqlx::query_as(
            "SELECT id, subscription_id, code, created_at \
             FROM subscription_short_codes WHERE code = ?",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<Option<ShortCode>, SubscriptionError> {
        let row: Option<ShortCodeRow> = sqlx::query_as(
            "SELECT id, subscription_id, code, created_at \
             FROM subscription_short_codes WHERE subscription_id = ?",
        )
        .bind(subscription_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn delete(&self, id: ShortCodeId) -> Result<(), SubscriptionError> {
        let result = sqlx::query("DELETE FROM subscription_short_codes WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SubscriptionError::ShortCodeNotFound);
        }
        Ok(())
    }

    async fn delete_for_subscription(
        &self,
        subscription_id: SubscriptionId,
    ) -> Result<(), SubscriptionError> {
        sqlx::query("DELETE FROM subscription_short_codes WHERE subscription_id = ?")
            .bind(subscription_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        Ok(())
    }
}
