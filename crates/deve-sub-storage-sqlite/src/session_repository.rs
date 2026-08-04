//! SQLite implementation of [`SessionRepository`].
//!
//! Converts between domain [`Session`] objects and SQLite rows. Session
//! tokens are stored as HMAC-SHA256 digests (`token_hash`), never as raw
//! tokens. See `docs/plan/00-engineering-constitution.md` §"Data and
//! security".

use async_trait::async_trait;
use deve_sub_domain::{IdentityError, Session, SessionRepository};
use deve_sub_kernel::{SessionId, Timestamp, UserId};
use sqlx::sqlite::SqlitePool;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// SQLite-backed session repository.
pub struct SqliteSessionRepository {
    pool: SqlitePool,
}

impl SqliteSessionRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    user_id: String,
    token_hash: String,
    created_at: String,
    expires_at: String,
    revoked: i64,
}

impl SessionRow {
    fn to_domain(&self) -> Result<Session, IdentityError> {
        Ok(Session {
            id: SessionId::parse(&self.id).map_err(|e| IdentityError::Storage(e.to_string()))?,
            user_id: UserId::parse(&self.user_id)
                .map_err(|e| IdentityError::Storage(e.to_string()))?,
            token_hash: self.token_hash.clone(),
            created_at: parse_ts(&self.created_at)?,
            expires_at: parse_ts(&self.expires_at)?,
            revoked: self.revoked != 0,
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
impl SessionRepository for SqliteSessionRepository {
    async fn create(&self, session: &Session) -> Result<(), IdentityError> {
        let created_at = format_ts(session.created_at)?;
        let expires_at = format_ts(session.expires_at)?;

        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, created_at, expires_at, revoked) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(session.user_id.to_string())
        .bind(&session.token_hash)
        .bind(created_at)
        .bind(expires_at)
        .bind(session.revoked as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, IdentityError> {
        let row: Option<SessionRow> =
            sqlx::query_as("SELECT id, user_id, token_hash, created_at, expires_at, revoked FROM sessions WHERE token_hash = ?")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| IdentityError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn revoke(&self, id: SessionId) -> Result<(), IdentityError> {
        let result = sqlx::query("UPDATE sessions SET revoked = 1 WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(IdentityError::SessionNotFound);
        }
        Ok(())
    }

    async fn revoke_all_for_user(&self, user_id: UserId) -> Result<(), IdentityError> {
        sqlx::query("UPDATE sessions SET revoked = 1 WHERE user_id = ? AND revoked = 0")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }
}
