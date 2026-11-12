#![allow(clippy::expect_used)]
//! NODE-010/GEN-015: actual CLI preview and exact original-artifact repair.
use deve_sub_domain::{NodePoolRepository, SourceType};
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::SqliteNodePoolRepository;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    path::{Path, PathBuf},
    process::Output,
    sync::Arc,
};
use tokio::process::Command;
const BIN: &str = env!("CARGO_BIN_EXE_deve-sub");
struct Fixture {
    dir: tempfile::TempDir,
    db: PathBuf,
    key_path: PathBuf,
    key: Arc<MasterKey>,
    ids: Vec<deve_sub_kernel::NodeId>,
    original: String,
    format: SourceType,
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("live.db");
        let key_path = dir.path().join("original.key");
        std::fs::write(&key_path, [0x42; 32]).expect("key");
        let key = Arc::new(MasterKey::load(&key_path).expect("key"));
        let pool = SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db.display()))
            .await
            .expect("pool");
        let mut migrator = sqlx::migrate!("../../migrations");
        migrator.migrations = migrator
            .migrations
            .iter()
            .filter(|m| m.version <= 29)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        migrator.run(&pool).await.expect("legacy");
        deve_sub_storage_sqlite::ensure_key_binding(
            &pool,
            &key.fingerprint().expect("fingerprint"),
        )
        .await
        .expect("binding");
        let original = format!(
            "proxies:\n  - name: same-name\n    type: trojan\n    server: reserved.example\n    port: 443\n    password: TEST_A\n    sni: reserved.example\n    skip-cert-verify: false\n    fingerprint: '{}'\n",
            "11".repeat(32)
        );
        let mut a = deve_sub_application::source::parse_content(
            SourceType::MihomoYaml,
            None,
            original.as_bytes(),
        )
        .expect("original")
        .remove(0)
        .node
        .expect("supported");
        a.tls.as_mut().expect("TLS").certificate_pins.clear();
        let mut b = a.clone();
        b.id = deve_sub_kernel::NodeId::new();
        b.authentication = deve_sub_domain::Authentication::Password {
            password: "TEST_B".into(),
        };
        let mut c = b.clone();
        c.id = deve_sub_kernel::NodeId::new();
        c.endpoint.port = 8443;
        let ids = vec![a.id, b.id, c.id];
        let repo = SqliteNodePoolRepository::new_with_key(pool.clone(), key.clone());
        repo.import_nodes(vec![a, b, c])
            .await
            .expect("legacy nodes");
        repo.set_node_chain(ids[2], Some(&[ids[0]]))
            .await
            .expect("chain");
        sqlx::query("INSERT INTO node_overrides(id,node_id,enabled,sni,fingerprint) VALUES ('01KZAAAAAAAAAAAAAAAAAAAA00',?,0,'override.example','chrome')").bind(ids[0].to_string()).execute(&pool).await.expect("disabled override");
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&pool)
            .await
            .expect("checkpoint");
        pool.close().await;
        Self {
            dir,
            db,
            key_path,
            key,
            ids,
            original,
            format: SourceType::MihomoYaml,
        }
    }
    async fn pool(&self) -> SqlitePool {
        SqlitePool::connect(&format!("sqlite://{}", self.db.display()))
            .await
            .expect("pool")
    }
    async fn map(&self, action: &str) -> PathBuf {
        let pool = self.pool().await;
        let repo = SqliteNodePoolRepository::new_with_key(pool.clone(), self.key.clone());
        let snapshot = repo.security_snapshot(self.ids[0]).await.expect("base");
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM pool_meta WHERE id=1")
            .fetch_one(&pool)
            .await
            .expect("revision");
        pool.close().await;
        let mut item = json!({"node_id":self.ids[0].to_string(),"expected_revision":snapshot.revision,"expected_fingerprint":snapshot.fingerprint,"evidence_sha256":"a".repeat(64),"action":action});
        if action == "restore_tls" {
            let sha = hash(self.original.as_bytes());
            std::fs::write(self.dir.path().join("original.yaml"), &self.original)
                .expect("original file");
            item["evidence_sha256"] = json!(sha);
            item["artifact"] = json!({"path":"original.yaml","sha256":sha,"format":self.format,"parsed_entry_index":0});
        }
        let document = json!({"version":1,"reviewed":true,"key_fingerprint":self.key.fingerprint().expect("fingerprint"),"expected_pool_revision":revision,"actions":[item]});
        let path = self.dir.path().join(format!("{action}.json"));
        write_json(&path, &document);
        path
    }
    async fn run(&self, map: Option<&Path>, preview: bool) -> Output {
        let mut command = Command::new(BIN);
        command
            .current_dir(self.dir.path())
            .env_remove("DEVE_SUB_KEY_PATH")
            .env_remove("DEVE_SUB_DB_PATH")
            .env_remove("DEVE_SUB_CONFIG")
            .args(["migrate", "--db-path"])
            .arg(&self.db)
            .arg("--key-path")
            .arg(&self.key_path);
        if let Some(path) = map {
            command.arg("--security-map").arg(path);
        }
        if preview {
            command.arg("--preview");
        }
        command.output().await.expect("CLI")
    }
    async fn assert_state(&self, quarantined: bool) {
        let pool = self.pool().await;
        let repo = SqliteNodePoolRepository::new_with_key(pool.clone(), self.key.clone());
        let a = repo
            .get_node(self.ids[0])
            .await
            .expect("get")
            .expect("old ID retained");
        assert_eq!(a.security_quarantined, quarantined);
        assert!(!a.is_active, "manual disabled state retained");
        assert_eq!(
            a.override_info.expect("override").sni.as_deref(),
            Some("override.example")
        );
        let b = repo
            .get_node(self.ids[1])
            .await
            .expect("get")
            .expect("unrelated retained");
        assert!(b.is_active);
        assert!(!b.security_quarantined);
        let c = repo
            .get_node(self.ids[2])
            .await
            .expect("get")
            .expect("chain retained");
        assert_eq!(c.security_quarantined, quarantined);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 3);
        pool.close().await;
    }
}
fn write_json(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).expect("json")).expect("write");
}
fn success(out: &Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn historical_pool_requires_map_preview_is_readonly_and_repair_preserves_old_identity() {
    let f = Fixture::new().await;
    let before = std::fs::read(&f.db).expect("before");
    assert!(!f.run(None, false).await.status.success());
    assert_eq!(before, std::fs::read(&f.db).expect("unchanged"));
    let quarantine = f.map("quarantine").await;
    let preview = f.run(Some(&quarantine), true).await;
    success(&preview);
    assert_eq!(before, std::fs::read(&f.db).expect("preview unchanged"));
    let text = String::from_utf8(preview.stdout).expect("stdout");
    let impact: Value = serde_json::from_str(text.lines().next().expect("impact")).expect("json");
    let blocked = impact["distribution_blocked_ids"]
        .as_array()
        .expect("exact impact");
    assert!(blocked.contains(&json!(f.ids[0].to_string())));
    assert!(blocked.contains(&json!(f.ids[2].to_string())));
    assert!(!blocked.contains(&json!(f.ids[1].to_string())));
    assert!(
        impact["cache_impact"]
            .as_str()
            .expect("impact")
            .contains("all caches")
    );
    success(&f.run(Some(&quarantine), false).await);
    f.assert_state(true).await;
    let applied = std::fs::read(&f.db).expect("after");
    success(&f.run(Some(&quarantine), false).await);
    f.assert_state(true).await;
    let repair = f.map("restore_tls").await;
    success(&f.run(Some(&repair), true).await);
    f.assert_state(true).await;
    success(&f.run(Some(&repair), false).await);
    f.assert_state(false).await;
    let saved = std::fs::read_dir(f.dir.path())
        .expect("private rollback directories")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".deve-sub-pre-migrate-"))
        })
        .collect::<Vec<_>>();
    let sha = hash(f.original.as_bytes());
    let evidence = saved
        .iter()
        .find(|path| path.join(format!("input-{sha}.bin")).exists())
        .expect("exact parsed input durably retained");
    assert_eq!(
        std::fs::read(evidence.join(format!("input-{sha}.bin"))).expect("retained artifact"),
        f.original.as_bytes()
    );
    assert_eq!(
        std::fs::read(evidence.join("security-map.json")).expect("retained map"),
        std::fs::read(&repair).expect("map")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(evidence.join("security-map.json"))
                .expect("mode")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    success(&f.run(Some(&repair), false).await);
    let pool = f.pool().await;
    let repo = SqliteNodePoolRepository::new_with_key(pool.clone(), f.key.clone());
    let base = repo
        .security_snapshot(f.ids[0])
        .await
        .expect("repaired base");
    assert_eq!(
        base.node.tls.expect("TLS").certificate_pins[0].as_str(),
        format!("mihomoFingerprintSHA256:{}", "11".repeat(32))
    );
    pool.close().await;
    assert_ne!(applied, std::fs::read(&f.db).expect("repair"));
}

#[tokio::test]
async fn absent_wrong_hash_wrong_entry_non_tls_change_and_stale_cas_keep_quarantine() {
    let f = Fixture::new().await;
    let quarantine = f.map("quarantine").await;
    success(&f.run(Some(&quarantine), false).await);
    let path = f.map("restore_tls").await;
    let original: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("map")).expect("json");
    for kind in ["absent", "hash", "entry", "identity", "revision", "key"] {
        let mut bad = original.clone();
        match kind {
            "absent" => bad["actions"][0]["artifact"]["path"] = json!("absent.yaml"),
            "hash" => bad["actions"][0]["artifact"]["sha256"] = json!("0".repeat(64)),
            "entry" => bad["actions"][0]["artifact"]["parsed_entry_index"] = json!(1),
            "identity" => {
                let text = f.original.replace("TEST_A", "TEST_DIFFERENT");
                let sha = hash(text.as_bytes());
                std::fs::write(f.dir.path().join("different.yaml"), text).expect("artifact");
                bad["actions"][0]["artifact"]["path"] = json!("different.yaml");
                bad["actions"][0]["artifact"]["sha256"] = json!(sha);
                bad["actions"][0]["evidence_sha256"] = json!(sha);
            }
            "revision" => bad["actions"][0]["expected_revision"] = json!(999),
            "key" => bad["key_fingerprint"] = json!("0".repeat(64)),
            _ => unreachable!(),
        }
        write_json(&path, &bad);
        let before = std::fs::read(&f.db).expect("before");
        assert!(!f.run(Some(&path), false).await.status.success(), "{kind}");
        assert_eq!(before, std::fs::read(&f.db).expect("unchanged"), "{kind}");
        f.assert_state(true).await;
    }
    write_json(&path, &original);
    success(&f.run(Some(&path), false).await);
    f.assert_state(false).await;
}

#[tokio::test]
async fn original_spki_policy_is_not_rewritten_into_certificate_fingerprint_policy() {
    let mut f = Fixture::new().await;
    let pin = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    f.original=json!({"outbounds":[{"type":"trojan","tag":"same-name","server":"reserved.example","server_port":443,"password":"TEST_A","tls":{"enabled":true,"server_name":"reserved.example","insecure":false,"certificate_public_key_sha256":[pin]}}]}).to_string();
    f.format = SourceType::SingboxJson;
    let quarantine = f.map("quarantine").await;
    success(&f.run(Some(&quarantine), false).await);
    let repair = f.map("restore_tls").await;
    success(&f.run(Some(&repair), false).await);
    f.assert_state(false).await;
    let pool = f.pool().await;
    let base = SqliteNodePoolRepository::new_with_key(pool.clone(), f.key.clone())
        .security_snapshot(f.ids[0])
        .await
        .expect("original ID");
    assert_eq!(
        base.node.tls.expect("TLS").certificate_pins[0].as_str(),
        format!("spkiSHA256:{pin}")
    );
    pool.close().await;
}
