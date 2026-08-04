//! SQLite implementation of [`TotpSecretRepository`].
//!
//! Stores encrypted TOTP secrets (XChaCha20-Poly1305 ciphertext + nonce) as
//! BLOBs. The domain entity holds the encrypted bytes; the application layer
//! handles encryption/decryption via `deve-sub-security`.

use async_trait::async_trait;
use deve_sub_domain::{IdentityError, TotpSecret, TotpSecretRepository};
use deve_sub_kernel::UserId;
use sqlx::sqlite::SqlitePool;

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
        Ok(TotpSecret {
            user_id: UserId::parse(&self.user_id)
                .map_err(|e| IdentityError::Storage(e.to_string()))?,
            secret_ciphertext: self.secret_ciphertext.clone(),
            nonce: self.nonce.clone(),
            created_at: crate::timestamp::parse_ts(&self.created_at)?,
        })
    }
}

#[async_trait]
impl TotpSecretRepository for SqliteTotpSecretRepository {
    async fn upsert(&self, secret: &TotpSecret) -> Result<(), IdentityError> {
        let created_at = format_ts(secret.created_at)?;
        sqlx::query(
            "INSERT INTO totp_secrets (user_id, secret_ciphertext, nonce, created_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET \
               secret_ciphertext = excluded.secret_ciphertext, \
               nonce = excluded.nonce, \
               created_at = excluded.created_at",
        )
        .bind(secret.user_id.to_string())
        .bind(&secret.secret_ciphertext)
        .bind(&secret.nonce)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| IdentityError::Storage(e.to_string()))?;
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

    async fn delete(&self, user_id: UserId) -> Result<(), IdentityError> {
        sqlx::query("DELETE FROM totp_secrets WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| IdentityError::Storage(e.to_string()))?;
        Ok(())
    }
}
