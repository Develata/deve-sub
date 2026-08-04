//! SQLite implementation of [`UserRepository`].
//!
//! Converts between domain [`User`] objects and SQLite rows. Timestamps are
//! stored as RFC 3339 strings, matching the `strftime` default in migration
//! 0002. See ADR-0002 for the storage Port decision.

use async_trait::async_trait;
use deve_sub_domain::{IdentityError, Role, User, UserRepository};
use deve_sub_kernel::{Timestamp, UserId};
use sqlx::sqlite::SqlitePool;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// SQLite-backed user repository.
pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Internal row representation for `sqlx::FromRow`.
#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
    role: String,
    enabled: i64,
    expires_at: Option<String>,
    traffic_quota: i64,
    created_at: String,
}

impl UserRow {
    fn to_domain(&self) -> Result<User, IdentityError> {
        Ok(User {
            id: UserId::parse(&self.id).map_err(|e| IdentityError::Storage(e.to_string()))?,
            username: self.username.clone(),
            password_hash: self.password_hash.clone(),
            role: self
                .role
                .parse::<Role>()
                .map_err(|e| IdentityError::Storage(e.to_string()))?,
            enabled: self.enabled != 0,
            expires_at: self.expires_at.as_deref().map(parse_ts).transpose()?,
            traffic_quota: u64::try_from(self.traffic_quota)
                .map_err(|_| IdentityError::Storage("negative traffic_quota".to_owned()))?,
            created_at: parse_ts(&self.created_at)?,
        })
    }
}

fn format_ts(ts: Timestamp) -> Result<String, IdentityError> {
    ts.as_offset_date_time()
        .format(&Rfc3339)
        .map_err(|e| IdentityError::Storage(format!("timestamp format error: {e}")))
}

fn parse_ts(s: &str) -> Result<Timestamp, IdentityError> {
    OffsetDateTime::parse(s, &Rfc3339)
        .map(Timestamp::from_offset_date_time)
        .map_err(|e| IdentityError::Storage(format!("timestamp parse error: {e}")))
}

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn create(&self, user: &User) -> Result<(), IdentityError> {
        let expires_at = user.expires_at.map(format_ts).transpose()?;
        let created_at = format_ts(user.created_at)?;

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, enabled, expires_at, traffic_quota, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user.id.to_string())
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(user.role.to_string())
        .bind(user.enabled as i64)
        .bind(expires_at)
        .bind(user.traffic_quota as i64)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                IdentityError::UsernameExists
            } else {
                IdentityError::Storage(msg)
            }
        })?;
        Ok(())
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, IdentityError> {
        let row: Option<UserRow> =
            sqlx::query_as("SELECT id, username, password_hash, role, enabled, expires_at, traffic_quota, created_at FROM users WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| IdentityError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, IdentityError> {
        let row: Option<UserRow> =
            sqlx::query_as("SELECT id, username, password_hash, role, enabled, expires_at, traffic_quota, created_at FROM users WHERE username = ?")
                .bind(username)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| IdentityError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn count(&self) -> Result<i64, IdentityError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(count)
    }

    async fn list(&self, cursor: Option<UserId>, limit: u32) -> Result<Vec<User>, IdentityError> {
        // WHY: cap at 100 to prevent unbounded result sets from a malicious
        // or accidental large `limit` query parameter.
        let limit = limit.min(100) as i64;
        let rows: Vec<UserRow> = match cursor {
            Some(c) => {
                sqlx::query_as(
                    "SELECT id, username, password_hash, role, enabled, expires_at, traffic_quota, created_at \
                     FROM users WHERE id > ? ORDER BY id LIMIT ?",
                )
                .bind(c.to_string())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| IdentityError::Storage(e.to_string()))?
            }
            None => {
                sqlx::query_as(
                    "SELECT id, username, password_hash, role, enabled, expires_at, traffic_quota, created_at \
                     FROM users ORDER BY id LIMIT ?",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| IdentityError::Storage(e.to_string()))?
            }
        };
        rows.iter().map(|r| r.to_domain()).collect()
    }

    async fn create_if_empty(&self, user: &User) -> Result<(), IdentityError> {
        let expires_at = user.expires_at.map(format_ts).transpose()?;
        let created_at = format_ts(user.created_at)?;

        let result = sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, enabled, expires_at, traffic_quota, created_at) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ? \
             WHERE NOT EXISTS (SELECT 1 FROM users LIMIT 1)",
        )
        .bind(user.id.to_string())
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(user.role.to_string())
        .bind(user.enabled as i64)
        .bind(expires_at)
        .bind(user.traffic_quota as i64)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(IdentityError::AlreadyInitialized);
        }
        Ok(())
    }

    async fn set_enabled(&self, id: UserId, enabled: bool) -> Result<(), IdentityError> {
        let result = sqlx::query("UPDATE users SET enabled = ? WHERE id = ?")
            .bind(enabled as i64)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(IdentityError::UserNotFound);
        }
        Ok(())
    }
}
