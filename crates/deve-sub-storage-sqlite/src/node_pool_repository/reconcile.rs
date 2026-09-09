//! Source reconciliation transaction: identity resolution, provenance and publication.
use super::*;

impl SqliteNodePoolRepository {
    pub(super) async fn reconcile_source(
        &self,
        input: ReconcileInput<'_>,
    ) -> Result<ReconcileResult, SourceError> {
        let started = std::time::Instant::now();
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

        // WHY: the first UPDATE above acquired the write lock. No other
        // writer can change identities while this operation-local map lives.
        let fingerprints = input
            .entries
            .iter()
            .map(|entry| {
                entry
                    .node
                    .as_ref()
                    .map(|node| node_fingerprint(node, self.master_key.as_deref()))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let keys: Vec<&str> = fingerprints.iter().filter_map(|f| f.as_deref()).collect();
        let mut identities = std::collections::HashMap::<String, (String, bool)>::new();
        // WHY: group in SQL so multiple historical missing candidates cannot
        // expand a 500-key lookup into an unbounded Rust result set.
        for chunk in keys.chunks(500) {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "SELECT identity_fingerprint, COALESCE(MIN(CASE WHEN missing_from_source = 0 THEN id END), MIN(id)), MIN(missing_from_source) FROM nodes WHERE identity_fingerprint IN (",
            );
            let mut separated = query.separated(",");
            for key in chunk {
                separated.push_bind(*key);
            }
            query.push(") GROUP BY identity_fingerprint");
            let rows: Vec<(String, String, bool)> = query
                .build_query_as()
                .fetch_all(&mut *tx)
                .await
                .map_err(storage_error)?;
            for (fingerprint, id, missing) in rows {
                identities.entry(fingerprint).or_insert((id, missing));
            }
        }
        let mut result = ReconcileResult::default();
        let mut seen: HashSet<String> = HashSet::new();
        for (batch_index, entries) in input.entries.chunks(250).enumerate() {
            let mut items = Vec::with_capacity(entries.len());
            let mut bindings = Vec::with_capacity(entries.len());
            for (offset, entry) in entries.iter().enumerate() {
                let mut final_status = entry.initial_status;
                let mut node_id = None;
                if let (Some(node), Some(fingerprint)) =
                    (&entry.node, &fingerprints[batch_index * 250 + offset])
                {
                    if let Some((id, missing)) = identities.get_mut(fingerprint) {
                        if *missing {
                            sqlx::query("UPDATE nodes SET missing_from_source = 0, revision = revision + 1 WHERE id = ?")
                                .bind(&*id).execute(&mut *tx).await.map_err(storage_error)?;
                            *missing = false;
                            result.reactivated_nodes += 1;
                        } else {
                            if final_status == ItemParseStatus::Parsed {
                                final_status = ItemParseStatus::Duplicate;
                            }
                            result.duplicate_nodes += 1;
                        }
                        node_id = Some(id.clone());
                    } else {
                        let id = insert_node(
                            &mut tx,
                            node,
                            &to_json(&node.protocol)?,
                            &node.endpoint.host.uri_host(),
                            fingerprint,
                            self.master_key.as_deref(),
                        )
                        .await?
                        .ok_or_else(|| {
                            SourceError::Storage(
                                "unexpected node identity conflict inside write transaction".into(),
                            )
                        })?;
                        identities.insert(fingerprint.clone(), (id.clone(), false));
                        node_id = Some(id);
                        result.new_nodes += 1;
                    }
                }
                items.push((
                    SourceItemId::new().to_string(),
                    seal_json(
                        self.master_key.as_deref(),
                        CTX_SOURCE_ITEM_URI,
                        &entry.raw_uri,
                    )?,
                    final_status.to_string(),
                ));
                if let Some(id) = node_id {
                    seen.insert(id.clone());
                    bindings.push((
                        NodeSourceBindingId::new().to_string(),
                        id,
                        seal_json(self.master_key.as_deref(), CTX_BINDING_URI, &entry.raw_uri)?,
                    ));
                }
            }
            let snapshot_id = input.snapshot.id.to_string();
            let source_id = input.source_id.to_string();
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO source_items (id, snapshot_id, raw_uri_encrypted, parse_status) ",
            );
            query.push_values(&items, |mut row, (id, raw, status)| {
                row.push_bind(id)
                    .push_bind(&snapshot_id)
                    .push_bind(raw)
                    .push_bind(status);
            });
            query
                .build()
                .execute(&mut *tx)
                .await
                .map_err(storage_error)?;
            if !bindings.is_empty() {
                let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                    "INSERT INTO node_source_bindings (id, node_id, source_id, raw_uri_encrypted) ",
                );
                query.push_values(&bindings, |mut row, (id, node_id, raw)| {
                    row.push_bind(id)
                        .push_bind(node_id)
                        .push_bind(&source_id)
                        .push_bind(raw);
                });
                query
                    .build()
                    .execute(&mut *tx)
                    .await
                    .map_err(storage_error)?;
            }
        }

        // 6. Mark missing: nodes previously bound to this source that were not
        // seen in this refresh. Their binding was deleted in step 4 and not
        // recreated. If no other source binds them, they become missing.
        let missing_candidates: Vec<String> = old_bound.difference(&seen).cloned().collect();
        if !missing_candidates.is_empty() {
            // WHY: fetch all remaining bindings in one grouped query per chunk
            // instead of one COUNT per node (N+1). A candidate absent from the
            // result has zero remaining bindings and must be marked missing.
            let mut still_bound: HashSet<String> = HashSet::new();
            for chunk in missing_candidates.chunks(500) {
                let placeholders = std::iter::repeat_n("?,", chunk.len())
                    .collect::<String>()
                    .trim_end_matches(',')
                    .to_owned();
                let sql = format!(
                    "SELECT node_id FROM node_source_bindings \
                     WHERE node_id IN ({placeholders}) GROUP BY node_id"
                );
                let mut query = sqlx::query_as::<_, (String,)>(&sql);
                for id in chunk {
                    query = query.bind(id);
                }
                let rows: Vec<(String,)> = query
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(|e| SourceError::Storage(e.to_string()))?;
                for (id,) in rows {
                    still_bound.insert(id);
                }
            }
            for old_node_id in &missing_candidates {
                if !still_bound.contains(old_node_id) {
                    sqlx::query("UPDATE nodes SET missing_from_source = 1 WHERE id = ?")
                        .bind(old_node_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| SourceError::Storage(e.to_string()))?;
                    result.missing_nodes += 1;
                }
            }
        }

        // 7. Bump the global pool revision so stale generation cache entries
        // are invalidated. WHY: the cache key includes pool_revision; bumping
        // here ensures a post-refresh generation produces a new cache entry
        // rather than serving stale content (GEN-015, constraint #19).
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;

        // 8. Retention: keep only the newest SOURCE_SNAPSHOT_RETAIN snapshots
        // for this source. WHY: source_items rows (one per node per refresh)
        // are the dominant storage growth path; ON DELETE CASCADE removes
        // them with their snapshot. The snapshot inserted above is the
        // highest-version row for this source, so the active snapshot is
        // never pruned.
        sqlx::query(
            "DELETE FROM source_snapshots WHERE source_id = ? AND id NOT IN \
             (SELECT id FROM source_snapshots WHERE source_id = ? \
              ORDER BY version DESC LIMIT ?)",
        )
        .bind(input.source_id.to_string())
        .bind(input.source_id.to_string())
        .bind(SOURCE_SNAPSHOT_RETAIN)
        .execute(&mut *tx)
        .await
        .map_err(|e| SourceError::Storage(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| SourceError::Storage(e.to_string()))?;
        tracing::info!(
            entries = input.entries.len(),
            elapsed_ms = started.elapsed().as_millis(),
            new_nodes = result.new_nodes,
            duplicate_nodes = result.duplicate_nodes,
            reactivated_nodes = result.reactivated_nodes,
            "source reconciliation completed"
        );
        Ok(result)
    }
}

fn storage_error(error: sqlx::Error) -> SourceError {
    SourceError::Storage(error.to_string())
}
