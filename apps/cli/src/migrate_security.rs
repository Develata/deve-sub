//! Parse an explicit recovery map and hash-verified original input artifacts.
use anyhow::{Context, Result, bail};
use deve_sub_domain::source::recovery::{NodeSecurityAction, NodeSecurityChange, NodeSecurityPlan};
use deve_sub_domain::{ProtocolConfig, SourceType};
use deve_sub_kernel::NodeId;
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::SqliteNodePoolRepository;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_INPUT: u64 = 10 * 1024 * 1024;

pub(super) struct PreparedReview {
    pub(super) plan: NodeSecurityPlan,
    pub(super) map_bytes: Vec<u8>,
    pub(super) artifacts: std::collections::BTreeMap<String, Vec<u8>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewMap {
    version: u32,
    reviewed: bool,
    key_fingerprint: String,
    expected_pool_revision: u64,
    actions: Vec<ReviewAction>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewAction {
    node_id: String,
    expected_revision: u64,
    expected_fingerprint: String,
    evidence_sha256: String,
    action: String,
    artifact: Option<Artifact>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: PathBuf,
    sha256: String,
    format: SourceType,
    parsed_entry_index: usize,
}

fn read_limited(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        bail!("security recovery input exceeds 10 MiB");
    }
    Ok(bytes)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(super) async fn prepare(
    pool: &SqlitePool,
    key: &MasterKey,
    path: &Path,
) -> Result<PreparedReview> {
    let bytes = read_limited(path).context("read reviewed security recovery map")?;
    let map: ReviewMap = serde_json::from_slice(&bytes).context("invalid security recovery map")?;
    if map.version != 1 || !map.reviewed || map.actions.len() > 10_000 {
        bail!(
            "security recovery requires version 1, reviewed=true and at most 10000 exact actions"
        );
    }
    if key.fingerprint()? != map.key_fingerprint {
        bail!("security recovery map does not match the original master key");
    }
    let repo = SqliteNodePoolRepository::new_with_key(
        pool.clone(),
        Arc::new(MasterKey::from_bytes(
            key.as_bytes()
                .try_into()
                .context("invalid master-key length")?,
        )),
    );
    let mut actions = Vec::with_capacity(map.actions.len());
    let mut total_artifact_bytes = 0;
    let mut artifacts = std::collections::BTreeMap::new();
    for action in map.actions {
        let id = NodeId::parse(&action.node_id).context("invalid old NodeId in recovery map")?;
        let change = match action.action.as_str() {
            "quarantine" => {
                if action.artifact.is_some() {
                    bail!("quarantine does not consume or guess an original-input artifact");
                }
                NodeSecurityChange::Quarantine
            }
            "restore_tls" => {
                let artifact = action.artifact.context(
                    "TLS recovery requires an exact original artifact and parsed entry index",
                )?;
                if artifact.format == SourceType::Auto {
                    bail!("security recovery artifact format must be explicit");
                }
                let artifact_path = if artifact.path.is_absolute() {
                    artifact.path
                } else {
                    path.parent().unwrap_or(Path::new(".")).join(artifact.path)
                };
                let original =
                    read_limited(&artifact_path).context("read exact original security input")?;
                total_artifact_bytes += original.len();
                if total_artifact_bytes > 64 * 1024 * 1024 {
                    bail!("security recovery artifact batch exceeds 64 MiB");
                }
                if hash(&original) != artifact.sha256 || action.evidence_sha256 != artifact.sha256 {
                    bail!("original artifact hash differs from the reviewed mapping");
                }
                let entries =
                    deve_sub_application::source::parse_content(artifact.format, None, &original)
                        .map_err(|_| {
                        anyhow::anyhow!("original security input cannot be parsed faithfully")
                    })?;
                let parsed = entries
                    .into_iter()
                    .nth(artifact.parsed_entry_index)
                    .and_then(|entry| entry.node)
                    .context("reviewed original input entry is absent or unsupported")?;
                if matches!(parsed.config, ProtocolConfig::Unsupported(_)) {
                    bail!("reviewed original input is unsupported; keep the old node quarantined");
                }
                let snapshot = repo.security_snapshot(id).await?;
                // WHY: only TLS policy may change in this recovery slice.
                // Exact endpoint/authentication/protocol/config equality avoids
                // a same-name or same-host mapping overwriting credentials.
                let mut old = snapshot.node;
                let mut candidate = parsed.clone();
                old.tls = None;
                candidate.tls = None;
                if old.canonical_identity_str()? != candidate.canonical_identity_str()? {
                    bail!(
                        "original entry changes non-TLS identity; separate protocol recovery review is required"
                    );
                }
                let tls = parsed
                    .tls
                    .context("original input has no TLS policy to restore")?;
                if !tls.enabled || tls.certificate_pins.is_empty() || tls.reality.is_some() {
                    bail!("original TLS pin policy is incomplete or unsupported");
                }
                artifacts.insert(artifact.sha256, original);
                NodeSecurityChange::RestoreTls(tls)
            }
            _ => bail!("unknown security recovery action"),
        };
        actions.push(NodeSecurityAction {
            node_id: id,
            expected_revision: action.expected_revision,
            expected_fingerprint: action.expected_fingerprint,
            evidence_sha256: action.evidence_sha256,
            change,
        });
    }
    Ok(PreparedReview {
        plan: NodeSecurityPlan {
            plan_sha256: hash(&bytes),
            key_fingerprint: map.key_fingerprint,
            expected_pool_revision: map.expected_pool_revision,
            actions,
        },
        map_bytes: bytes,
        artifacts,
    })
}

pub(super) fn impact(
    plan: &NodeSecurityPlan,
    before: &[NodeId],
    after: &[NodeId],
) -> serde_json::Value {
    serde_json::json!({"phase":"verified_private_staging_before_publication","plan_sha256":plan.plan_sha256,"expected_pool_revision":plan.expected_pool_revision,
        "actions":plan.actions.iter().map(|action|serde_json::json!({"node_id":action.node_id.to_string(),"expected_revision":action.expected_revision,"expected_fingerprint":action.expected_fingerprint,"action":match &action.change {NodeSecurityChange::Quarantine=>"quarantine",NodeSecurityChange::RestoreTls(_)=>"restore_tls"},"evidence_sha256":action.evidence_sha256})).collect::<Vec<_>>(),
        "distribution_blocked_ids":after.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "newly_blocked_ids":after.iter().filter(|id| !before.contains(id)).map(ToString::to_string).collect::<Vec<_>>(),
        "released_from_security_quarantine_ids":before.iter().filter(|id| !after.contains(id)).map(ToString::to_string).collect::<Vec<_>>(),
        "release_note":"user disabled state and source withdrawal still apply to released IDs",
        "cache_impact":if plan.actions.is_empty() {"no withdrawal change"} else {"all caches before the new global withdrawal floor must regenerate; no historical output is deleted or relabeled"},
        "preserved":["NodeId","credentials","tags","overrides and disabled state","source bindings","chains","historical templates and subscriptions"]})
}
