//! Original-ID security transitions, independent of user enabled/override state.
use super::*;
use crate::node_row::NODE_COLUMNS;
use deve_sub_domain::ProtocolConfig;
use deve_sub_domain::source::recovery::{
    NodeSecurityChange, NodeSecurityPlan, NodeSecuritySnapshot,
};

fn invalid(message: &'static str) -> SourceError {
    SourceError::Storage(message.into())
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn storage(error: sqlx::Error) -> SourceError {
    SourceError::Storage(error.to_string())
}

impl SqliteNodePoolRepository {
    /// Load the persisted base, never an override-adjusted identity. This also
    /// works before 0030; no recovery table is needed to inspect an old row.
    pub async fn security_snapshot(&self, id: NodeId) -> Result<NodeSecuritySnapshot, SourceError> {
        let key = self
            .master_key
            .as_deref()
            .ok_or_else(|| invalid("original master key required"))?;
        let row: NodeRow=sqlx::query_as(&format!("SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN node_overrides o ON o.node_id=n.id WHERE n.id=?"))
            .bind(id.to_string()).fetch_optional(&self.pool).await.map_err(storage)?.ok_or_else(||SourceError::NodeNotFound(id.to_string()))?;
        let (revision, fingerprint): (i64, String) =
            sqlx::query_as("SELECT revision,identity_fingerprint FROM nodes WHERE id=?")
                .bind(id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;
        Ok(NodeSecuritySnapshot {
            node: row.base_node(key)?,
            revision: u64::try_from(revision).map_err(|_| invalid("invalid old node revision"))?,
            fingerprint,
        })
    }

    /// Exact current quarantine closure, including every chain owner that
    /// reaches a quarantined old ID. Does not change stored chain references.
    pub async fn security_blocked_ids(&self) -> Result<Vec<NodeId>, SourceError> {
        let rows: Vec<String> = sqlx::query_scalar("WITH RECURSIVE blocked(id) AS (SELECT node_id FROM node_security_recovery WHERE state='quarantined' UNION SELECT n.id FROM nodes n JOIN json_each(COALESCE(n.chain_json,'[]')) item JOIN blocked b ON b.id=item.value WHERE item.type='text') SELECT id FROM blocked ORDER BY id")
            .fetch_all(&self.pool).await.map_err(storage)?;
        rows.into_iter()
            .map(|id| NodeId::parse(&id).map_err(|_| invalid("invalid stored chain identity")))
            .collect()
    }

    /// Apply an operator-reviewed exact mapping atomically. Returns false only
    /// for a verified replay of the same batch against its unchanged result.
    pub async fn apply_security_plan(&self, plan: &NodeSecurityPlan) -> Result<bool, SourceError> {
        let key = self
            .master_key
            .as_deref()
            .ok_or_else(|| invalid("original master key required"))?;
        if !digest(&plan.plan_sha256)
            || !digest(&plan.key_fingerprint)
            || key
                .fingerprint()
                .map_err(|_| invalid("key fingerprint failed"))?
                != plan.key_fingerprint
        {
            return Err(invalid(
                "invalid security review or original key fingerprint",
            ));
        }
        let mut unique = HashSet::new();
        for action in &plan.actions {
            if !unique.insert(action.node_id)
                || !digest(&action.evidence_sha256)
                || (!action.expected_fingerprint.is_empty()
                    && !(action.expected_fingerprint.len() == 43
                        && action
                            .expected_fingerprint
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')))
            {
                return Err(invalid(
                    "duplicate node or invalid security recovery evidence",
                ));
            }
        }
        let expected = i64::try_from(plan.expected_pool_revision)
            .map_err(|_| invalid("invalid pool revision"))?;
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(storage)?;
        let bound: Option<String> =
            sqlx::query_scalar("SELECT current_key_fingerprint FROM key_metadata WHERE id=1")
                .fetch_optional(&mut *tx)
                .await
                .map_err(storage)?;
        if bound.as_deref() != Some(plan.key_fingerprint.as_str()) {
            return Err(invalid(
                "database original key binding is missing or mismatched",
            ));
        }
        let current: i64 = sqlx::query_scalar("SELECT revision FROM pool_meta WHERE id=1")
            .fetch_one(&mut *tx)
            .await
            .map_err(storage)?;
        let previous:Option<(i64,i64,i64)>=sqlx::query_as("SELECT expected_pool_revision,resulting_pool_revision,action_count FROM node_security_review_batches WHERE plan_sha256=?")
            .bind(&plan.plan_sha256).fetch_optional(&mut *tx).await.map_err(storage)?;
        if let Some((was, result, count)) = previous {
            if was != expected
                || result != current
                || usize::try_from(count).ok() != Some(plan.actions.len())
            {
                return Err(invalid(
                    "security review replay no longer matches its result",
                ));
            }
            for action in &plan.actions {
                let state = match &action.change {
                    NodeSecurityChange::Quarantine => "quarantined",
                    NodeSecurityChange::RestoreTls(_) => "repaired",
                };
                let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM node_security_recovery r JOIN nodes n ON n.id=r.node_id WHERE r.node_id=? AND r.plan_sha256=? AND r.state=? AND r.evidence_sha256=? AND r.expected_revision=? AND r.expected_fingerprint=? AND n.revision=r.resulting_revision AND n.identity_fingerprint=r.resulting_fingerprint)")
                    .bind(action.node_id.to_string()).bind(&plan.plan_sha256).bind(state).bind(&action.evidence_sha256)
                    .bind(i64::try_from(action.expected_revision).map_err(|_|invalid("invalid node revision"))?).bind(&action.expected_fingerprint)
                    .fetch_one(&mut *tx).await.map_err(storage)?;
                if !matches {
                    return Err(invalid(
                        "security review replay identity or evidence changed",
                    ));
                }
                if let NodeSecurityChange::RestoreTls(tls) = &action.change {
                    let row: NodeRow = sqlx::query_as(&format!("SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN node_overrides o ON o.node_id=n.id WHERE n.id=?"))
                        .bind(action.node_id.to_string()).fetch_one(&mut *tx).await.map_err(storage)?;
                    let base = row.base_node(key)?;
                    let recorded: String = sqlx::query_scalar(
                        "SELECT resulting_fingerprint FROM node_security_recovery WHERE node_id=?",
                    )
                    .bind(action.node_id.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(storage)?;
                    if base.tls.as_ref() != Some(tls)
                        || node_fingerprint(&base, Some(key))? != recorded
                    {
                        return Err(invalid(
                            "security review replay no longer authenticates its repaired TLS policy",
                        ));
                    }
                }
            }
            tx.rollback().await.map_err(storage)?;
            return Ok(false);
        }
        if current != expected {
            return Err(invalid(
                "pool changed since security review; preview a fresh exact mapping",
            ));
        }
        let next = if plan.actions.is_empty() {
            current
        } else {
            current
                .checked_add(1)
                .ok_or_else(|| invalid("pool revision overflow"))?
        };
        sqlx::query("INSERT INTO node_security_review_batches(plan_sha256,expected_pool_revision,resulting_pool_revision,action_count) VALUES (?,?,?,?)")
            .bind(&plan.plan_sha256).bind(expected).bind(next).bind(plan.actions.len() as i64).execute(&mut *tx).await.map_err(storage)?;
        for action in &plan.actions {
            let old_revision = i64::try_from(action.expected_revision)
                .map_err(|_| invalid("invalid node revision"))?;
            let (revision, fingerprint): (i64, String) =
                sqlx::query_as("SELECT revision,identity_fingerprint FROM nodes WHERE id=?")
                    .bind(action.node_id.to_string())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage)?
                    .ok_or_else(|| SourceError::NodeNotFound(action.node_id.to_string()))?;
            if revision != old_revision || fingerprint != action.expected_fingerprint {
                return Err(invalid(
                    "node changed since security review; no partial actions applied",
                ));
            }
            let resulting_revision = revision
                .checked_add(1)
                .ok_or_else(|| invalid("node revision overflow"))?;
            let (state, resulting_fingerprint) = match &action.change {
                NodeSecurityChange::Quarantine => {
                    let changed = sqlx::query("UPDATE nodes SET revision=? WHERE id=? AND revision=? AND identity_fingerprint=?")
                        .bind(resulting_revision).bind(action.node_id.to_string()).bind(revision).bind(&fingerprint).execute(&mut *tx).await.map_err(storage)?;
                    if changed.rows_affected() != 1 {
                        return Err(invalid(
                            "quarantine compare-and-swap did not update exactly one node",
                        ));
                    }
                    ("quarantined", fingerprint.clone())
                }
                NodeSecurityChange::RestoreTls(tls) => {
                    let quarantined: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM node_security_recovery WHERE node_id=? AND state='quarantined')").bind(action.node_id.to_string()).fetch_one(&mut *tx).await.map_err(storage)?;
                    if !quarantined {
                        return Err(invalid(
                            "TLS repair requires this exact old ID to be quarantined first",
                        ));
                    }
                    if !tls.enabled || tls.certificate_pins.is_empty() || tls.reality.is_some() {
                        return Err(invalid(
                            "restored TLS pin policy is incomplete or unsupported",
                        ));
                    }
                    let row:NodeRow=sqlx::query_as(&format!("SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN node_overrides o ON o.node_id=n.id WHERE n.id=?"))
                        .bind(action.node_id.to_string()).fetch_one(&mut *tx).await.map_err(storage)?;
                    let mut base = row.base_node(key)?;
                    if matches!(base.config, ProtocolConfig::Unsupported(_)) {
                        return Err(invalid(
                            "unsupported historical base requires a separately reviewed protocol recovery",
                        ));
                    }
                    if !fingerprint.is_empty() && node_fingerprint(&base, Some(key))? != fingerprint
                    {
                        return Err(invalid(
                            "old base fingerprint cannot be reproduced without guessing",
                        ));
                    }
                    base.tls = Some(tls.clone());
                    let repaired = node_fingerprint(&base, Some(key))?;
                    let collision: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM nodes WHERE identity_fingerprint=? AND id<>?)",
                    )
                    .bind(&repaired)
                    .bind(action.node_id.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(storage)?;
                    if collision {
                        return Err(invalid(
                            "restored identity belongs to another node; refusing merge or replacement",
                        ));
                    }
                    let encrypted = seal_json(Some(key), CTX_TLS, &to_json(tls)?)?;
                    let changed = sqlx::query("UPDATE nodes SET tls_json_encrypted=?,identity_fingerprint=?,revision=? WHERE id=? AND revision=? AND identity_fingerprint=?")
                        .bind(encrypted).bind(&repaired).bind(resulting_revision).bind(action.node_id.to_string()).bind(revision).bind(&fingerprint).execute(&mut *tx).await.map_err(storage)?;
                    if changed.rows_affected() != 1 {
                        return Err(invalid(
                            "TLS recovery compare-and-swap did not update exactly one node",
                        ));
                    }
                    ("repaired", repaired)
                }
            };
            let actual: (i64, String) =
                sqlx::query_as("SELECT revision,identity_fingerprint FROM nodes WHERE id=?")
                    .bind(action.node_id.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(storage)?;
            if actual != (resulting_revision, resulting_fingerprint.clone()) {
                return Err(invalid(
                    "node recovery result differs from the reviewed action",
                ));
            }
            if let NodeSecurityChange::RestoreTls(tls) = &action.change {
                let row: NodeRow = sqlx::query_as(&format!("SELECT {NODE_COLUMNS} FROM nodes n LEFT JOIN node_overrides o ON o.node_id=n.id WHERE n.id=?"))
                    .bind(action.node_id.to_string()).fetch_one(&mut *tx).await.map_err(storage)?;
                let base = row.base_node(key)?;
                if base.tls.as_ref() != Some(tls)
                    || node_fingerprint(&base, Some(key))? != resulting_fingerprint
                {
                    return Err(invalid(
                        "stored TLS recovery cannot be authenticated against its resulting identity",
                    ));
                }
            }
            sqlx::query("INSERT INTO node_security_recovery(node_id,state,plan_sha256,evidence_sha256,expected_revision,expected_fingerprint,resulting_revision,resulting_fingerprint) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(node_id) DO UPDATE SET state=excluded.state,plan_sha256=excluded.plan_sha256,evidence_sha256=excluded.evidence_sha256,expected_revision=excluded.expected_revision,expected_fingerprint=excluded.expected_fingerprint,resulting_revision=excluded.resulting_revision,resulting_fingerprint=excluded.resulting_fingerprint")
                .bind(action.node_id.to_string()).bind(state).bind(&plan.plan_sha256).bind(&action.evidence_sha256).bind(revision).bind(&fingerprint).bind(resulting_revision).bind(&resulting_fingerprint).execute(&mut *tx).await.map_err(storage)?;
            sqlx::query("INSERT INTO node_security_recovery_events(plan_sha256,node_id,state,evidence_sha256,expected_revision,expected_fingerprint,resulting_revision,resulting_fingerprint) VALUES (?,?,?,?,?,?,?,?)")
                .bind(&plan.plan_sha256).bind(action.node_id.to_string()).bind(state).bind(&action.evidence_sha256).bind(revision).bind(&fingerprint).bind(resulting_revision).bind(&resulting_fingerprint).execute(&mut *tx).await.map_err(storage)?;
        }
        if !plan.actions.is_empty() {
            // No reliable historical cache-to-node lineage exists. Fence all
            // older results without deleting history or changing user settings.
            sqlx::query("UPDATE pool_meta SET revision=?,withdrawal_revision=? WHERE id=1")
                .bind(next)
                .bind(next)
                .execute(&mut *tx)
                .await
                .map_err(storage)?;
        }
        tx.commit().await.map_err(storage)?;
        Ok(true)
    }
}
