//! SQLite storage adapter for Deve Sub.
//!
//! Implements the storage Port defined in the domain layer using SQLx
//! with SQLite in WAL mode. See ADR-0002 for the storage Port decision and
//! `docs/plan/13-storage.md` for the SQLite configuration policy.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod audit_log_repository;
pub mod generation_cache_repository;
pub mod node_override_repository;
pub mod node_pool_repository;
mod node_row;
pub mod pool_meta_repository;
pub mod probe_repository;
pub mod recovery_code_repository;
pub mod session_repository;
pub mod short_code_repository;
pub mod source_refresh_job_repository;
pub mod source_repository;
pub mod source_snapshot_repository;
pub mod subscription_repository;
pub mod subscription_token_repository;
pub mod temp_link_repository;
pub mod template_repository;
pub mod template_version_repository;
pub mod timestamp;
pub mod totp_repository;
pub mod traffic_daily_snapshot_repository;
pub mod traffic_repository;
pub mod user_repository;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;

use async_trait::async_trait;
use deve_sub_application::{DbHealthPort, HealthError};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use thiserror::Error;

pub use audit_log_repository::SqliteAuditLogRepository;
pub use generation_cache_repository::SqliteGenerationCacheRepository;
pub use node_override_repository::SqliteNodeOverrideRepository;
pub use node_pool_repository::SqliteNodePoolRepository;
pub use pool_meta_repository::SqlitePoolMetaRepository;
pub use probe_repository::{
    SqliteLatencyRecordRepository, SqliteProbeRunRepository, SqliteProbeSourceRepository,
};
pub use recovery_code_repository::SqliteRecoveryCodeRepository;
pub use session_repository::SqliteSessionRepository;
pub use short_code_repository::SqliteShortCodeRepository;
pub use source_refresh_job_repository::SqliteSourceRefreshJobRepository;
pub use source_repository::SqliteSourceRepository;
pub use source_snapshot_repository::SqliteSourceSnapshotRepository;
pub use subscription_repository::SqliteSubscriptionRepository;
pub use subscription_token_repository::SqliteSubscriptionTokenRepository;
pub use temp_link_repository::SqliteTempLinkRepository;
pub use template_repository::SqliteTemplateRepository;
pub use template_version_repository::SqliteTemplateVersionRepository;
pub use totp_repository::SqliteTotpSecretRepository;
pub use traffic_daily_snapshot_repository::SqliteTrafficDailySnapshotRepository;
pub use traffic_repository::SqliteTrafficRepository;
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

    /// DS-AUD-B06: structured schema-verification errors. The previous
    /// `verify_schema` only checked that `_sqlx_migrations` had a row; it
    /// passed when the DB was behind, ahead, dirty, or had a checksum
    /// mismatch. These variants make each failure mode actionable.
    #[error("database schema is not initialized — run `deve-sub migrate` first")]
    NotInitialized,

    #[error(
        "database schema is behind the binary: migration(s) {missing:?} not applied — \
         run `deve-sub migrate`"
    )]
    SchemaBehind { missing: Vec<i64> },

    #[error(
        "database schema is ahead of the binary: applied migration {version} is not known to \
         this binary — upgrade deve-sub (constraint #13: forward-only)"
    )]
    SchemaAhead { version: i64 },

    #[error(
        "database migration {version} is marked as not successfully applied (dirty) — \
         run `deve-sub migrate` to retry, or restore from backup"
    )]
    SchemaDirty { version: i64 },

    #[error(
        "checksum mismatch on migration {version}: the applied migration's SQL was edited after \
         it was applied — restore from backup or reconcile the migration file"
    )]
    SchemaChecksumMismatch { version: i64 },

    /// DS-AUD-B07: the loaded master key does not match the key bound to the
    /// database. The fingerprint is a one-way HMAC-SHA256 digest (the raw
    /// key cannot be recovered from it), so including both fingerprints in
    /// the message is safe and aids operator diagnosis. Fail-closed: the
    /// command must refuse to proceed to prevent a new key epoch on an
    /// existing DB whose key was lost/misconfigured.
    #[error(
        "master key fingerprint mismatch: the loaded key does not match the key bound to this \
         database.\n  expected (bound to DB): {expected}\n  actual (loaded key):   {actual}\n  \
         Restore the correct key from backup, or remove the database to start fresh"
    )]
    KeyFingerprintMismatch { expected: String, actual: String },
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

/// Bind the database to the loaded master key, or verify the loaded key
/// matches the key already bound (DS-AUD-B07).
///
/// Call this after [`run_migrations`] and `MasterKey::load` on every keyed
/// entry point (CLI management commands, `serve`). The behavior is:
/// - If `key_metadata` is empty (fresh DB) → INSERT a singleton row (id=1)
///   recording `fingerprint`. This is the "new empty DB init transaction"
///   the audit permits: the first key to open a fresh DB binds to it.
/// - If a row exists → compare `current_key_fingerprint` against
///   `fingerprint`. Mismatch → [`StorageError::KeyFingerprintMismatch`]
///   (fail-closed). This prevents a management command from silently
///   generating a NEW key on a host with an existing DB whose key file was
///   lost/misconfigured, which would split the key epoch and make old
///   ciphertext unreadable.
///
/// `migrate` (which runs before any key is loaded) must NOT call this; the
/// binding happens on the first keyed command after migrate.
///
/// # Errors
/// Returns [`StorageError::Database`] if the query fails, or
/// [`StorageError::KeyFingerprintMismatch`] if the loaded key does not match
/// the bound key.
pub async fn ensure_key_binding(pool: &SqlitePool, fingerprint: &str) -> Result<(), StorageError> {
    let bound: Option<(String,)> =
        sqlx::query_as("SELECT current_key_fingerprint FROM key_metadata WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    match bound {
        None => {
            sqlx::query("INSERT INTO key_metadata (id, current_key_fingerprint) VALUES (1, ?)")
                .bind(fingerprint)
                .execute(pool)
                .await?;
            tracing::info!(fingerprint, "master key bound to database");
        }
        Some((expected,)) if expected == fingerprint => {}
        Some((expected,)) => {
            return Err(StorageError::KeyFingerprintMismatch {
                expected,
                actual: fingerprint.to_owned(),
            });
        }
    }
    Ok(())
}

/// Return the highest migration version embedded in this binary (compile-time
/// `migrations/` set). Used by the restore command to decide whether forward
/// migrations are needed (constraint #13: forward-only).
#[must_use]
pub fn embedded_schema_version() -> i64 {
    sqlx::migrate!("../../migrations")
        .migrations
        .iter()
        .map(|m| m.version)
        .max()
        .unwrap_or(0)
}

/// Verify the database schema matches the migrations embedded in this
/// binary, refusing to serve on a stale, ahead, dirty, or tampered
/// schema (DS-AUD-B06).
///
/// Checks, in order:
/// 1. `_sqlx_migrations` exists and is non-empty → else `NotInitialized`.
/// 2. Every applied row has `success=1` → else `SchemaDirty`.
/// 3. Every applied `version` is known to the embedded `Migrator` → else
///    `SchemaAhead` (binary is older than DB; constraint #13).
/// 4. Every applied `checksum` matches the embedded checksum → else
///    `SchemaChecksumMismatch`.
/// 5. Every embedded migration is applied → else `SchemaBehind`.
///
/// This is validate-only: it never applies pending migrations. The
/// explicit `deve-sub migrate` subcommand owns migration; `serve` must
/// refuse a stale schema so the operator notices, rather than silently
/// running on a schema it does not understand.
///
/// # Errors
/// Returns [`StorageError`] with the specific failure mode.
pub async fn verify_schema(pool: &SqlitePool) -> Result<(), StorageError> {
    let migrator = sqlx::migrate!("../../migrations");

    let applied: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .map_err(|_| StorageError::NotInitialized)?;

    if applied.is_empty() {
        return Err(StorageError::NotInitialized);
    }

    let embedded: HashMap<i64, &[u8]> = migrator
        .migrations
        .iter()
        .map(|m| (m.version, m.checksum.as_ref()))
        .collect();

    for (version, success, checksum) in &applied {
        if !success {
            return Err(StorageError::SchemaDirty { version: *version });
        }
        match embedded.get(version) {
            None => return Err(StorageError::SchemaAhead { version: *version }),
            Some(expected) if *expected != checksum.as_slice() => {
                return Err(StorageError::SchemaChecksumMismatch { version: *version });
            }
            _ => {}
        }
    }

    let applied_versions: HashSet<i64> = applied.iter().map(|(v, _, _)| *v).collect();
    let missing: Vec<i64> = migrator
        .migrations
        .iter()
        .filter(|m| !applied_versions.contains(&m.version))
        .map(|m| m.version)
        .collect();
    if !missing.is_empty() {
        return Err(StorageError::SchemaBehind { missing });
    }

    Ok(())
}
