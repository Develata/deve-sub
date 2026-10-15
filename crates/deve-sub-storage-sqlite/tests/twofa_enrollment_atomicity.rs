#![allow(clippy::expect_used)]

//! AUTH-005/006: real SQLite plus application commands, with barriers that
//! reproduce stale enrollment reads and triggers that force rollback paths.

#[path = "twofa_enrollment/support.rs"]
mod support;

use deve_sub_application::auth::{self, AuthError};
use deve_sub_domain::{IdentityError, RecoveryCode, RecoveryCodeRepository, TotpSecretRepository};
use deve_sub_kernel::UserId;
use support::{Fixture, PASSWORD, PausePoint, PausedTotp, bounded};

#[tokio::test]
async fn stale_verified_secret_must_not_enable_replacement() {
    let f = Fixture::new().await;
    let code = f.setup().await;
    let paused = PausedTotp::new(&f.totp, PausePoint::AfterSecretRead, 2);
    let verify = auth::verify_2fa(&f.users, &paused, &f.key, f.user.id, code);
    let replace = async {
        paused.ready.wait().await;
        f.setup().await;
        let replacement = f.snapshot().await;
        paused.release.wait().await;
        replacement
    };
    let (result, replacement) = bounded(async { tokio::join!(verify, replace) }).await;
    assert!(matches!(result, Err(AuthError::InvalidTwoFactorCode)));
    assert!(
        f.snapshot().await == replacement,
        "stale verification must not change any state"
    );
    assert!(!f.enabled().await);
}

#[tokio::test]
async fn two_concurrent_verifications_must_have_one_winner() {
    let f = Fixture::new().await;
    let code = f.setup().await;
    let secret = f.secret().await;
    let paused = PausedTotp::new(&f.totp, PausePoint::AfterSecretRead, 3);
    let first = auth::verify_2fa(&f.users, &paused, &f.key, f.user.id, code);
    let second = auth::verify_2fa(&f.users, &paused, &f.key, f.user.id, code);
    let sync = async {
        paused.ready.wait().await;
        paused.release.wait().await;
    };
    let (first, second, ()) = bounded(async { tokio::join!(first, second, sync) }).await;
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let (Ok(winner), Err(AuthError::TwoFactorAlreadyEnabled)) = (if first.is_ok() {
        (first, second)
    } else {
        (second, first)
    }) else {
        panic!("exactly one success and one already-enabled conflict are required")
    };
    assert_eq!(winner.recovery_codes.len(), 10);
    assert_eq!(f.usable_codes(&winner.recovery_codes).await, 10);
    assert!(f.enabled().await);
    assert!(f.secret().await == secret);
}

#[tokio::test]
async fn delayed_setup_must_not_replace_enabled_secret() {
    let f = Fixture::new().await;
    let code = f.setup().await;
    let paused = PausedTotp::new(&f.totp, PausePoint::BeforeSetupWrite, 2);
    let setup = auth::setup_2fa(&f.users, &paused, &f.key, f.user.id, "Test issuer");
    let verify = async {
        paused.ready.wait().await;
        let result = auth::verify_2fa(&f.users, &f.totp, &f.key, f.user.id, code)
            .await
            .expect("enable before late setup");
        assert!(
            f.totp
                .record_used_timestep(f.user.id, 1234)
                .await
                .expect("test login timestep")
        );
        let active = f.snapshot().await;
        paused.release.wait().await;
        (active, result.recovery_codes)
    };
    let (result, (active, codes)) = bounded(async { tokio::join!(setup, verify) }).await;
    assert!(matches!(result, Err(AuthError::TwoFactorAlreadyEnabled)));
    assert!(
        f.snapshot().await == active,
        "late setup must preserve active secret, replay state and codes"
    );
    assert_eq!(f.usable_codes(&codes).await, 10);
}

#[tokio::test]
async fn stale_disable_must_preserve_new_enrollment() {
    let f = Fixture::new().await;
    f.enable().await;
    let paused = PausedTotp::new(&f.totp, PausePoint::AfterSecretRead, 2);
    let disable = auth::disable_2fa(&f.users, &paused, f.user.id, PASSWORD);
    let reenable = async {
        paused.ready.wait().await;
        auth::disable_2fa(&f.users, &f.totp, f.user.id, PASSWORD)
            .await
            .expect("disable original enrollment");
        let codes = f.enable().await;
        let latest = f.snapshot().await;
        paused.release.wait().await;
        (latest, codes)
    };
    let (result, (latest, codes)) = bounded(async { tokio::join!(disable, reenable) }).await;
    assert!(matches!(result, Err(AuthError::TwoFactorNotEnabled)));
    assert!(
        f.snapshot().await == latest,
        "stale disable must preserve the entire new enrollment"
    );
    assert!(f.enabled().await);
    assert_eq!(f.usable_codes(&codes).await, 10);
}

#[tokio::test]
async fn stale_regeneration_must_preserve_new_enrollment() {
    let f = Fixture::new().await;
    f.enable().await;
    let paused = PausedTotp::new(&f.totp, PausePoint::AfterSecretRead, 2);
    let regenerate =
        auth::regenerate_recovery_codes(&f.users, &paused, &f.key, f.user.id, PASSWORD);
    let reenable = async {
        paused.ready.wait().await;
        auth::disable_2fa(&f.users, &f.totp, f.user.id, PASSWORD)
            .await
            .expect("disable original enrollment");
        let codes = f.enable().await;
        let latest = f.snapshot().await;
        paused.release.wait().await;
        (latest, codes)
    };
    let (result, (latest, codes)) = bounded(async { tokio::join!(regenerate, reenable) }).await;
    assert!(matches!(result, Err(AuthError::TwoFactorNotEnabled)));
    assert!(
        f.snapshot().await == latest,
        "stale regeneration must preserve the entire new enrollment"
    );
    assert!(f.enabled().await);
    assert_eq!(f.usable_codes(&codes).await, 10);
}

#[tokio::test]
async fn concurrent_regenerations_for_same_enrollment_commit_in_order() {
    let f = Fixture::new().await;
    let original = f.enable().await;
    let secret = f.secret().await;
    let paused = PausedTotp::new(&f.totp, PausePoint::AfterSecretRead, 3);
    let first = auth::regenerate_recovery_codes(&f.users, &paused, &f.key, f.user.id, PASSWORD);
    let second = auth::regenerate_recovery_codes(&f.users, &paused, &f.key, f.user.id, PASSWORD);
    let sync = async {
        paused.ready.wait().await;
        paused.release.wait().await;
    };
    let (first, second, ()) = bounded(async { tokio::join!(first, second, sync) }).await;
    let first = first.expect("first regeneration");
    let second = second.expect("second regeneration");
    let mut counts = [f.usable_codes(&first).await, f.usable_codes(&second).await];
    counts.sort();
    assert_eq!(counts, [0, 10]);
    assert_eq!(f.usable_codes(&original).await, 0);
    assert!(f.enabled().await);
    assert!(f.secret().await == secret);
}

#[tokio::test]
async fn enrollment_write_failures_roll_back_flag_secret_and_recovery_codes() {
    for trigger in [
        "CREATE TRIGGER reject_enable BEFORE UPDATE OF two_factor_enabled ON users BEGIN SELECT RAISE(ABORT, 'injected flag failure'); END",
        "CREATE TRIGGER reject_codes BEFORE INSERT ON recovery_codes BEGIN SELECT RAISE(ABORT, 'injected recovery failure'); END",
    ] {
        let f = Fixture::new().await;
        let code = f.setup().await;
        let prior = [RecoveryCode::new(
            f.user.id,
            "prior-test-only-code-hash".to_owned(),
        )];
        f.recovery
            .replace_all_for_user(f.user.id, &prior)
            .await
            .expect("existing orphan recovery batch");
        let before = f.snapshot().await;
        sqlx::query(trigger)
            .execute(&f.pool)
            .await
            .expect("failure trigger");
        let result = auth::verify_2fa(&f.users, &f.totp, &f.key, f.user.id, code).await;
        assert!(matches!(
            result,
            Err(AuthError::Identity(IdentityError::Storage(_)))
        ));
        assert!(
            f.snapshot().await == before,
            "failed enrollment must restore all prior state"
        );
    }
}

#[tokio::test]
async fn disable_delete_failure_rolls_back_active_enrollment() {
    let f = Fixture::new().await;
    let codes = f.enable().await;
    let before = f.snapshot().await;
    sqlx::query("CREATE TRIGGER reject_delete BEFORE DELETE ON recovery_codes BEGIN SELECT RAISE(ABORT, 'injected delete failure'); END")
        .execute(&f.pool).await.expect("failure trigger");
    let result = auth::disable_2fa(&f.users, &f.totp, f.user.id, PASSWORD).await;
    assert!(matches!(
        result,
        Err(AuthError::Identity(IdentityError::Storage(_)))
    ));
    assert!(
        f.snapshot().await == before,
        "failed disable must restore flag, deleted secret and codes"
    );
    assert_eq!(f.usable_codes(&codes).await, 10);
}

#[tokio::test]
async fn regeneration_insert_failure_preserves_existing_batch() {
    let f = Fixture::new().await;
    let codes = f.enable().await;
    let before = f.snapshot().await;
    sqlx::query("CREATE TRIGGER reject_insert BEFORE INSERT ON recovery_codes BEGIN SELECT RAISE(ABORT, 'injected insert failure'); END")
        .execute(&f.pool).await.expect("failure trigger");
    let result =
        auth::regenerate_recovery_codes(&f.users, &f.totp, &f.key, f.user.id, PASSWORD).await;
    assert!(matches!(
        result,
        Err(AuthError::Identity(IdentityError::Storage(_)))
    ));
    assert!(
        f.snapshot().await == before,
        "failed regeneration must retain every existing code"
    );
    assert_eq!(f.usable_codes(&codes).await, 10);
}

#[tokio::test]
async fn mismatched_recovery_owner_cannot_enable_another_user() {
    let f = Fixture::new().await;
    f.setup().await;
    let before = f.snapshot().await;
    let wrong_owner = [RecoveryCode::new(
        UserId::new(),
        "other-test-only-code-hash".to_owned(),
    )];
    let result = f
        .totp
        .enable_if_current(&f.secret().await, &wrong_owner)
        .await;
    assert!(matches!(result, Err(IdentityError::Storage(_))));
    assert!(
        f.snapshot().await == before,
        "invalid ownership must roll back the enabled flag"
    );
}
