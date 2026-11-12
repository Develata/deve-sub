//! Recovery preserves selectors, version pins and withdrawal fencing.
use super::*;
use deve_sub_storage_sqlite::{
    ensure_key_binding, migrate_generation_cache, run_migrations,
    verify_generation_cache_encryption,
};

async fn insert_legacy(f: &Fixture, entry: &GenerationCacheEntry) {
    sqlx::query(
        "INSERT INTO generation_cache (id, template_id, template_version, profile, mode, \
         selection_mode, selection_payload, pool_revision, cache_key, content, is_active) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry.id.to_string())
    .bind(entry.template_id.to_string())
    .bind(entry.template_version as i64)
    .bind(&entry.profile)
    .bind(&entry.mode)
    .bind(&entry.selection_mode)
    .bind(&entry.selection_payload)
    .bind(entry.pool_revision as i64)
    .bind(&entry.cache_key)
    .bind(&entry.content)
    .bind(entry.is_active)
    .execute(&f.pool)
    .await
    .expect("legacy");
}

async fn payloads(f: &Fixture) -> Vec<(String, String, Option<String>, i64)> {
    sqlx::query_as(
        "SELECT id, content_legacy, content_encrypted, is_active FROM generation_cache ORDER BY id",
    )
    .fetch_all(&f.pool)
    .await
    .expect("payloads")
}

#[tokio::test]
async fn legacy_migration_preserves_cache_identity_pins_modes_and_withdrawal_floor() {
    let f = Fixture::new(28).await;
    ensure_key_binding(&f.pool, &f.key.fingerprint().expect("fingerprint"))
        .await
        .expect("bind");
    sqlx::query("UPDATE pool_meta SET revision = 7, withdrawal_revision = 5")
        .execute(&f.pool)
        .await
        .expect("floor");
    let mut entries = Vec::new();
    for index in 0..12 {
        let mut entry = f.entry();
        entry.template_version = index / 3 + 1;
        entry.is_active = index == 1;
        entry.mode = if index % 2 == 0 { "strict" } else { "lenient" }.into();
        entry.pool_revision = if index == 0 { 4 } else { 7 };
        entry.selection_payload = format!(r#"{{"mode":"fixed","nodeIds":["fixture-{index}"]}}"#);
        insert_legacy(&f, &entry).await;
        entries.push(entry);
    }
    let rollback = f.snapshot("before-migration.db").await;
    run_migrations(&f.pool).await.expect("schema transition");
    assert!(verify_generation_cache_encryption(&f.pool).await.is_err());
    assert!(
        f.repository
            .find_by_key(&entries[1].cache_key)
            .await
            .expect("historical cache excluded before payload read")
            .is_none()
    );
    let before = payloads(&f).await;
    assert!(
        migrate_generation_cache(&f.pool, &MasterKey::from_bytes(&[0x99; 32]))
            .await
            .is_err()
    );
    assert_eq!(
        payloads(&f).await,
        before,
        "wrong key cannot mutate legacy payloads"
    );
    assert_eq!(
        migrate_generation_cache(&f.pool, &f.key)
            .await
            .expect("backfill"),
        12
    );
    verify_generation_cache_encryption(&f.pool)
        .await
        .expect("ready");
    for entry in &entries {
        let row: (String, i64, i64, i64) = sqlx::query_as(
            "SELECT cache_key, template_version, pool_revision, is_active FROM generation_cache WHERE id = ?")
            .bind(entry.id.to_string()).fetch_one(&f.pool).await.expect("preserved metadata");
        assert_eq!(
            row,
            (
                entry.cache_key.clone(),
                entry.template_version as i64,
                entry.pool_revision as i64,
                i64::from(entry.is_active)
            )
        );
        assert!(
            f.repository
                .find_by_key(&entry.cache_key)
                .await
                .expect("excluded historical direct")
                .is_none()
        );
        assert!(
            f.repository
                .find_latest(
                    f.template,
                    &entry.profile,
                    &entry.selection_mode,
                    &entry.selection_payload,
                    Some(entry.template_version),
                    &entry.mode
                )
                .await
                .expect("excluded historical fallback")
                .is_none()
        );
    }
    let qualifications: Vec<i64> =
        sqlx::query_scalar("SELECT semantics_version FROM generation_cache")
            .fetch_all(&f.pool)
            .await
            .expect("qualification");
    assert!(qualifications.iter().all(|v| *v == 0));
    assert!(
        f.repository
            .find_by_key(&entries[0].cache_key)
            .await
            .expect("floor")
            .is_none()
    );
    assert!(
        f.repository
            .activate(f.template, "mihomo", entries[0].id)
            .await
            .is_err()
    );
    let floor: (i64, i64) = sqlx::query_as("SELECT revision, withdrawal_revision FROM pool_meta")
        .fetch_one(&f.pool)
        .await
        .expect("floor unchanged");
    assert_eq!(floor, (7, 5));
    let encrypted = payloads(&f).await;
    assert_eq!(
        encrypted.len(),
        12,
        "migration does not prune retained outputs"
    );
    assert_eq!(
        migrate_generation_cache(&f.pool, &f.key)
            .await
            .expect("rerun"),
        0
    );
    assert_eq!(
        payloads(&f).await,
        encrypted,
        "retry does not rotate ciphertext"
    );
    assert_no_canary(&f.snapshot("after-migration.db").await);

    // Rollback is a separate historical snapshot, never a down migration or
    // reinterpretation of ciphertext by an older binary.
    let restored = create_pool(&SqliteConfig::new(rollback))
        .await
        .expect("rollback pool");
    let old_content: String =
        sqlx::query_scalar("SELECT content FROM generation_cache WHERE id = ?")
            .bind(entries[1].id.to_string())
            .fetch_one(&restored)
            .await
            .expect("old schema content");
    assert_eq!(old_content, entries[1].content);
    assert!(
        sqlx::query("SELECT content FROM generation_cache")
            .execute(&f.pool)
            .await
            .is_err()
    );
    run_migrations(&restored)
        .await
        .expect("reapply after rollback");
    assert_eq!(
        migrate_generation_cache(&restored, &f.key)
            .await
            .expect("recover"),
        12
    );
}

#[tokio::test]
async fn interrupted_backfill_rolls_back_all_payloads_and_key_binding() {
    let f = Fixture::new(28).await;
    let first = f.entry();
    let mut second = f.entry();
    second.is_active = false;
    insert_legacy(&f, &first).await;
    insert_legacy(&f, &second).await;
    run_migrations(&f.pool).await.expect("schema");
    let before = payloads(&f).await;
    sqlx::query(&format!(
        "CREATE TRIGGER fixture_disk_failure BEFORE UPDATE ON generation_cache \
         WHEN OLD.id = '{}' BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END",
        second.id,
    ))
    .execute(&f.pool)
    .await
    .expect("failure injection");
    assert!(migrate_generation_cache(&f.pool, &f.key).await.is_err());
    assert_eq!(
        payloads(&f).await,
        before,
        "first-row encryption must roll back"
    );
    let bindings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM key_metadata")
        .fetch_one(&f.pool)
        .await
        .expect("key binding");
    assert_eq!(bindings, 0, "key binding shares payload rollback");
    sqlx::query("DROP TRIGGER fixture_disk_failure")
        .execute(&f.pool)
        .await
        .expect("recover writes");
    assert_eq!(
        migrate_generation_cache(&f.pool, &f.key)
            .await
            .expect("retry"),
        2
    );
    assert!(
        f.repository
            .find_active(f.template, "mihomo")
            .await
            .expect("historical active excluded")
            .is_none()
    );
}

#[tokio::test]
async fn ciphertext_authentication_failure_never_reencrypts_as_legacy() {
    let f = Fixture::new(28).await;
    let legacy = f.entry();
    insert_legacy(&f, &legacy).await;
    run_migrations(&f.pool).await.expect("schema");
    let mut corrupted = f.entry();
    corrupted.is_active = false;
    f.repository
        .store(&corrupted)
        .await
        .expect("encrypted output");
    sqlx::query("UPDATE generation_cache SET content_encrypted = ? WHERE id = ?")
        .bind(format!("v2:{CANARY}:invalid"))
        .bind(corrupted.id.to_string())
        .execute(&f.pool)
        .await
        .expect("corrupt envelope");
    let before = payloads(&f).await;
    assert!(migrate_generation_cache(&f.pool, &f.key).await.is_err());
    assert_eq!(
        payloads(&f).await,
        before,
        "no fallback, no partial legacy backfill"
    );
    assert!(
        f.repository
            .find_by_key(&corrupted.cache_key)
            .await
            .is_err()
    );
    assert!(
        f.repository
            .find_by_key(&legacy.cache_key)
            .await
            .expect("excluded legacy")
            .is_none()
    );
}
