//! SQLite implementation of [`SubscriptionRepository`].
//!
//! Converts between domain [`Subscription`] aggregates and SQLite rows.
//! `node_selection` is stored as a JSON string (serialized via `serde_json`).
//! Timestamps are stored as RFC 3339 strings, matching the `strftime` default
//! in migration 0009. See ADR-0002 for the storage Port decision and
//! `docs/plan/milestones/M6-subscription-distribution.md` for the milestone
//! blueprint.

use async_trait::async_trait;
use deve_sub_domain::{NodeSelector, Subscription, SubscriptionError, SubscriptionRepository};
use deve_sub_kernel::{ShortCodeId, SubscriptionId, TemplateId, UserId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::{format_ts, parse_ts};

/// SQLite-backed subscription repository.
pub struct SqliteSubscriptionRepository {
    pool: SqlitePool,
}

impl SqliteSubscriptionRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct SubscriptionRow {
    id: String,
    name: String,
    slug: String,
    owner_id: String,
    template_id: String,
    template_version_pin: Option<i64>,
    profile: String,
    node_selection: String,
    traffic_limit: Option<i64>,
    expires_at: Option<String>,
    token_id: String,
    short_code_id: Option<String>,
    enabled: i64,
    created_at: String,
    updated_at: String,
}

impl SubscriptionRow {
    fn to_domain(&self) -> Result<Subscription, SubscriptionError> {
        let node_selection: NodeSelector = serde_json::from_str(&self.node_selection)
            .map_err(|e| SubscriptionError::Storage(format!("invalid node_selection JSON: {e}")))?;
        Ok(Subscription {
            id: SubscriptionId::parse(&self.id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            name: self.name.clone(),
            slug: self.slug.clone(),
            owner_id: UserId::parse(&self.owner_id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            template_id: TemplateId::parse(&self.template_id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            template_version_pin: self
                .template_version_pin
                .map(|v| {
                    u64::try_from(v).map_err(|e| {
                        SubscriptionError::Storage(format!("negative template_version_pin: {e}"))
                    })
                })
                .transpose()?,
            profile: self.profile.clone(),
            node_selection,
            traffic_limit: self
                .traffic_limit
                .map(|v| {
                    u64::try_from(v).map_err(|e| {
                        SubscriptionError::Storage(format!("negative traffic_limit: {e}"))
                    })
                })
                .transpose()?,
            expires_at: self
                .expires_at
                .as_deref()
                .map(parse_ts)
                .transpose()
                .map_err(SubscriptionError::Storage)?,
            token_id: deve_sub_kernel::SubscriptionTokenId::parse(&self.token_id)
                .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            short_code_id: self
                .short_code_id
                .as_deref()
                .map(|s| {
                    ShortCodeId::parse(s).map_err(|e| SubscriptionError::Storage(e.to_string()))
                })
                .transpose()?,
            enabled: self.enabled != 0,
            created_at: parse_ts(&self.created_at).map_err(SubscriptionError::Storage)?,
            updated_at: parse_ts(&self.updated_at).map_err(SubscriptionError::Storage)?,
        })
    }
}

#[async_trait]
impl SubscriptionRepository for SqliteSubscriptionRepository {
    async fn create(&self, subscription: &Subscription) -> Result<(), SubscriptionError> {
        let node_selection = serde_json::to_string(&subscription.node_selection)
            .map_err(|e| SubscriptionError::Storage(format!("node_selection serialize: {e}")))?;
        let created_at = format_ts(subscription.created_at).map_err(SubscriptionError::Storage)?;
        let updated_at = format_ts(subscription.updated_at).map_err(SubscriptionError::Storage)?;
        let expires_at = subscription
            .expires_at
            .map(format_ts)
            .transpose()
            .map_err(SubscriptionError::Storage)?;

        sqlx::query(
            "INSERT INTO subscriptions \
             (id, name, slug, owner_id, template_id, template_version_pin, profile, \
              node_selection, traffic_limit, expires_at, token_id, enabled, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(subscription.id.to_string())
        .bind(&subscription.name)
        .bind(&subscription.slug)
        .bind(subscription.owner_id.to_string())
        .bind(subscription.template_id.to_string())
        .bind(subscription.template_version_pin.map(|v| v as i64))
        .bind(&subscription.profile)
        .bind(&node_selection)
        .bind(subscription.traffic_limit.map(|v| v as i64))
        .bind(expires_at)
        .bind(subscription.token_id.to_string())
        .bind(subscription.enabled as i64)
        .bind(created_at)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            // WHY: UNIQUE(owner_id, slug) is the only unique constraint on
            // this table. A violation means the slug is taken for this owner.
            if msg.contains("UNIQUE") {
                SubscriptionError::SlugExists
            } else {
                SubscriptionError::Storage(msg)
            }
        })?;
        Ok(())
    }

    async fn find_by_id(
        &self,
        id: SubscriptionId,
    ) -> Result<Option<Subscription>, SubscriptionError> {
        let row: Option<SubscriptionRow> = sqlx::query_as(
            "SELECT id, name, slug, owner_id, template_id, template_version_pin, profile, \
             node_selection, traffic_limit, expires_at, token_id, short_code_id, enabled, created_at, updated_at \
             FROM subscriptions WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_slug(
        &self,
        owner_id: UserId,
        slug: &str,
    ) -> Result<Option<Subscription>, SubscriptionError> {
        let row: Option<SubscriptionRow> = sqlx::query_as(
            "SELECT id, name, slug, owner_id, template_id, template_version_pin, profile, \
             node_selection, traffic_limit, expires_at, token_id, short_code_id, enabled, created_at, updated_at \
             FROM subscriptions WHERE owner_id = ? AND slug = ?",
        )
        .bind(owner_id.to_string())
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn list(
        &self,
        owner_id: UserId,
        cursor: Option<SubscriptionId>,
        limit: u32,
    ) -> Result<Vec<Subscription>, SubscriptionError> {
        let limit = limit.min(100) as i64;
        let rows: Vec<SubscriptionRow> = match cursor {
            Some(c) => sqlx::query_as(
                "SELECT id, name, slug, owner_id, template_id, template_version_pin, profile, \
                     node_selection, traffic_limit, expires_at, token_id, short_code_id, \
                     enabled, created_at, updated_at FROM subscriptions \
                     WHERE owner_id = ? AND id > ? ORDER BY id LIMIT ?",
            )
            .bind(owner_id.to_string())
            .bind(c.to_string())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
            None => sqlx::query_as(
                "SELECT id, name, slug, owner_id, template_id, template_version_pin, profile, \
                     node_selection, traffic_limit, expires_at, token_id, short_code_id, \
                     enabled, created_at, updated_at FROM subscriptions \
                     WHERE owner_id = ? ORDER BY id LIMIT ?",
            )
            .bind(owner_id.to_string())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?,
        };
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn update(&self, subscription: &Subscription) -> Result<(), SubscriptionError> {
        let node_selection = serde_json::to_string(&subscription.node_selection)
            .map_err(|e| SubscriptionError::Storage(format!("node_selection serialize: {e}")))?;
        let updated_at = format_ts(subscription.updated_at).map_err(SubscriptionError::Storage)?;
        let expires_at = subscription
            .expires_at
            .map(format_ts)
            .transpose()
            .map_err(SubscriptionError::Storage)?;

        let result = sqlx::query(
            "UPDATE subscriptions SET \
               name = ?, \
               slug = ?, \
               template_version_pin = ?, \
               profile = ?, \
               node_selection = ?, \
               traffic_limit = ?, \
               expires_at = ?, \
               enabled = ?, \
               updated_at = ? \
             WHERE id = ?",
        )
        .bind(&subscription.name)
        .bind(&subscription.slug)
        .bind(subscription.template_version_pin.map(|v| v as i64))
        .bind(&subscription.profile)
        .bind(&node_selection)
        .bind(subscription.traffic_limit.map(|v| v as i64))
        .bind(expires_at)
        .bind(subscription.enabled as i64)
        .bind(updated_at)
        .bind(subscription.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                SubscriptionError::SlugExists
            } else {
                SubscriptionError::Storage(msg)
            }
        })?;
        if result.rows_affected() == 0 {
            return Err(SubscriptionError::SubscriptionNotFound);
        }
        Ok(())
    }

    async fn delete(&self, id: SubscriptionId) -> Result<(), SubscriptionError> {
        // WHY: ON DELETE CASCADE in migrations 0009 and 0010 removes
        // subscription_tokens, subscription_short_codes, and
        // subscription_temp_links rows automatically. No manual cascade
        // needed for dependent rows.
        let result = sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SubscriptionError::SubscriptionNotFound);
        }
        Ok(())
    }

    async fn set_short_code_id(
        &self,
        subscription_id: SubscriptionId,
        short_code_id: Option<ShortCodeId>,
    ) -> Result<(), SubscriptionError> {
        let result = sqlx::query("UPDATE subscriptions SET short_code_id = ? WHERE id = ?")
            .bind(short_code_id.map(|id| id.to_string()))
            .bind(subscription_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SubscriptionError::SubscriptionNotFound);
        }
        Ok(())
    }
}
