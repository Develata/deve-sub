use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use deve_sub_application::auth;
use deve_sub_domain::{
    IdentityError, RecoveryCode, RecoveryCodeRepository, Role, TotpSecret, TotpSecretRepository,
    User, UserRepository,
};
use deve_sub_kernel::UserId;
use deve_sub_security::{
    MasterKey, PURPOSE_RECOVERY, base32_decode, hash_password, hmac_digest,
    normalize_recovery_code, totp_generate_code,
};
use deve_sub_storage_sqlite::{
    SqliteConfig, SqliteRecoveryCodeRepository, SqliteTotpSecretRepository, SqliteUserRepository,
    create_pool, run_migrations,
};
use sqlx::SqlitePool;
use tokio::sync::Barrier;

pub const PASSWORD: &str = "isolated-enrollment-test-password";
static PASSWORD_HASH: LazyLock<String> =
    LazyLock::new(|| hash_password(PASSWORD).expect("synthetic test password"));

pub struct Fixture {
    pub pool: SqlitePool,
    pub users: SqliteUserRepository,
    pub totp: SqliteTotpSecretRepository,
    pub recovery: SqliteRecoveryCodeRepository,
    pub user: User,
    pub key: MasterKey,
    _dir: tempfile::TempDir,
}

// Deliberately no Debug: a failed comparison must not print stored credentials.
#[derive(PartialEq, Eq)]
pub struct Snapshot {
    enabled: bool,
    secret: Option<TotpSecret>,
    last_used_timestep: Option<i64>,
    recovery_codes: Vec<(String, String, bool)>,
}

impl Fixture {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().expect("isolated directory");
        let pool = create_pool(&SqliteConfig::new(dir.path().join("auth-test.db")))
            .await
            .expect("isolated pool");
        run_migrations(&pool).await.expect("migrations");
        let users = SqliteUserRepository::new(pool.clone());
        let user = User::new(
            "enrollment.test.invalid",
            PASSWORD_HASH.clone(),
            Role::Admin,
        );
        users.create(&user).await.expect("test user");
        Self {
            users,
            totp: SqliteTotpSecretRepository::new(pool.clone()),
            recovery: SqliteRecoveryCodeRepository::new(pool.clone()),
            pool,
            user,
            key: MasterKey::from_bytes(&[9; 32]),
            _dir: dir,
        }
    }

    pub async fn setup(&self) -> u32 {
        let result = auth::setup_2fa(
            &self.users,
            &self.totp,
            &self.key,
            self.user.id,
            "Test issuer",
        )
        .await
        .expect("setup");
        totp_generate_code(&base32_decode(&result.secret).expect("test secret"))
    }

    pub async fn enable(&self) -> Vec<String> {
        let code = self.setup().await;
        auth::verify_2fa(&self.users, &self.totp, &self.key, self.user.id, code)
            .await
            .expect("enable")
            .recovery_codes
    }

    pub async fn secret(&self) -> TotpSecret {
        self.totp
            .find_by_user(self.user.id)
            .await
            .expect("secret read")
            .expect("secret")
    }

    pub async fn enabled(&self) -> bool {
        self.users
            .find_by_id(self.user.id)
            .await
            .expect("user read")
            .expect("user")
            .two_factor_enabled
    }

    pub async fn usable_codes(&self, codes: &[String]) -> usize {
        let mut usable = 0;
        for code in codes {
            let hash = hmac_digest(
                PURPOSE_RECOVERY,
                &normalize_recovery_code(code),
                self.key.as_bytes(),
            )
            .expect("test code hash");
            usable += usize::from(
                self.recovery
                    .find_unused_by_hash(self.user.id, &hash)
                    .await
                    .expect("recovery read")
                    .is_some(),
            );
        }
        usable
    }

    pub async fn snapshot(&self) -> Snapshot {
        let recovery_codes = sqlx::query_as(
            "SELECT id, code_hash, used FROM recovery_codes WHERE user_id = ? ORDER BY id",
        )
        .bind(self.user.id.to_string())
        .fetch_all(&self.pool)
        .await
        .expect("recovery snapshot");
        let last_used_timestep: Option<Option<i64>> =
            sqlx::query_scalar("SELECT last_used_timestep FROM totp_secrets WHERE user_id = ?")
                .bind(self.user.id.to_string())
                .fetch_optional(&self.pool)
                .await
                .expect("timestep snapshot");
        Snapshot {
            enabled: self.enabled().await,
            secret: self
                .totp
                .find_by_user(self.user.id)
                .await
                .expect("secret snapshot"),
            last_used_timestep: last_used_timestep.flatten(),
            recovery_codes,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum PausePoint {
    AfterSecretRead,
    BeforeSetupWrite,
}

/// Both barriers are required: a competing commit happens after all readers
/// captured the old state and before any of them may continue. No sleeps.
pub struct PausedTotp<'a> {
    inner: &'a SqliteTotpSecretRepository,
    point: PausePoint,
    pub ready: Barrier,
    pub release: Barrier,
}

impl<'a> PausedTotp<'a> {
    pub fn new(
        inner: &'a SqliteTotpSecretRepository,
        point: PausePoint,
        participants: usize,
    ) -> Self {
        Self {
            inner,
            point,
            ready: Barrier::new(participants),
            release: Barrier::new(participants),
        }
    }

    async fn pause(&self, point: PausePoint) {
        if self.point == point {
            self.ready.wait().await;
            self.release.wait().await;
        }
    }
}

#[async_trait]
impl TotpSecretRepository for PausedTotp<'_> {
    async fn upsert(&self, secret: &TotpSecret) -> Result<(), IdentityError> {
        self.pause(PausePoint::BeforeSetupWrite).await;
        self.inner.upsert(secret).await
    }

    async fn find_by_user(&self, user_id: UserId) -> Result<Option<TotpSecret>, IdentityError> {
        let secret = self.inner.find_by_user(user_id).await?;
        self.pause(PausePoint::AfterSecretRead).await;
        Ok(secret)
    }

    async fn enable_if_current(
        &self,
        expected: &TotpSecret,
        codes: &[RecoveryCode],
    ) -> Result<bool, IdentityError> {
        self.inner.enable_if_current(expected, codes).await
    }

    async fn disable_if_current(&self, expected: &TotpSecret) -> Result<bool, IdentityError> {
        self.inner.disable_if_current(expected).await
    }

    async fn replace_recovery_codes_if_current(
        &self,
        expected: &TotpSecret,
        codes: &[RecoveryCode],
    ) -> Result<bool, IdentityError> {
        self.inner
            .replace_recovery_codes_if_current(expected, codes)
            .await
    }

    async fn record_used_timestep(
        &self,
        user_id: UserId,
        timestep: u64,
    ) -> Result<bool, IdentityError> {
        self.inner.record_used_timestep(user_id, timestep).await
    }

    async fn delete(&self, user_id: UserId) -> Result<(), IdentityError> {
        self.inner.delete(user_id).await
    }
}

pub async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .expect("barrier scenario deadline")
}
