//! SQLite implementation of [`RecoveryCodeRepository`].
//!
//! Stores recovery codes as HMAC-SHA256 hashes. Each code is single-use;
//! the `used` column tracks consumption. Codes are deleted in bulk when
//! regenerated or when 2FA is disabled.

use async_trait::async_trait;
use deve_sub_domain::{IdentityError, RecoveryCode, RecoveryCodeRepository, Session};
use deve_sub_kernel::{RecoveryCodeId, UserId};
use sqlx::sqlite::SqlitePool;

/// SQLite-backed recovery code repository.
pub struct SqliteRecoveryCodeRepository {
    pool: SqlitePool,
}

impl SqliteRecoveryCodeRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct RecoveryCodeRow {
    id: String,
    user_id: String,
    code_hash: String,
    used: i64,
    created_at: String,
}

impl RecoveryCodeRow {
    fn to_domain(&self) -> Result<RecoveryCode, IdentityError> {
        Ok(RecoveryCode {
            id: RecoveryCodeId::parse(&self.id)
                .map_err(|e| IdentityError::Storage(e.to_string()))?,
            user_id: UserId::parse(&self.user_id)
                .map_err(|e| IdentityError::Storage(e.to_string()))?,
            code_hash: self.code_hash.clone(),
            used: self.used != 0,
            created_at: crate::timestamp::parse_ts(&self.created_at)
                .map_err(IdentityError::Storage)?,
        })
    }
}

#[async_trait]
impl RecoveryCodeRepository for SqliteRecoveryCodeRepository {
    async fn replace_all_for_user(
        &self,
        user_id: UserId,
        codes: &[RecoveryCode],
    ) -> Result<(), IdentityError> {
        // WHY: delete + insert in a single transaction so there is never a
        // window where the user has zero recovery codes. Without this, a
        // failure between the delete and insert would leave the user unable
        // to recover from a lost TOTP device (AUTH-006).
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;

        sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;

        for code in codes {
            // WHY: defense-in-depth — the DELETE uses the `user_id` parameter
            // but the INSERT binds `code.user_id`. Current callers always
            // construct codes with the matching user_id, but this assertion
            // catches a future caller that might mismatch.
            debug_assert_eq!(code.user_id, user_id);
            let created_at =
                crate::timestamp::format_ts(code.created_at).map_err(IdentityError::Storage)?;
            sqlx::query(
                "INSERT INTO recovery_codes (id, user_id, code_hash, used, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(code.id.to_string())
            .bind(code.user_id.to_string())
            .bind(&code.code_hash)
            .bind(code.used as i64)
            .bind(created_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_unused_by_hash(
        &self,
        user_id: UserId,
        code_hash: &str,
    ) -> Result<Option<RecoveryCode>, IdentityError> {
        let row: Option<RecoveryCodeRow> = sqlx::query_as(
            "SELECT id, user_id, code_hash, used, created_at \
             FROM recovery_codes \
             WHERE user_id = ? AND code_hash = ? AND used = 0",
        )
        .bind(user_id.to_string())
        .bind(code_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn mark_used_and_create_session(
        &self,
        recovery_code_id: RecoveryCodeId,
        session: &Session,
    ) -> Result<(), IdentityError> {
        // WHY: consume the recovery code and insert the session in one
        // transaction so a session-insert failure rolls back the code
        // consumption. Without this, `login_2fa` could burn a recovery code
        // without granting a session, leaving the user one code poorer and
        // still locked out (AUTH-006, P0-11).
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;

        let result = sqlx::query("UPDATE recovery_codes SET used = 1 WHERE id = ? AND used = 0")
            .bind(recovery_code_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            // WHY: drop tx explicitly to roll back; `begin` acquired a write
            // lock that must be released before returning.
            drop(tx);
            return Err(IdentityError::RecoveryCodeNotFound);
        }

        let created_at =
            crate::timestamp::format_ts(session.created_at).map_err(IdentityError::Storage)?;
        let expires_at =
            crate::timestamp::format_ts(session.expires_at).map_err(IdentityError::Storage)?;
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
        .execute(&mut *tx)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn delete_all_for_user(&self, user_id: UserId) -> Result<(), IdentityError> {
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }
}
