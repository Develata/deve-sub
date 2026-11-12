//! GEN-015: ciphertext history cannot impersonate a current producer.
use super::*;
use deve_sub_storage_sqlite::{migrate_generation_cache, run_migrations};

#[tokio::test]
async fn encrypted_v6_upgrade_keeps_history_but_never_serves_or_promotes_it() {
    let f = Fixture::new(29).await;
    let mut old = f.entry();
    old.cache_key = "reserved-pre-v7-lossy-output".into();
    let envelope = deve_sub_security::envelope::seal(
        f.key.as_bytes(),
        format!("generation_cache.content\0{}\0{}", old.id, old.cache_key).as_bytes(),
        old.content.as_bytes(),
    )
    .expect("historical envelope");
    sqlx::query("INSERT INTO generation_cache (id, template_id, template_version, profile, mode, selection_mode, selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active) VALUES (?, ?, 3, 'mihomo', 'lenient', 'fixed', ?, 7, ?, ?, '', 1)")
        .bind(old.id.to_string()).bind(f.template.to_string()).bind(&old.selection_payload)
        .bind(&old.cache_key).bind(&envelope).execute(&f.pool).await.expect("v6 active");
    run_migrations(&f.pool).await.expect("upgrade 0031");
    migrate_generation_cache(&f.pool, &f.key)
        .await
        .expect("authenticate historical bytes");
    for _ in 0..2 {
        assert!(
            f.repository
                .find_by_key(&old.cache_key)
                .await
                .expect("direct")
                .is_none()
        );
        assert!(
            f.repository
                .find_active(f.template, "mihomo")
                .await
                .expect("active")
                .is_none()
        );
        assert!(
            f.repository
                .find_latest(
                    f.template,
                    "mihomo",
                    "fixed",
                    &old.selection_payload,
                    None,
                    "lenient"
                )
                .await
                .expect("last good")
                .is_none()
        );
        assert!(
            f.repository
                .activate(f.template, "mihomo", old.id)
                .await
                .is_err()
        );
        assert!(f.repository.store(&old).await.is_err());
        assert_eq!(f.encrypted(old.id).await, envelope);
        let row: (i64, i64) = sqlx::query_as(
            "SELECT semantics_version, is_active FROM generation_cache WHERE id = ?",
        )
        .bind(old.id.to_string())
        .fetch_one(&f.pool)
        .await
        .expect("history");
        assert_eq!(row, (0, 1));
        assert_eq!(
            migrate_generation_cache(&f.pool, &f.key)
                .await
                .expect("retry"),
            0
        );
    }
    let mut fresh = f.entry();
    fresh.is_active = false;
    f.repository
        .store(&fresh)
        .await
        .expect("current generation");
    f.repository
        .activate(f.template, "mihomo", fresh.id)
        .await
        .expect("publish");
    assert_eq!(
        f.repository
            .find_active(f.template, "mihomo")
            .await
            .expect("active")
            .expect("qualified")
            .id,
        fresh.id
    );
    assert_eq!(f.encrypted(old.id).await, envelope);
}

#[tokio::test]
async fn newer_unqualified_row_cannot_mask_or_evict_current_last_good() {
    let f = Fixture::new(i64::MAX).await;
    sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES ('owner', 'owner', 'test', 'admin')").execute(&f.pool).await.expect("owner");
    let mut good = f.entry();
    good.is_active = false;
    f.repository
        .store(&good)
        .await
        .expect("qualified last good");
    sqlx::query("INSERT INTO subscriptions (id, name, slug, owner_id, template_id, profile, node_selection, token_id) VALUES ('sub', 'sub', 'sub', 'owner', ?, 'mihomo', ?, 'test-token')")
        .bind(f.template.to_string()).bind(&good.selection_payload).execute(&f.pool).await.expect("subscription");
    // A historical row sorts after the qualified result and matches its selector.
    let newer = GenerationCacheId::parse("7ZZZZZZZZZZZZZZZZZZZZZZZZZ").expect("newer id");
    let key = "pre-v7-history";
    let encrypted = deve_sub_security::envelope::seal(
        f.key.as_bytes(),
        format!("generation_cache.content\0{newer}\0{key}").as_bytes(),
        b"pinless historical fixture",
    )
    .expect("envelope");
    sqlx::query("INSERT INTO generation_cache (id, template_id, template_version, profile, mode, selection_mode, selection_payload, pool_revision, cache_key, content_encrypted, content_legacy, is_active) VALUES (?, ?, 3, 'mihomo', 'lenient', 'fixed', ?, 7, ?, ?, '', 0)")
        .bind(newer.to_string()).bind(f.template.to_string()).bind(&good.selection_payload).bind(key).bind(encrypted).execute(&f.pool).await.expect("historical row");
    for _ in 0..20 {
        let mut other = f.entry();
        other.is_active = false;
        f.repository.store(&other).await.expect("retention churn");
    }
    let found = f
        .repository
        .find_latest(
            f.template,
            "mihomo",
            "fixed",
            &good.selection_payload,
            Some(3),
            "lenient",
        )
        .await
        .expect("fallback")
        .expect("current protected result");
    assert_eq!(found.id, good.id);
    assert!(
        f.repository
            .find_by_key(key)
            .await
            .expect("history direct")
            .is_none()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM generation_cache WHERE id = ?")
            .bind(newer.to_string())
            .fetch_one(&f.pool)
            .await
            .expect("history retained"),
        1
    );
    sqlx::query("UPDATE pool_meta SET revision = 8, withdrawal_revision = 8")
        .execute(&f.pool)
        .await
        .expect("withdrawal");
    assert!(
        f.repository
            .find_latest(
                f.template,
                "mihomo",
                "fixed",
                &good.selection_payload,
                None,
                "lenient"
            )
            .await
            .expect("withdrawal floor")
            .is_none()
    );
}
