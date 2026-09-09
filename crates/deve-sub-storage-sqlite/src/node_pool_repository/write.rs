//! Node serialization and encrypted insertion.
use super::*;

/// Insert a new node into the `nodes` table.
///
/// `created_at` uses the column DEFAULT (current UTC time). `revision` is 0,
/// `status` is `'active'`, `missing_from_source` is 0 for new nodes.
pub(super) async fn insert_node(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    node: &Node,
    proto_str: &str,
    host_str: &str,
    fingerprint: &str,
    key: Option<&MasterKey>,
) -> Result<Option<String>, SourceError> {
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

    // WHY: two concurrent refreshes for different sources that both yield the
    // same node (e.g. an airport listed in two sources) each run their own
    // write transaction. Both pass the fingerprint existence check in their
    // own snapshot, then race to INSERT. The loser would hit the
    // `idx_nodes_dedup` partial UNIQUE index and abort the entire reconcile
    // transaction. `ON CONFLICT DO NOTHING` makes the loser's INSERT a no-op
    // (rows_affected == 0), and the caller re-queries the winning node and
    // counts it as a duplicate — keeping the transaction alive (SRC-016).
    let result = sqlx::query(
        "INSERT INTO nodes \
         (id, display_name, protocol_kind, host, port, \
         protocol_config_json_encrypted, authentication_json_encrypted, \
         tls_json_encrypted, transport_json_encrypted, \
         udp_capability, multiplex_json, obfuscation_json_encrypted, \
         congestion_json, region, extras_json_encrypted, \
         imported_at, revision, status, missing_from_source, source_label, \
         identity_fingerprint) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'active', 0, ?, ?) \
         ON CONFLICT DO NOTHING",
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

    if result.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(node_id))
}
