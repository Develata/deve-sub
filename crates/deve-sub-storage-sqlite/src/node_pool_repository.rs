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

mod reconcile;
mod write;
use write::insert_node;

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

/// Retention bound: newest snapshots kept per source; older ones are pruned
/// at the end of each reconcile transaction (cascade removes their
/// source_items).
const SOURCE_SNAPSHOT_RETAIN: i64 = 10;

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
        self.reconcile_source(input).await
    }

    async fn list_nodes(
        &self,
        filter: &NodeFilter,
        cursor: Option<NodeId>,
        limit: u32,
    ) -> Result<Vec<NodePoolEntry>, SourceError> {
        // WHY: self-cap at 10_000 (matching the API layer's documented max in
        // `apps/server/src/nodes.rs`) so a non-API caller cannot load+decrypt
        // the entire pool in one call. Sibling list methods cap at 100 because
        // their API layers also cap at 100; nodes has a higher API ceiling, so
        // the storage cap matches it (SRC-020).
        let limit_i: i64 = i64::from(limit.min(10_000));

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
        // WHY: SQLite default SQLITE_MAX_VARIABLE_NUMBER is 999. Binding
        // more than 998 IDs in a single IN-clause fails at runtime. Chunk
        // into batches of 500 (safe margin) and merge results.
        let mut entries = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let placeholders = std::iter::repeat_n("?,", chunk.len())
                .collect::<String>()
                .trim_end_matches(',')
                .to_owned();
            let sql = format!(
                "SELECT {NODE_COLUMNS} FROM nodes n \
                 LEFT JOIN node_overrides o ON o.node_id = n.id \
                 WHERE n.id IN ({placeholders})"
            );
            let mut query = sqlx::query_as::<_, NodeRow>(&sql);
            for id in chunk {
                query = query.bind(id.to_string());
            }
            let rows: Vec<NodeRow> = query
                .fetch_all(&self.pool)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;
            for r in &rows {
                entries.push(r.to_pool_entry(self.master_key.as_deref())?);
            }
        }
        Ok(entries)
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
                    match new_id {
                        Some(id) => {
                            let nid = NodeId::parse(&id)
                                .map_err(|e| SourceError::Storage(e.to_string()))?;
                            result.new_nodes += 1;
                            result.outcomes.push(ImportOutcome::Inserted(nid));
                        }
                        // WHY: `ON CONFLICT DO NOTHING` fired — a concurrent
                        // import or refresh inserted the same node. Re-query
                        // and count as a duplicate (SRC-016).
                        None => {
                            let winner: Option<(String,)> = sqlx::query_as(
                                "SELECT id FROM nodes \
                                 WHERE identity_fingerprint = ? \
                                 AND missing_from_source = 0 \
                                 LIMIT 1",
                            )
                            .bind(&fingerprint)
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(|e| SourceError::Storage(e.to_string()))?;
                            if let Some((winner_id,)) = winner {
                                let nid = NodeId::parse(&winner_id)
                                    .map_err(|e| SourceError::Storage(e.to_string()))?;
                                result.duplicate_nodes += 1;
                                result.outcomes.push(ImportOutcome::Duplicate(nid));
                            }
                        }
                    }
                }
            }
        }

        // Bump the global pool revision so stale generation cache entries are
        // invalidated. WHY: same as reconcile — the cache key includes
        // pool_revision (GEN-015).
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;

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
            if !chain.nodes().is_empty() {
                out.push(NodeChainEntry {
                    node_id,
                    chain: chain.nodes().to_vec(),
                });
            }
        }
        Ok(out)
    }

    async fn existing_node_ids(&self, ids: &[NodeId]) -> Result<Vec<NodeId>, SourceError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut result: Vec<NodeId> = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let placeholders = std::iter::repeat_n("?,", chunk.len())
                .collect::<String>()
                .trim_end_matches(',')
                .to_owned();
            let sql = format!("SELECT id FROM nodes WHERE id IN ({placeholders})");
            let mut query = sqlx::query_as::<_, (String,)>(&sql);
            for id in chunk {
                query = query.bind(id.to_string());
            }
            let rows = query
                .fetch_all(&self.pool)
                .await
                .map_err(|e| SourceError::Storage(e.to_string()))?;
            for (id_str,) in rows {
                result
                    .push(NodeId::parse(&id_str).map_err(|e| SourceError::Storage(e.to_string()))?);
            }
        }
        Ok(result)
    }

    async fn set_node_chain(
        &self,
        node_id: NodeId,
        chain: Option<&[NodeId]>,
    ) -> Result<(), SourceError> {
        let chain_json = match chain {
            // WHY: NodeChain is #[serde(transparent)], so serializing the
            // raw node IDs produces the same JSON as to_json(&NodeChain).
            // Avoids constructing the domain entity (whose `nodes` field is
            // pub(crate)) in the storage adapter.
            Some(nodes) => Some(to_json(&nodes.to_vec())?),
            None => None,
        };
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        let result = sqlx::query("UPDATE nodes SET chain_json = ? WHERE id = ?")
            .bind(&chain_json)
            .bind(node_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(SourceError::NodeNotFound(node_id.to_string()));
        }
        // Chain changes alter emitted output (NODE-017) — invalidate cache.
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(())
    }
}
