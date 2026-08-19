//! SQLite implementation of [`NodePoolRepository`].
//!
//! The [`reconcile`] method performs the entire source refresh transaction:
//! deactivate the previous active snapshot, insert the new snapshot, insert
//! source items, dedup and upsert nodes into the pool, create source
//! bindings, and mark missing nodes — all in a single database transaction
//! (constraint #19: on failure, preserve the last successful subscription
//! version).
//!
//! Query methods ([`list_nodes`], [`get_node`]) reconstruct the full
//! [`Node`] aggregate from the denormalized `nodes` columns plus a subquery
//! for the first source label. [`import_nodes`] inserts manually-parsed
//! nodes with dedup but no source binding.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{
    ImportOutcome, ImportResult, ItemParseStatus, Node, NodeChain, NodeChainEntry, NodeFilter,
    NodePoolEntry, NodePoolRepository, ReconcileInput, ReconcileResult, SourceError,
};
use deve_sub_kernel::{NodeId, NodeSourceBindingId, SourceItemId};
use deve_sub_security::{MasterKey, PURPOSE_NODE_IDENTITY, envelope, identity_fingerprint};
use sqlx::sqlite::SqlitePool;

use crate::node_row::{NODE_COLUMNS, NodeRow};
use crate::timestamp::format_ts;

/// SQLite-backed node pool repository.
pub struct SqliteNodePoolRepository {
    pool: SqlitePool,
    master_key: Option<Arc<MasterKey>>,
}

impl SqliteNodePoolRepository {
    /// Create a new repository without at-rest encryption.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            master_key: None,
        }
    }

    /// Create a new repository with at-rest encryption for credential fields.
    /// See ADR-0007.
    #[must_use]
    pub fn new_with_key(pool: SqlitePool, master_key: Arc<MasterKey>) -> Self {
        Self {
            pool,
            master_key: Some(master_key),
        }
    }
}

/// Serialize a value to a JSON string, mapping serde errors to [`SourceError`].
fn to_json<T: serde::Serialize>(value: &T) -> Result<String, SourceError> {
    serde_json::to_string(value).map_err(|e| SourceError::Storage(e.to_string()))
}

/// Serialize an `Option<T>` to an optional JSON string (`None` → SQL NULL).
fn to_json_opt<T: serde::Serialize>(value: &Option<T>) -> Result<Option<String>, SourceError> {
    value.as_ref().map(to_json).transpose()
}

/// Encrypt a JSON string into a secret envelope, if a key is set. The
/// `context` label drives HKDF subkey derivation and is bound as AAD.
fn seal_json(
    key: Option<&MasterKey>,
    context: &[u8],
    json: &str,
) -> Result<Option<String>, SourceError> {
    match key {
        Some(k) => envelope::seal(k.as_bytes(), context, json.as_bytes())
            .map(Some)
            .map_err(|e| SourceError::Storage(format!("encryption failed: {e}"))),
        None => Ok(None),
    }
}

/// Encrypt an optional JSON string into a secret envelope.
fn seal_json_opt(
    key: Option<&MasterKey>,
    context: &[u8],
    json: &Option<String>,
) -> Result<Option<String>, SourceError> {
    match json {
        Some(s) => seal_json(key, context, s),
        None => Ok(None),
    }
}

/// HKDF/AAD context labels for node columns.
const CTX_PROTOCOL_CONFIG: &[u8] = b"nodes.protocol_config_json";
const CTX_AUTHENTICATION: &[u8] = b"nodes.authentication_json";
const CTX_TLS: &[u8] = b"nodes.tls_json";
const CTX_TRANSPORT: &[u8] = b"nodes.transport_json";
const CTX_OBFUSCATION: &[u8] = b"nodes.obfuscation_json";
const CTX_EXTRAS: &[u8] = b"nodes.extras_json";

/// Compute the node identity fingerprint (B-12).
///
/// The fingerprint is a keyed HMAC-SHA256 of the canonical node identity
/// JSON string (see [`Node::canonical_identity_str`]), using the master
/// key. When no key is set (test mode), a plain SHA256 digest is used —
/// the two forms are not interchangeable but are each internally
/// consistent within a single database instance.
fn node_fingerprint(node: &Node, key: Option<&MasterKey>) -> Result<String, SourceError> {
    let canonical = node
        .canonical_identity_str()
        .map_err(|e| SourceError::Storage(format!("canonical identity: {e}")))?;
    identity_fingerprint(PURPOSE_NODE_IDENTITY, &canonical, key.map(|k| k.as_bytes()))
        .map_err(|e| SourceError::Storage(format!("identity fingerprint: {e}")))
}
const CTX_SOURCE_ITEM_URI: &[u8] = b"source_items.raw_uri";
const CTX_BINDING_URI: &[u8] = b"node_source_bindings.raw_uri";

#[async_trait]
impl NodePoolRepository for SqliteNodePoolRepository {
    async fn reconcile(&self, input: ReconcileInput<'_>) -> Result<ReconcileResult, SourceError> {
        // WHY: the entire refresh is one transaction so a failure at any step
        // rolls back everything — the old snapshot stays active and the node
        // pool is untouched (constraint #19).
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

        // 1. Deactivate the previous active snapshot for this source.
        sqlx::query(
            "UPDATE source_snapshots SET is_active = 0 WHERE source_id = ? AND is_active = 1",
        )
        .bind(input.source_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;

        // 2. Insert the new snapshot as active.
        let fetched_at = format_ts(input.snapshot.fetched_at).map_err(SourceError::Storage)?;
        sqlx::query(
            "INSERT INTO source_snapshots \
             (id, source_id, version, fetched_at, etag, node_count, is_active) \
             VALUES (?, ?, ?, ?, ?, ?, 1)",
        )
        .bind(input.snapshot.id.to_string())
        .bind(input.source_id.to_string())
        .bind(input.snapshot.version as i64)
        .bind(fetched_at)
        .bind(&input.snapshot.etag)
        .bind(input.snapshot.node_count as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;

        // 3. Collect old bound node IDs for missing detection in step 6.
        // WHY: we capture the pre-deletion binding state so step 6 can detect
        // which nodes this source previously contributed to but no longer
        // does. Missing reactivation candidates are queried directly from the
        // nodes table in step 5 (a missing node has no binding to JOIN on).
        let old_bound_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT n.id FROM nodes n \
             JOIN node_source_bindings b ON n.id = b.node_id \
             WHERE b.source_id = ?",
        )
        .bind(input.source_id.to_string())
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;

        let old_bound: HashSet<String> = old_bound_rows.into_iter().map(|(id,)| id).collect();

        // 4. Delete all existing bindings for this source. New bindings are
        // created during entry processing below. The old_bound set is already
        // captured, so deleting first avoids duplicate-binding issues.
        sqlx::query("DELETE FROM node_source_bindings WHERE source_id = ?")
            .bind(input.source_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

        // 5. Process each entry: insert source_item, dedup/upsert node, binding.
        let mut result = ReconcileResult::default();
        let mut seen: HashSet<String> = HashSet::new();

        for entry in input.entries {
            let mut final_status = entry.initial_status;
            let mut node_id_opt: Option<String> = None;

            if let Some(node) = &entry.node {
                let proto_str = to_json(&node.protocol)?;
                let host_str = node.endpoint.host.uri_host();
                let fingerprint = node_fingerprint(node, self.master_key.as_deref())?;

                let active: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM nodes \
                     WHERE identity_fingerprint = ? \
                     AND missing_from_source = 0 \
                     LIMIT 1",
                )
                .bind(&fingerprint)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;

                if let Some((existing_id,)) = active {
                    if final_status == ItemParseStatus::Parsed {
                        final_status = ItemParseStatus::Duplicate;
                    }
                    node_id_opt = Some(existing_id);
                    result.duplicate_nodes += 1;
                } else {
                    // WHY: query the nodes table directly for a missing node
                    // with the same identity fingerprint. A missing node has
                    // no binding to this source (it was deleted in a prior
                    // refresh), so a bindings JOIN would miss it. The dedup
                    // partial unique index guarantees at most one active node
                    // per fingerprint but allows multiple missing ones; we
                    // take the first.
                    let missing: Option<(String,)> = sqlx::query_as(
                        "SELECT id FROM nodes \
                         WHERE identity_fingerprint = ? \
                         AND missing_from_source = 1 \
                         LIMIT 1",
                    )
                    .bind(&fingerprint)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?;

                    if let Some((missing_id,)) = missing {
                        sqlx::query(
                            "UPDATE nodes SET missing_from_source = 0, revision = revision + 1 \
                             WHERE id = ?",
                        )
                        .bind(&missing_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| SourceError::Storage(e.to_string()))?;
                        node_id_opt = Some(missing_id);
                        result.reactivated_nodes += 1;
                    } else {
                        let new_id = insert_node(
                            &mut tx,
                            node,
                            &proto_str,
                            &host_str,
                            &fingerprint,
                            self.master_key.as_deref(),
                        )
                        .await?;
                        node_id_opt = Some(new_id);
                        result.new_nodes += 1;
                    }
                }
            }

            // Insert the source_item with the final parse status.
            let raw_uri_encrypted = seal_json(
                self.master_key.as_deref(),
                CTX_SOURCE_ITEM_URI,
                &entry.raw_uri,
            )?;
            sqlx::query(
                "INSERT INTO source_items \
                 (id, snapshot_id, raw_uri_encrypted, parse_status) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(SourceItemId::new().to_string())
            .bind(input.snapshot.id.to_string())
            .bind(&raw_uri_encrypted)
            .bind(final_status.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

            // Create a binding if this entry produced or matched a node.
            if let Some(node_id) = &node_id_opt {
                let binding_uri_encrypted =
                    seal_json(self.master_key.as_deref(), CTX_BINDING_URI, &entry.raw_uri)?;
                sqlx::query(
                    "INSERT INTO node_source_bindings \
                     (id, node_id, source_id, raw_uri_encrypted) \
                     VALUES (?, ?, ?, ?)",
                )
                .bind(NodeSourceBindingId::new().to_string())
                .bind(node_id)
                .bind(input.source_id.to_string())
                .bind(&binding_uri_encrypted)
                .execute(&mut *tx)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;
                seen.insert(node_id.clone());
            }
        }

        // 6. Mark missing: nodes previously bound to this source that were not
        // seen in this refresh. Their binding was deleted in step 4 and not
        // recreated. If no other source binds them, they become missing.
        for old_node_id in old_bound.difference(&seen) {
            let count: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM node_source_bindings WHERE node_id = ?")
                    .bind(old_node_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?;
            if count.0 == 0 {
                sqlx::query("UPDATE nodes SET missing_from_source = 1 WHERE id = ?")
                    .bind(old_node_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?;
                result.missing_nodes += 1;
            }
        }

        // 7. Bump the global pool revision so stale generation cache entries
        // are invalidated. WHY: the cache key includes pool_revision; bumping
        // here ensures a post-refresh generation produces a new cache entry
        // rather than serving stale content (GEN-015, constraint #19).
        sqlx::query("UPDATE pool_meta SET revision = revision + 1 WHERE id = 1")
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(result)
    }

    async fn list_nodes(
        &self,
        filter: &NodeFilter,
        cursor: Option<NodeId>,
        limit: u32,
    ) -> Result<Vec<NodePoolEntry>, SourceError> {
        let limit_i: i64 = limit.into();

        let proto_json = match &filter.protocol {
            Some(p) => Some(to_json(p)?),
            None => None,
        };

        let mut sql = String::from("SELECT ");
        sql.push_str(NODE_COLUMNS);
        sql.push_str(" FROM nodes n LEFT JOIN node_overrides o ON o.node_id = n.id WHERE 1=1");

        if proto_json.is_some() {
            sql.push_str(" AND n.protocol_kind = ?");
        }
        if filter.region.is_some() {
            sql.push_str(" AND n.region = ?");
        }
        if !filter.include_missing {
            sql.push_str(" AND n.missing_from_source = 0");
        }
        if !filter.include_inactive {
            sql.push_str(" AND n.status = 'active'");
        }
        if cursor.is_some() {
            sql.push_str(" AND n.id > ?");
        }
        sql.push_str(" ORDER BY n.id ASC LIMIT ?");

        let mut q = sqlx::query_as::<_, NodeRow>(&sql);
        if let Some(p) = &proto_json {
            q = q.bind(p);
        }
        if let Some(region) = &filter.region {
            q = q.bind(region);
        }
        if let Some(c) = cursor {
            q = q.bind(c.to_string());
        }
        q = q.bind(limit_i);

        let rows: Vec<NodeRow> = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        rows.iter()
            .map(|r| r.to_pool_entry(self.master_key.as_deref()))
            .collect()
    }

    async fn get_node(&self, id: NodeId) -> Result<Option<NodePoolEntry>, SourceError> {
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes n \
             LEFT JOIN node_overrides o ON o.node_id = n.id WHERE n.id = ?"
        );
        let row: Option<NodeRow> = sqlx::query_as::<_, NodeRow>(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        row.map(|r| r.to_pool_entry(self.master_key.as_deref()))
            .transpose()
    }

    async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodePoolEntry>, SourceError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?,", ids.len())
            .collect::<String>()
            .trim_end_matches(',')
            .to_owned();
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM nodes n \
             LEFT JOIN node_overrides o ON o.node_id = n.id \
             WHERE n.id IN ({placeholders})"
        );
        let mut query = sqlx::query_as::<_, NodeRow>(&sql);
        for id in ids {
            query = query.bind(id.to_string());
        }
        let rows: Vec<NodeRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        rows.iter()
            .map(|r| r.to_pool_entry(self.master_key.as_deref()))
            .collect()
    }

    async fn import_nodes(&self, nodes: Vec<Node>) -> Result<ImportResult, SourceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

        let mut result = ImportResult::default();

        for node in nodes {
            let proto_str = to_json(&node.protocol)?;
            let host_str = node.endpoint.host.uri_host();
            let fingerprint = node_fingerprint(&node, self.master_key.as_deref())?;

            // WHY: dedup matches reconcile — one active (non-missing) node per
            // identity fingerprint (B-12). Duplicates are counted but NOT
            // overwritten; the existing node's credentials are preserved
            // (NODE-003: do not drop nodes with different credentials — but
            // now nodes with different credentials have different
            // fingerprints and are distinct entries, not duplicates).
            let existing: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM nodes \
                 WHERE identity_fingerprint = ? \
                 AND missing_from_source = 0 \
                 LIMIT 1",
            )
            .bind(&fingerprint)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

            if let Some((existing_id,)) = existing {
                let nid =
                    NodeId::parse(&existing_id).map_err(|e| SourceError::Storage(e.to_string()))?;
                result.duplicate_nodes += 1;
                result.outcomes.push(ImportOutcome::Duplicate(nid));
            } else {
                let missing: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM nodes \
                     WHERE identity_fingerprint = ? \
                     AND missing_from_source = 1 \
                     LIMIT 1",
                )
                .bind(&fingerprint)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;

                if let Some((missing_id,)) = missing {
                    // WHY: reactivate a previously-missing node with the same
                    // identity fingerprint rather than creating a duplicate
                    // row. The dedup partial unique index would otherwise
                    // reject the insert. We keep the existing node's
                    // credentials/config (NODE-003) and only flip
                    // missing_from_source back to 0.
                    sqlx::query(
                        "UPDATE nodes SET missing_from_source = 0, revision = revision + 1 \
                         WHERE id = ?",
                    )
                    .bind(&missing_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?;
                    let nid = NodeId::parse(&missing_id)
                        .map_err(|e| SourceError::Storage(e.to_string()))?;
                    result.new_nodes += 1;
                    result.outcomes.push(ImportOutcome::Inserted(nid));
                } else {
                    let new_id = insert_node(
                        &mut tx,
                        &node,
                        &proto_str,
                        &host_str,
                        &fingerprint,
                        self.master_key.as_deref(),
                    )
                    .await?;
                    let nid =
                        NodeId::parse(&new_id).map_err(|e| SourceError::Storage(e.to_string()))?;
                    result.new_nodes += 1;
                    result.outcomes.push(ImportOutcome::Inserted(nid));
                }
            }
        }

        // Bump the global pool revision so stale generation cache entries are
        // invalidated. WHY: same as reconcile — the cache key includes
        // pool_revision (GEN-015).
        sqlx::query("UPDATE pool_meta SET revision = revision + 1 WHERE id = 1")
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(result)
    }

    async fn list_node_chains(&self) -> Result<Vec<NodeChainEntry>, SourceError> {
        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id, chain_json FROM nodes WHERE chain_json IS NOT NULL")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;

        let mut out = Vec::with_capacity(rows.len());
        for (id_str, chain_json) in rows {
            let node_id =
                NodeId::parse(&id_str).map_err(|e| SourceError::Storage(e.to_string()))?;
            let chain: NodeChain = serde_json::from_str(chain_json.as_deref().unwrap_or("[]"))
                .map_err(|e| SourceError::Storage(e.to_string()))?;
            if !chain.nodes.is_empty() {
                out.push(NodeChainEntry {
                    node_id,
                    chain: chain.nodes,
                });
            }
        }
        Ok(out)
    }

    async fn existing_node_ids(&self, ids: &[NodeId]) -> Result<Vec<NodeId>, SourceError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?,", ids.len())
            .collect::<String>()
            .trim_end_matches(',')
            .to_owned();
        let sql = format!("SELECT id FROM nodes WHERE id IN ({placeholders})");
        let mut query = sqlx::query_as::<_, (String,)>(&sql);
        for id in ids {
            query = query.bind(id.to_string());
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        let mut result: Vec<NodeId> = Vec::with_capacity(rows.len());
        for (id_str,) in rows {
            result.push(NodeId::parse(&id_str).map_err(|e| SourceError::Storage(e.to_string()))?);
        }
        Ok(result)
    }

    async fn set_node_chain(
        &self,
        node_id: NodeId,
        chain: Option<&[NodeId]>,
    ) -> Result<(), SourceError> {
        let chain_json = match chain {
            Some(nodes) => {
                let chain = NodeChain {
                    nodes: nodes.to_vec(),
                };
                Some(to_json(&chain)?)
            }
            None => None,
        };
        let result = sqlx::query("UPDATE nodes SET chain_json = ? WHERE id = ?")
            .bind(&chain_json)
            .bind(node_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SourceError::NodeNotFound(node_id.to_string()));
        }
        Ok(())
    }
}

/// Insert a new node into the `nodes` table.
///
/// `created_at` uses the column DEFAULT (current UTC time). `revision` is 0,
/// `status` is `'active'`, `missing_from_source` is 0 for new nodes.
async fn insert_node(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    node: &Node,
    proto_str: &str,
    host_str: &str,
    fingerprint: &str,
    key: Option<&MasterKey>,
) -> Result<String, SourceError> {
    let node_id = node.id.to_string();
    let imported_at = format_ts(node.source.imported_at).map_err(SourceError::Storage)?;
    let config_json = to_json(&node.config)?;
    let auth_json = to_json(&node.authentication)?;
    let tls_json = to_json_opt(&node.tls)?;
    let transport_json = to_json_opt(&node.transport)?;
    let udp_json = to_json(&node.udp)?;
    let multiplex_json = to_json_opt(&node.multiplex)?;
    let obfuscation_json = to_json_opt(&node.obfuscation)?;
    let congestion_json = to_json_opt(&node.congestion)?;
    let extras_json = to_json(&node.extras)?;

    let config_json_encrypted = seal_json(key, CTX_PROTOCOL_CONFIG, &config_json)?;
    let auth_json_encrypted = seal_json(key, CTX_AUTHENTICATION, &auth_json)?;
    let tls_json_encrypted = seal_json_opt(key, CTX_TLS, &tls_json)?;
    let transport_json_encrypted = seal_json_opt(key, CTX_TRANSPORT, &transport_json)?;
    let obfuscation_json_encrypted = seal_json_opt(key, CTX_OBFUSCATION, &obfuscation_json)?;
    let extras_json_encrypted = seal_json(key, CTX_EXTRAS, &extras_json)?;

    sqlx::query(
        "INSERT INTO nodes \
         (id, display_name, protocol_kind, host, port, \
         protocol_config_json_encrypted, authentication_json_encrypted, \
         tls_json_encrypted, transport_json_encrypted, \
         udp_capability, multiplex_json, obfuscation_json_encrypted, \
         congestion_json, region, extras_json_encrypted, \
         imported_at, revision, status, missing_from_source, source_label, \
         identity_fingerprint) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'active', 0, ?, ?)",
    )
    .bind(&node_id)
    .bind(&node.display_name)
    .bind(proto_str)
    .bind(host_str)
    .bind(i64::from(node.endpoint.port))
    .bind(&config_json_encrypted)
    .bind(&auth_json_encrypted)
    .bind(&tls_json_encrypted)
    .bind(&transport_json_encrypted)
    .bind(&udp_json)
    .bind(&multiplex_json)
    .bind(&obfuscation_json_encrypted)
    .bind(&congestion_json)
    .bind(&node.region.value)
    .bind(&extras_json_encrypted)
    .bind(imported_at)
    .bind(&node.source.source_label)
    .bind(fingerprint)
    .execute(&mut **tx)
    .await
    .map_err(|e| SourceError::Storage(e.to_string()))?;

    Ok(node_id)
}
