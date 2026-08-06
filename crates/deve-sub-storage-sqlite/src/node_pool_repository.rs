//! SQLite implementation of [`NodePoolRepository`].
//!
//! The [`reconcile`] method performs the entire source refresh transaction:
//! deactivate the previous active snapshot, insert the new snapshot, insert
//! source items, dedup and upsert nodes into the pool, create source
//! bindings, and mark missing nodes — all in a single database transaction
//! (constraint #19: on failure, preserve the last successful subscription
//! version).

use std::collections::HashSet;

use async_trait::async_trait;
use deve_sub_domain::{
    ItemParseStatus, Node, NodePoolRepository, ReconcileInput, ReconcileResult, SourceError,
};
use deve_sub_kernel::{NodeSourceBindingId, SourceItemId};
use sqlx::sqlite::SqlitePool;

use crate::timestamp::format_ts;

/// SQLite-backed node pool repository.
pub struct SqliteNodePoolRepository {
    pool: SqlitePool,
}

impl SqliteNodePoolRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
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
                let port_i64 = i64::from(node.endpoint.port);

                let active: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM nodes \
                     WHERE protocol_kind = ? AND host = ? AND port = ? \
                     AND missing_from_source = 0 \
                     LIMIT 1",
                )
                .bind(&proto_str)
                .bind(&host_str)
                .bind(port_i64)
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
                    // with the same dedup key. A missing node has no binding
                    // to this source (it was deleted in a prior refresh), so a
                    // bindings JOIN would miss it. The dedup unique index
                    // guarantees at most one active node per key but allows
                    // multiple missing ones; we take the first.
                    let missing: Option<(String,)> = sqlx::query_as(
                        "SELECT id FROM nodes \
                         WHERE protocol_kind = ? AND host = ? AND port = ? \
                         AND missing_from_source = 1 \
                         LIMIT 1",
                    )
                    .bind(&proto_str)
                    .bind(&host_str)
                    .bind(port_i64)
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
                        let new_id = insert_node(&mut tx, node, &proto_str, &host_str).await?;
                        node_id_opt = Some(new_id);
                        result.new_nodes += 1;
                    }
                }
            }

            // Insert the source_item with the final parse status.
            sqlx::query(
                "INSERT INTO source_items (id, snapshot_id, raw_uri, parse_status) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(SourceItemId::new().to_string())
            .bind(input.snapshot.id.to_string())
            .bind(&entry.raw_uri)
            .bind(final_status.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;

            // Create a binding if this entry produced or matched a node.
            if let Some(node_id) = &node_id_opt {
                sqlx::query(
                    "INSERT INTO node_source_bindings (id, node_id, source_id, raw_uri) \
                     VALUES (?, ?, ?, ?)",
                )
                .bind(NodeSourceBindingId::new().to_string())
                .bind(node_id)
                .bind(input.source_id.to_string())
                .bind(&entry.raw_uri)
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

        // 7. Commit the transaction.
        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        Ok(result)
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

    sqlx::query(
        "INSERT INTO nodes \
         (id, display_name, protocol_kind, host, port, protocol_config_json, \
         authentication_json, tls_json, transport_json, udp_capability, \
         multiplex_json, obfuscation_json, congestion_json, region, extras_json, \
         imported_at, revision, status, missing_from_source) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'active', 0)",
    )
    .bind(&node_id)
    .bind(&node.display_name)
    .bind(proto_str)
    .bind(host_str)
    .bind(i64::from(node.endpoint.port))
    .bind(&config_json)
    .bind(&auth_json)
    .bind(&tls_json)
    .bind(&transport_json)
    .bind(&udp_json)
    .bind(&multiplex_json)
    .bind(&obfuscation_json)
    .bind(&congestion_json)
    .bind(&node.region.value)
    .bind(&extras_json)
    .bind(&imported_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| SourceError::Storage(e.to_string()))?;

    Ok(node_id)
}
