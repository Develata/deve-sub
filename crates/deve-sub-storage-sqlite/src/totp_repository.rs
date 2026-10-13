//! SQLite implementation of [`TotpSecretRepository`].
//!
//! Stores encrypted TOTP secrets (XChaCha20-Poly1305 ciphertext + nonce) as
//! BLOBs. The domain entity holds the encrypted bytes; the application layer
//! handles encryption/decryption via `deve-sub-security`.

use async_trait::async_trait;
use deve_sub_domain::{IdentityError, RecoveryCode, TotpSecret, TotpSecretRepository};
use deve_sub_kernel::UserId;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqlitePool;

use crate::recovery_code_repository::replace_in_transaction;
use crate::timestamp::format_ts;

/// SQLite-backed TOTP secret repository.
pub struct SqliteTotpSecretRepository {
    pool: SqlitePool,
}

impl SqliteTotpSecretRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct TotpSecretRow {
    user_id: String,
    secret_ciphertext: Vec<u8>,
    nonce: Vec<u8>,
    created_at: String,
}

impl TotpSecretRow {
    fn to_domain(&self) -> Result<TotpSecret, IdentityError> {
        let nonce: [u8; 24] = self
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| IdentityError::Storage("TOTP nonce is not 24 bytes".to_owned()))?;
        Ok(TotpSecret {
            user_id: UserId::parse(&self.user_id)
                .map_err(|e| IdentityError::Storage(e.to_string()))?,
            secret_ciphertext: self.secret_ciphertext.clone(),
            nonce,
            created_at: crate::timestamp::parse_ts(&self.created_at)
                .map_err(IdentityError::Storage)?,
        })
    }
}

#[async_trait]
impl TotpSecretRepository for SqliteTotpSecretRepository {
    async fn upsert(&self, secret: &TotpSecret) -> Result<(), IdentityError> {
        let created_at = format_ts(secret.created_at).map_err(IdentityError::Storage)?;
        // WHY: checking the flag in this write prevents a setup that read an
        // earlier disabled state from replacing a concurrently enabled secret.
        let result = sqlx::query(
            "INSERT INTO totp_secrets (user_id, secret_ciphertext, nonce, created_at) \
             SELECT id, ?, ?, ? FROM users WHERE id = ? AND two_factor_enabled = 0 \
             ON CONFLICT(user_id) DO UPDATE SET \
               secret_ciphertext = excluded.secret_ciphertext, \
               nonce = excluded.nonce, \
               created_at = excluded.created_at, \
               last_used_timestep = NULL",
        )
        .bind(&secret.secret_ciphertext)
        .bind(&secret.nonce[..])
        .bind(created_at)
        .bind(secret.user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(IdentityError::TwoFactorAlreadyEnabled);
        }
        Ok(())
    }

    async fn find_by_user(&self, user_id: UserId) -> Result<Option<TotpSecret>, IdentityError> {
        let row: Option<TotpSecretRow> =
            sqlx::query_as("SELECT user_id, secret_ciphertext, nonce, created_at FROM totp_secrets WHERE user_id = ?")
                .bind(user_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| IdentityError::Storage(e.to_string()))?;
        row.map(|r| r.to_domain()).transpose()
    }

    async fn enable_if_current(
        &self,
        expected: &TotpSecret,
        codes: &[RecoveryCode],
    ) -> Result<bool, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        if !set_enabled_if_current(&mut tx, expected, false, true).await? {
            let enabled: Option<bool> =
                sqlx::query_scalar("SELECT two_factor_enabled FROM users WHERE id = ?")
                    .bind(expected.user_id.to_string())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage)?;
            tx.rollback().await.map_err(storage)?;
            return if enabled == Some(true) {
                Err(IdentityError::TwoFactorAlreadyEnabled)
            } else {
                Ok(false)
            };
        }
        replace_in_transaction(&mut tx, expected.user_id, codes).await?;
        tx.commit().await.map_err(storage)?;
        Ok(true)
    }

    async fn disable_if_current(&self, expected: &TotpSecret) -> Result<bool, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        if !set_enabled_if_current(&mut tx, expected, true, false).await? {
            tx.rollback().await.map_err(storage)?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM totp_secrets WHERE user_id = ?")
            .bind(expected.user_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
            .bind(expected.user_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        tx.commit().await.map_err(storage)?;
        Ok(true)
    }

    async fn replace_recovery_codes_if_current(
        &self,
        expected: &TotpSecret,
        codes: &[RecoveryCode],
    ) -> Result<bool, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        if !set_enabled_if_current(&mut tx, expected, true, true).await? {
            tx.rollback().await.map_err(storage)?;
            return Ok(false);
        }
        replace_in_transaction(&mut tx, expected.user_id, codes).await?;
        tx.commit().await.map_err(storage)?;
        Ok(true)
    }

    async fn record_used_timestep(
        &self,
        user_id: UserId,
        timestep: u64,
    ) -> Result<bool, IdentityError> {
        // WHY: the guard makes the check-and-set atomic — a replayed
        // timestep matches zero rows, so concurrent attempts cannot both
        // succeed (RFC 6238 §5.2).
        let result = sqlx::query(
            "UPDATE totp_secrets SET last_used_timestep = ? WHERE user_id = ? \
             AND (last_used_timestep IS NULL OR last_used_timestep < ?)",
        )
        .bind(
            i64::try_from(timestep)
                .map_err(|_| IdentityError::Storage("TOTP timestep out of i64 range".to_owned()))?,
        )
        .bind(user_id.to_string())
        .bind(
            i64::try_from(timestep)
                .map_err(|_| IdentityError::Storage("TOTP timestep out of i64 range".to_owned()))?,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete(&self, user_id: UserId) -> Result<(), IdentityError> {
        sqlx::query("DELETE FROM totp_secrets WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }
}

/// Acquire write admission and compare the exact enrollment in one statement.
/// Timesteps are deliberately excluded: accepting a login code does not create
/// a new enrollment. Random ciphertext/nonce pairs distinguish setup attempts.
async fn set_enabled_if_current(
    connection: &mut SqliteConnection,
    expected: &TotpSecret,
    was_enabled: bool,
    enabled: bool,
) -> Result<bool, IdentityError> {
    // WHY: this must be the transaction's first statement. Reading before
    // write admission can race another writer and fail SQLITE_BUSY_SNAPSHOT;
    // all three lifecycle operations need the same protected enrollment.
    let result = sqlx::query(
        "UPDATE users SET two_factor_enabled = ? \
         WHERE id = ? AND two_factor_enabled = ? \
         AND EXISTS (SELECT 1 FROM totp_secrets \
             WHERE user_id = users.id AND secret_ciphertext = ? AND nonce = ?)",
    )
    .bind(enabled)
    .bind(expected.user_id.to_string())
    .bind(was_enabled)
    .bind(&expected.secret_ciphertext)
    .bind(&expected.nonce[..])
    .execute(connection)
    .await
    .map_err(storage)?;
    Ok(result.rows_affected() == 1)
}

fn storage(error: sqlx::Error) -> IdentityError {
    IdentityError::Storage(error.to_string())
}
