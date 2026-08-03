//! SQLite storage adapter for Deve Sub.
//!
//! Implements the storage Port defined in the application layer using SQLx
//! with SQLite in WAL mode. See ADR-0002 for the storage Port decision and
//! `docs/plan/13-storage.md` for the SQLite configuration policy.

#![cfg_attr(test, allow(clippy::expect_used))]

use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use thiserror::Error;

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

/// Check database connectivity by executing a trivial query.
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
