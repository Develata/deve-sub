#![allow(clippy::expect_used)]
//! NODE-010/GEN-015: exact old-ID isolation and in-place TLS repair.
use deve_sub_domain::source::recovery::{NodeSecurityAction, NodeSecurityChange, NodeSecurityPlan};
use deve_sub_domain::{CertificatePin, NodeFilter, NodePoolRepository};
use deve_sub_kernel::NodeId;
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{SqliteNodePoolRepository, ensure_key_binding};
use sqlx::{Row, SqlitePool};
use std::{collections::BTreeMap, sync::Arc};

struct Fixture {
    pool: SqlitePool,
    repo: SqliteNodePoolRepository,
    key: Arc<MasterKey>,
    ids: Vec<NodeId>,
    _dir: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = SqlitePool::connect(&format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("db").display()
        ))
        .await
        .expect("pool");
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("schema");
        let key = Arc::new(MasterKey::from_bytes(&[0x42; 32]));
        ensure_key_binding(&pool, &key.fingerprint().expect("fingerprint"))
            .await
            .expect("binding");
        let repo = SqliteNodePoolRepository::new_with_key(pool.clone(), key.clone());
        let mut nodes = Vec::new();
        for password in ["TEST_A", "TEST_B", "TEST_C", "TEST_D"] {
            // Deliberately identical names and endpoints; credentials distinguish IDs.
            nodes.push(
                deve_sub_protocol::parse_uri(&format!(
                    "trojan://{password}@reserved.example:443?sni=reserved.example#same-name"
                ))
                .expect("fixture"),
            );
        }
        let ids = nodes.iter().map(|n| n.id).collect();
        repo.import_nodes(nodes).await.expect("import");
        Self {
            pool,
            repo,
            key,
            ids,
            _dir: dir,
        }
    }
    async fn plan(&self, index: usize, change: NodeSecurityChange, hash: char) -> NodeSecurityPlan {
        let snapshot = self
            .repo
            .security_snapshot(self.ids[index])
            .await
            .expect("base snapshot");
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM pool_meta WHERE id=1")
            .fetch_one(&self.pool)
            .await
            .expect("pool revision");
        NodeSecurityPlan {
            plan_sha256: hash.to_string().repeat(64),
            key_fingerprint: self.key.fingerprint().expect("key"),
            expected_pool_revision: revision as u64,
            actions: vec![NodeSecurityAction {
                node_id: self.ids[index],
                expected_revision: snapshot.revision,
                expected_fingerprint: snapshot.fingerprint,
                evidence_sha256: "e".repeat(64),
                change,
            }],
        }
    }
    async fn pins(&self, index: usize) -> deve_sub_domain::TlsConfig {
        let mut tls = self
            .repo
            .security_snapshot(self.ids[index])
            .await
            .expect("base")
            .node
            .tls
            .expect("TLS");
        tls.skip_cert_verify = Some(false);
        tls.certificate_pins = vec![CertificatePin::new(format!(
            "mihomoFingerprintSHA256:{}",
            "11".repeat(32)
        ))];
        tls
    }
}

/// Compare persisted values, including ciphertext, rather than effective DTOs.
async fn protected(
    pool: &SqlitePool,
    omit_node_security: bool,
) -> BTreeMap<String, Vec<Vec<String>>> {
    let tables:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'node_security_%' AND name NOT IN ('_sqlx_migrations','pool_meta') ORDER BY name").fetch_all(pool).await.expect("tables");
    let mut snapshot = BTreeMap::new();
    for table in tables {
        let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
            sqlx::query_as(&format!("PRAGMA table_info(\"{table}\")"))
                .fetch_all(pool)
                .await
                .expect("columns");
        let selected = columns
            .iter()
            .filter(|(_, name, _, _, _, _)| {
                !omit_node_security
                    || table != "nodes"
                    || !matches!(
                        name.as_str(),
                        "revision" | "identity_fingerprint" | "tls_json_encrypted"
                    )
            })
            .map(|(_, name, _, _, _, _)| format!("quote(\"{name}\")"))
            .collect::<Vec<_>>();
        let rows = sqlx::query(&format!(
            "SELECT {} FROM \"{table}\" ORDER BY rowid",
            selected.join(",")
        ))
        .fetch_all(pool)
        .await
        .expect("snapshot");
        snapshot.insert(
            table,
            rows.iter()
                .map(|row| {
                    (0..selected.len())
                        .map(|i| row.get::<String, _>(i))
                        .collect()
                })
                .collect(),
        );
    }
    snapshot
}

#[tokio::test]
async fn exact_quarantine_preserves_settings_references_and_chain_closure() {
    let f = Fixture::new().await;
    for sql in [
        "INSERT INTO users(id,username,password_hash,role) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA05','reserved','test','admin')",
        "INSERT INTO templates(id,name) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA03','reserved')",
        "INSERT INTO tags(id,name,color) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA01','reserved','#000000')",
        "INSERT INTO sources(id,name,source_type) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA02','reserved','uri_list')",
    ] {
        sqlx::query(sql)
            .execute(&f.pool)
            .await
            .expect("reference fixture");
    }
    sqlx::query("INSERT INTO template_versions(id,template_id,version,spec_json,spec_yaml) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA04','01KZAAAAAAAAAAAAAAAAAAAA03',3,?,?)").bind(format!("{{\"oldId\":\"{}\"}}",f.ids[0])).bind(format!("oldId: {}",f.ids[0])).execute(&f.pool).await.expect("history");
    sqlx::query("INSERT INTO subscriptions(id,name,slug,owner_id,template_id,template_version_pin,profile,node_selection,token_id) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA06','reserved','reserved','01KZAAAAAAAAAAAAAAAAAAAA05','01KZAAAAAAAAAAAAAAAAAAAA03',3,'mihomo',?,'01KZAAAAAAAAAAAAAAAAAAAA07')").bind(format!("{{\"mode\":\"fixed\",\"nodeIds\":[\"{}\"]}}",f.ids[0])).execute(&f.pool).await.expect("subscription");
    sqlx::query("INSERT INTO subscription_tokens(id,subscription_id,token_digest) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA07','01KZAAAAAAAAAAAAAAAAAAAA06','reserved-test-hmac')").execute(&f.pool).await.expect("token");
    sqlx::query(
        "INSERT INTO node_source_bindings(id,node_id,source_id) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA08',?,'01KZAAAAAAAAAAAAAAAAAAAA02')",
    )
    .bind(f.ids[0].to_string())
    .execute(&f.pool)
    .await
    .expect("binding");
    sqlx::query("INSERT INTO node_tags(node_id,tag_id) VALUES (?,'01KZAAAAAAAAAAAAAAAAAAAA01')")
        .bind(f.ids[0].to_string())
        .execute(&f.pool)
        .await
        .expect("tag");
    sqlx::query("INSERT INTO node_overrides(id,node_id,enabled,sni,skip_cert_verify,fingerprint,sort_order) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA00',?,1,'override.example',0,'chrome',42)").bind(f.ids[0].to_string()).execute(&f.pool).await.expect("enabled override");
    f.repo
        .set_node_chain(f.ids[2], Some(&[f.ids[0]]))
        .await
        .expect("parent");
    f.repo
        .set_node_chain(f.ids[3], Some(&[f.ids[2]]))
        .await
        .expect("grandparent");
    let before = protected(&f.pool, true).await;
    let plan = f.plan(0, NodeSecurityChange::Quarantine, 'a').await;
    assert!(f.repo.apply_security_plan(&plan).await.expect("quarantine"));
    assert!(
        !f.repo
            .apply_security_plan(&plan)
            .await
            .expect("verified replay")
    );
    assert_eq!(before, protected(&f.pool, true).await);
    let mut blocked = f.repo.security_blocked_ids().await.expect("closure");
    blocked.sort();
    let mut expected = vec![f.ids[0], f.ids[2], f.ids[3]];
    expected.sort();
    assert_eq!(blocked, expected);
    for (i, id) in f.ids.iter().enumerate() {
        let entry = f.repo.get_node(*id).await.expect("get").expect("kept");
        assert_eq!(entry.security_quarantined, i != 1);
        assert_eq!(entry.is_active, i == 1);
    }
    let active = f
        .repo
        .list_nodes(&NodeFilter::default(), None, 100)
        .await
        .expect("eligible");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].node.id, f.ids[1]);
    let floor: (i64, i64) =
        sqlx::query_as("SELECT revision,withdrawal_revision FROM pool_meta WHERE id=1")
            .fetch_one(&f.pool)
            .await
            .expect("floor");
    assert_eq!(floor.0, floor.1);
    // A later user enable cannot clear the independent security state.
    sqlx::query("UPDATE node_overrides SET enabled=1 WHERE node_id=?")
        .bind(f.ids[0].to_string())
        .execute(&f.pool)
        .await
        .expect("enable");
    assert!(
        !f.repo
            .get_node(f.ids[0])
            .await
            .expect("get")
            .expect("kept")
            .is_active
    );
    // Exercise repair against the same nonempty historical reference fixture.
    f.repo
        .apply_security_plan(
            &f.plan(0, NodeSecurityChange::RestoreTls(f.pins(0).await), 'b')
                .await,
        )
        .await
        .expect("repair with all references populated");
    assert_eq!(
        before,
        protected(&f.pool, true).await,
        "tags, bindings, template history, fixed subscriptions, token and chain references survive repair"
    );
    assert!(
        f.repo
            .security_blocked_ids()
            .await
            .expect("closure released")
            .is_empty()
    );
}

#[tokio::test]
async fn tls_repair_keeps_old_id_ciphertext_overrides_and_disabled_state() {
    let f = Fixture::new().await;
    sqlx::query("INSERT INTO node_overrides(id,node_id,enabled,sni,fingerprint) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA00',?,0,'override.example','chrome')").bind(f.ids[0].to_string()).execute(&f.pool).await.expect("disabled");
    let original_tls = f
        .repo
        .security_snapshot(f.ids[0])
        .await
        .expect("base")
        .node
        .tls;
    assert_ne!(
        original_tls,
        f.repo
            .get_node(f.ids[0])
            .await
            .expect("get")
            .expect("kept")
            .node
            .tls
    );
    let before = protected(&f.pool, true).await;
    f.repo
        .apply_security_plan(&f.plan(0, NodeSecurityChange::Quarantine, 'a').await)
        .await
        .expect("quarantine");
    let tls = f.pins(0).await;
    let repair = f
        .plan(0, NodeSecurityChange::RestoreTls(tls.clone()), 'b')
        .await;
    assert!(f.repo.apply_security_plan(&repair).await.expect("repair"));
    assert!(!f.repo.apply_security_plan(&repair).await.expect("replay"));
    assert_eq!(before, protected(&f.pool, true).await);
    let base = f
        .repo
        .security_snapshot(f.ids[0])
        .await
        .expect("repaired base");
    assert_eq!(base.node.id, f.ids[0]);
    assert_eq!(base.node.tls, Some(tls));
    assert_ne!(base.fingerprint, repair.actions[0].expected_fingerprint);
    let entry = f.repo.get_node(f.ids[0]).await.expect("get").expect("kept");
    assert!(!entry.security_quarantined);
    assert!(!entry.is_active);
    assert_eq!(
        entry.override_info.expect("override").sni.as_deref(),
        Some("override.example")
    );
}

#[tokio::test]
async fn wrong_key_stale_cas_collision_and_late_fault_are_atomic() {
    let f = Fixture::new().await;
    let mut plan = f.plan(0, NodeSecurityChange::Quarantine, 'a').await;
    let before = protected(&f.pool, false).await;
    plan.key_fingerprint = "0".repeat(64);
    assert!(f.repo.apply_security_plan(&plan).await.is_err());
    plan.key_fingerprint = f.key.fingerprint().expect("key");
    plan.actions[0].expected_revision += 1;
    assert!(f.repo.apply_security_plan(&plan).await.is_err());
    plan.actions[0].expected_revision -= 1;
    plan.actions.push(NodeSecurityAction {
        node_id: f.ids[1],
        expected_revision: 999,
        expected_fingerprint: "A".repeat(43),
        evidence_sha256: "e".repeat(64),
        change: NodeSecurityChange::Quarantine,
    });
    assert!(f.repo.apply_security_plan(&plan).await.is_err());
    assert_eq!(before, protected(&f.pool, false).await);
    assert!(
        f.repo
            .security_blocked_ids()
            .await
            .expect("no partial state")
            .is_empty()
    );
    let tls = f.pins(0).await;
    let mut duplicate = f.repo.security_snapshot(f.ids[0]).await.expect("base").node;
    duplicate.id = NodeId::new();
    duplicate.tls = Some(tls.clone());
    f.repo
        .import_nodes(vec![duplicate])
        .await
        .expect("other exact identity");
    f.repo
        .apply_security_plan(&f.plan(0, NodeSecurityChange::Quarantine, 'b').await)
        .await
        .expect("quarantine");
    let before = protected(&f.pool, false).await;
    assert!(
        f.repo
            .apply_security_plan(&f.plan(0, NodeSecurityChange::RestoreTls(tls), 'c').await)
            .await
            .is_err()
    );
    assert_eq!(before, protected(&f.pool, false).await);
    assert_eq!(
        f.repo.security_blocked_ids().await.expect("still blocked"),
        vec![f.ids[0]]
    );
}

#[tokio::test]
async fn reviewed_empty_batch_never_disables_nodes_or_changes_withdrawal() {
    let f = Fixture::new().await;
    let before = protected(&f.pool, false).await;
    let mut plan = f.plan(0, NodeSecurityChange::Quarantine, 'a').await;
    plan.actions.clear();
    let floor: (i64, i64) =
        sqlx::query_as("SELECT revision,withdrawal_revision FROM pool_meta WHERE id=1")
            .fetch_one(&f.pool)
            .await
            .expect("floor");
    assert!(
        f.repo
            .apply_security_plan(&plan)
            .await
            .expect("empty review")
    );
    assert!(!f.repo.apply_security_plan(&plan).await.expect("replay"));
    assert_eq!(before, protected(&f.pool, false).await);
    let after: (i64, i64) =
        sqlx::query_as("SELECT revision,withdrawal_revision FROM pool_meta WHERE id=1")
            .fetch_one(&f.pool)
            .await
            .expect("floor");
    assert_eq!(floor, after);
    assert_eq!(
        f.repo
            .list_nodes(&NodeFilter::default(), None, 100)
            .await
            .expect("all eligible")
            .len(),
        4
    );
}

#[tokio::test]
async fn ignored_tls_update_cannot_record_repaired_or_release_quarantine() {
    let f = Fixture::new().await;
    f.repo
        .apply_security_plan(&f.plan(0, NodeSecurityChange::Quarantine, 'a').await)
        .await
        .expect("quarantine");
    let before = protected(&f.pool, false).await;
    let floor: (i64, i64) =
        sqlx::query_as("SELECT revision,withdrawal_revision FROM pool_meta WHERE id=1")
            .fetch_one(&f.pool)
            .await
            .expect("floor");
    sqlx::query("CREATE TRIGGER ignore_repair BEFORE UPDATE OF tls_json_encrypted ON nodes BEGIN SELECT RAISE(IGNORE); END").execute(&f.pool).await.expect("fault");
    assert!(
        f.repo
            .apply_security_plan(
                &f.plan(0, NodeSecurityChange::RestoreTls(f.pins(0).await), 'b')
                    .await
            )
            .await
            .is_err()
    );
    assert_eq!(before, protected(&f.pool, false).await);
    let after: (i64, i64) =
        sqlx::query_as("SELECT revision,withdrawal_revision FROM pool_meta WHERE id=1")
            .fetch_one(&f.pool)
            .await
            .expect("floor");
    assert_eq!(floor, after);
    assert_eq!(
        f.repo.security_blocked_ids().await.expect("still blocked"),
        vec![f.ids[0]]
    );
    let batches: i64 = sqlx::query_scalar("SELECT count(*) FROM node_security_review_batches")
        .fetch_one(&f.pool)
        .await
        .expect("batches");
    assert_eq!(batches, 1);
}

#[tokio::test]
async fn altered_result_and_different_replay_policy_are_not_verified_repairs() {
    let f = Fixture::new().await;
    f.repo
        .apply_security_plan(&f.plan(0, NodeSecurityChange::Quarantine, 'a').await)
        .await
        .expect("quarantine");
    let repair = f
        .plan(0, NodeSecurityChange::RestoreTls(f.pins(0).await), 'b')
        .await;
    sqlx::query("CREATE TRIGGER corrupt_repair AFTER UPDATE OF tls_json_encrypted ON nodes BEGIN UPDATE nodes SET tls_json_encrypted=OLD.tls_json_encrypted WHERE id=NEW.id; END").execute(&f.pool).await.expect("fault");
    assert!(f.repo.apply_security_plan(&repair).await.is_err());
    assert_eq!(
        f.repo
            .security_blocked_ids()
            .await
            .expect("still quarantined"),
        vec![f.ids[0]]
    );
    sqlx::query("DROP TRIGGER corrupt_repair")
        .execute(&f.pool)
        .await
        .expect("remove synthetic fault");
    f.repo.apply_security_plan(&repair).await.expect("repair");
    let mut different = f.pins(0).await;
    different.certificate_pins = vec![CertificatePin::new(format!(
        "spkiSHA256:{}",
        "A".repeat(43)
    ))];
    let bad_replay = NodeSecurityPlan {
        plan_sha256: repair.plan_sha256.clone(),
        key_fingerprint: repair.key_fingerprint.clone(),
        expected_pool_revision: repair.expected_pool_revision,
        actions: vec![NodeSecurityAction {
            node_id: repair.actions[0].node_id,
            expected_revision: repair.actions[0].expected_revision,
            expected_fingerprint: repair.actions[0].expected_fingerprint.clone(),
            evidence_sha256: repair.actions[0].evidence_sha256.clone(),
            change: NodeSecurityChange::RestoreTls(different.clone()),
        }],
    };
    assert!(
        f.repo.apply_security_plan(&bad_replay).await.is_err(),
        "same map SHA cannot authenticate a different parsed policy"
    );
    let ciphertext = deve_sub_security::envelope::seal(
        f.key.as_bytes(),
        b"nodes.tls_json",
        &serde_json::to_vec(&different).expect("TLS"),
    )
    .expect("synthetic replacement");
    sqlx::query("UPDATE nodes SET tls_json_encrypted=? WHERE id=?")
        .bind(ciphertext)
        .bind(f.ids[0].to_string())
        .execute(&f.pool)
        .await
        .expect("alter stored result without metadata");
    assert!(
        f.repo.apply_security_plan(&repair).await.is_err(),
        "metadata alone is not a verified replay"
    );
}

#[tokio::test]
async fn repair_cannot_implicitly_select_a_previously_unquarantined_identity() {
    let f = Fixture::new().await;
    let before = protected(&f.pool, false).await;
    assert!(
        f.repo
            .apply_security_plan(
                &f.plan(0, NodeSecurityChange::RestoreTls(f.pins(0).await), 'a')
                    .await
            )
            .await
            .is_err()
    );
    assert_eq!(before, protected(&f.pool, false).await);
    assert!(
        f.repo
            .security_blocked_ids()
            .await
            .expect("not broadened")
            .is_empty()
    );
}
