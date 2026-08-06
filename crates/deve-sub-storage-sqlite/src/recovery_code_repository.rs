//! SQLite implementation of [`RecoveryCodeRepository`].
//!
//! Stores recovery codes as HMAC-SHA256 hashes. Each code is single-use;
//! the `used` column tracks consumption. Codes are deleted in bulk when
//! regenerated or when 2FA is disabled.

use async_trait::async_trait;
use deve_sub_domain::{IdentityError, RecoveryCode, RecoveryCodeRepository};
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

    async fn mark_used(&self, id: RecoveryCodeId) -> Result<(), IdentityError> {
        // WHY: `AND used = 0` prevents concurrent double-use. SQLite counts a
        // matched-but-unchanged row in `rows_affected()`, so without this
        // guard a no-op UPDATE (1→1) would return 1 and two concurrent
        // requests could both "succeed" (AUTH-006).
        let result = sqlx::query("UPDATE recovery_codes SET used = 1 WHERE id = ? AND used = 0")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(IdentityError::RecoveryCodeNotFound);
        }
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
