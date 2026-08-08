//! SQLite storage adapter for Deve Sub.
//!
//! Implements the storage Port defined in the domain layer using SQLx
//! with SQLite in WAL mode. See ADR-0002 for the storage Port decision and
//! `docs/plan/13-storage.md` for the SQLite configuration policy.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod generation_cache_repository;
pub mod node_override_repository;
pub mod node_pool_repository;
mod node_row;
pub mod pool_meta_repository;
pub mod recovery_code_repository;
pub mod session_repository;
pub mod source_repository;
pub mod source_snapshot_repository;
pub mod subscription_repository;
pub mod subscription_token_repository;
pub mod template_repository;
pub mod template_version_repository;
pub mod timestamp;
pub mod totp_repository;
pub mod user_repository;

use std::path::Path;
use std::str::FromStr;

use async_trait::async_trait;
use deve_sub_application::{DbHealthPort, HealthError};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use thiserror::Error;

pub use generation_cache_repository::SqliteGenerationCacheRepository;
pub use node_override_repository::SqliteNodeOverrideRepository;
pub use node_pool_repository::SqliteNodePoolRepository;
pub use pool_meta_repository::SqlitePoolMetaRepository;
pub use recovery_code_repository::SqliteRecoveryCodeRepository;
pub use session_repository::SqliteSessionRepository;
pub use source_repository::SqliteSourceRepository;
pub use source_snapshot_repository::SqliteSourceSnapshotRepository;
pub use subscription_repository::SqliteSubscriptionRepository;
pub use subscription_token_repository::SqliteSubscriptionTokenRepository;
pub use template_repository::SqliteTemplateRepository;
pub use template_version_repository::SqliteTemplateVersionRepository;
pub use totp_repository::SqliteTotpSecretRepository;
pub use user_repository::SqliteUserRepository;

/// SQLite configuration from `docs/plan/13-storage.md`.
///
/// WAL mode allows concurrent reads with a single writer. Write transactions
/// must stay short. See ADR-0002.
#[derive(Debug, Clone)]
pub struct SqliteConfig {
    /// Path to the SQLite database file.
    pub path: String,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
}

impl SqliteConfig {
    /// Create a new SQLite config with the given database path.
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_string_lossy().into_owned(),
            max_connections: 8,
        }
    }

    /// Set the maximum number of connections in the pool.
    #[must_use]
    pub fn max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }
}

/// Errors produced by the SQLite storage adapter.
#[derive(Debug, Error)]
pub enum StorageError {
    /// A database operation failed.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// A migration failed to apply.
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// The database path was invalid.
    #[error("invalid database path: {0}")]
    InvalidPath(String),
}

/// SQLite PRAGMA configuration applied on every new connection.
///
/// From `docs/plan/13-storage.md`:
/// ```text
/// journal_mode=WAL
/// foreign_keys=ON
/// busy_timeout=5000
/// synchronous=NORMAL
/// temp_store=MEMORY
/// ```
const PRAGMAS: &[(&str, &str)] = &[
    ("journal_mode", "WAL"),
    ("foreign_keys", "ON"),
    ("busy_timeout", "5000"),
    ("synchronous", "NORMAL"),
    ("temp_store", "MEMORY"),
];

/// Create a SQLite connection pool with WAL and PRAGMA configuration from
/// `docs/plan/13-storage.md`.
///
/// # Errors
/// Returns [`StorageError`] if the database path is invalid or the pool
/// cannot be created.
pub async fn create_pool(config: &SqliteConfig) -> Result<SqlitePool, StorageError> {
    let url = format!("sqlite://{}?mode=rwc", config.path);
    let mut options = SqliteConnectOptions::from_str(&url)
        .map_err(|e| StorageError::InvalidPath(e.to_string()))?;

    for (key, value) in PRAGMAS {
        options = options.pragma(*key, *value);
    }

    SqlitePoolOptions::new()
        .max_connections(config.max_connections)
        .connect_with(options)
        .await
        .map_err(StorageError::from)
}

/// Database health checker backed by a SQLite connection pool.
///
/// Implements [`DbHealthPort`] so the delivery layer can check database
/// connectivity without depending directly on the storage adapter.
pub struct SqliteHealthCheck {
    pool: SqlitePool,
}

impl SqliteHealthCheck {
    /// Create a new health checker wrapping the given pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DbHealthPort for SqliteHealthCheck {
    async fn check(&self) -> Result<(), HealthError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| HealthError::Database(e.to_string()))
    }
}

/// Check database connectivity by executing a trivial query.
///
/// Convenience function for non-trait usage. Prefer [`SqliteHealthCheck`]
/// through the [`DbHealthPort`] trait in delivery-layer code.
///
/// # Errors
/// Returns [`StorageError`] if the query fails, indicating the database is
/// not reachable or healthy.
pub async fn check_database(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(StorageError::from)
}

/// Run all pending migrations. Wraps `sqlx::migrate!` so callers don't
/// repeat the migration path.
///
/// # Errors
/// Returns [`StorageError`] if the database path is invalid or a migration
/// fails.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(StorageError::from)
}

/// Verify that the database schema is up-to-date by checking the
/// `_sqlx_migrations` table exists and has at least one applied migration.
///
/// Call this at startup before serving to give a helpful error if the user
/// forgot to run `deve-sub migrate`.
///
/// # Errors
/// Returns [`StorageError`] if the database is not migrated or unreachable.
pub async fn verify_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("SELECT 1 FROM _sqlx_migrations LIMIT 1")
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| {
            StorageError::InvalidPath(format!(
                "database schema is not initialized — run `deve-sub migrate` first ({e})"
            ))
        })
}
