#![allow(clippy::expect_used)]

//! Regression tests for `SqliteRecoveryCodeRepository::mark_used_and_create_session`
//! (P0-11).
//!
//! Verifies that recovery-code consumption and session creation are atomic:
//! a session-insert failure rolls back the `used = 1` update, so the user
//! does not lose a recovery code without getting a session (AUTH-006).

use deve_sub_domain::{IdentityError, RecoveryCode, RecoveryCodeRepository, Session};
use deve_sub_kernel::{Timestamp, UserId};
use deve_sub_storage_sqlite::SqliteRecoveryCodeRepository;
use sqlx::sqlite::SqlitePool;

struct TestDb {
    pool: SqlitePool,
    _dir: tempfile::TempDir,
}

impl TestDb {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool =
            sqlx::sqlite::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
                .await
                .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migrations");
        Self { pool, _dir: dir }
    }

    async fn insert_user(&self, user_id: &UserId) {
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)")
            .bind(user_id.to_string())
            .bind("test_user")
            .bind("hash_placeholder")
            .execute(&self.pool)
            .await
            .expect("insert user");
    }

    async fn insert_recovery_code(&self, code: &RecoveryCode) {
        let created_at =
            deve_sub_storage_sqlite::timestamp::format_ts(code.created_at).expect("format ts");
        sqlx::query(
            "INSERT INTO recovery_codes (id, user_id, code_hash, used, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(code.id.to_string())
        .bind(code.user_id.to_string())
        .bind(&code.code_hash)
        .bind(code.used as i64)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .expect("insert recovery code");
    }

    async fn recovery_code_used(&self, code_id: &deve_sub_kernel::RecoveryCodeId) -> bool {
        let row: (i64,) = sqlx::query_as("SELECT used FROM recovery_codes WHERE id = ?")
            .bind(code_id.to_string())
            .fetch_one(&self.pool)
            .await
            .expect("fetch recovery code");
        row.0 != 0
    }

    async fn session_exists(&self, session_id: &deve_sub_kernel::SessionId) -> bool {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
            .fetch_one(&self.pool)
            .await
            .expect("count sessions");
        row.0 != 0
    }
}

fn make_recovery_code(user_id: UserId) -> RecoveryCode {
    RecoveryCode::new(user_id, "test-hash-aaaaaaaaaaaaaaaa".to_owned())
}

fn make_session(user_id: UserId) -> Session {
    let expires_at = Timestamp::now() + time::Duration::seconds(3600);
    Session::new(user_id, "test-session-hash".to_owned(), expires_at)
}

/// Happy path: code is consumed and session is created in one transaction.
#[tokio::test]
async fn mark_used_and_create_session_succeeds() {
    let db = TestDb::new().await;
    let repo = SqliteRecoveryCodeRepository::new(db.pool.clone());

    let user_id = UserId::new();
    db.insert_user(&user_id).await;

    let code = make_recovery_code(user_id);
    db.insert_recovery_code(&code).await;

    let session = make_session(user_id);
    repo.mark_used_and_create_session(code.id, &session)
        .await
        .expect("atomic op succeeds");

    assert!(db.recovery_code_used(&code.id).await, "code should be used");
    assert!(db.session_exists(&session.id).await, "session should exist");
}

/// Regression: if the session INSERT fails (duplicate PK), the recovery-code
/// consumption must roll back — the code stays `used = 0` so the user can
/// retry without losing a code.
#[tokio::test]
async fn mark_used_and_create_session_rolls_back_on_duplicate_session_id() {
    let db = TestDb::new().await;
    let repo = SqliteRecoveryCodeRepository::new(db.pool.clone());

    let user_id = UserId::new();
    db.insert_user(&user_id).await;

    let code = make_recovery_code(user_id);
    db.insert_recovery_code(&code).await;

    let session = make_session(user_id);

    // Pre-insert a session row with the same ID to force a PK conflict inside
    // the transaction.
    let created_at =
        deve_sub_storage_sqlite::timestamp::format_ts(session.created_at).expect("format ts");
    let expires_at =
        deve_sub_storage_sqlite::timestamp::format_ts(session.expires_at).expect("format ts");
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, created_at, expires_at, revoked) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(session.id.to_string())
    .bind(session.user_id.to_string())
    .bind("pre-existing-hash")
    .bind(created_at)
    .bind(expires_at)
    .bind(0_i64)
    .execute(&db.pool)
    .await
    .expect("pre-insert session");

    let result = repo.mark_used_and_create_session(code.id, &session).await;

    assert!(
        matches!(result, Err(IdentityError::Storage(_))),
        "expected Storage error on duplicate session PK, got {result:?}"
    );

    // P0-11 core assertion: the recovery code must NOT be consumed.
    assert!(
        !db.recovery_code_used(&code.id).await,
        "recovery code must remain unused after rollback"
    );
}

/// Concurrency guard: if the code was already used by another request,
/// `mark_used_and_create_session` returns `RecoveryCodeNotFound` and does
/// not insert a session.
#[tokio::test]
async fn mark_used_and_create_session_returns_not_found_for_used_code() {
    let db = TestDb::new().await;
    let repo = SqliteRecoveryCodeRepository::new(db.pool.clone());

    let user_id = UserId::new();
    db.insert_user(&user_id).await;

    let mut code = make_recovery_code(user_id);
    code.used = true;
    db.insert_recovery_code(&code).await;

    let session = make_session(user_id);
    let result = repo.mark_used_and_create_session(code.id, &session).await;

    assert!(
        matches!(result, Err(IdentityError::RecoveryCodeNotFound)),
        "expected RecoveryCodeNotFound for already-used code, got {result:?}"
    );
    assert!(
        !db.session_exists(&session.id).await,
        "session must not be created when code is already used"
    );
}
