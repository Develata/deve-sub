//! Row mapping helpers for the `nodes` table.
//!
//! [`NodeRow`] is the sqlx row type shared by `list_nodes` and `get_node`.
//! [`NODE_COLUMNS`] is the shared column list with a COALESCE for
//! `source_label` that prefers the persisted column value and falls back to
//! the `node_source_bindings` JOIN for source-bound nodes whose
//! `source_label` column is empty.

use deve_sub_domain::{
    Host, Node, NodePoolEntry, NodeSource, RegionAssignment, RegionMethod, SourceError,
};
use deve_sub_kernel::NodeId;

use crate::timestamp::parse_ts;

/// Deserialize a JSON string into `T`, mapping errors to [`SourceError`].
fn from_json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, SourceError> {
    serde_json::from_str(s).map_err(|e| SourceError::Storage(format!("json decode error: {e}")))
}

/// Deserialize an optional JSON column (`None` or NULL → `None`).
fn from_json_opt<T: serde::de::DeserializeOwned>(
    s: Option<&str>,
) -> Result<Option<T>, SourceError> {
    s.map(from_json).transpose()
}

/// Raw row from the `nodes` table plus a COALESCE for the source label.
#[derive(sqlx::FromRow)]
pub(crate) struct NodeRow {
    id: String,
    display_name: String,
    protocol_kind: String,
    host: String,
    port: i64,
    protocol_config_json: String,
    authentication_json: String,
    tls_json: Option<String>,
    transport_json: Option<String>,
    udp_capability: Option<String>,
    multiplex_json: Option<String>,
    obfuscation_json: Option<String>,
    congestion_json: Option<String>,
    region: Option<String>,
    extras_json: String,
    imported_at: String,
    revision: i64,
    status: String,
    missing_from_source: i64,
    created_at: String,
    source_label: Option<String>,
}

impl NodeRow {
    /// Reconstruct a full [`NodePoolEntry`] from the denormalized row.
    pub(crate) fn to_pool_entry(&self) -> Result<NodePoolEntry, SourceError> {
        let node = Node {
            id: NodeId::parse(&self.id).map_err(|e| SourceError::Storage(e.to_string()))?,
            display_name: self.display_name.clone(),
            protocol: from_json(&self.protocol_kind)?,
            config: from_json(&self.protocol_config_json)?,
            endpoint: deve_sub_domain::Endpoint {
                host: Host::parse_uri_host(&self.host),
                port: u16::try_from(self.port)
                    .map_err(|_| SourceError::Storage("port out of range".to_owned()))?,
            },
            authentication: from_json(&self.authentication_json)?,
            transport: from_json_opt(self.transport_json.as_deref())?,
            tls: from_json_opt(self.tls_json.as_deref())?,
            udp: self
                .udp_capability
                .as_deref()
                .map(from_json)
                .transpose()?
                .unwrap_or_default(),
            multiplex: from_json_opt(self.multiplex_json.as_deref())?,
            obfuscation: from_json_opt(self.obfuscation_json.as_deref())?,
            congestion: from_json_opt(self.congestion_json.as_deref())?,
            chain: None,
            source: NodeSource {
                source_label: self.source_label.clone().unwrap_or_default(),
                raw_uri: None,
                imported_at: parse_ts(&self.imported_at).map_err(SourceError::Storage)?,
            },
            tags: Vec::new(),
            // WHY: RegionMethod::Auto is hardcoded because the method is not
            // persisted in the nodes table (Slice 4 will add manual region
            // override via node_overrides). The region VALUE is persisted.
            region: RegionAssignment {
                method: RegionMethod::Auto,
                value: self.region.clone(),
            },
            extras: from_json(&self.extras_json)?,
        };

        Ok(NodePoolEntry {
            node,
            missing_from_source: self.missing_from_source != 0,
            is_active: self.status == "active",
            revision: u64::try_from(self.revision)
                .map_err(|_| SourceError::Storage("revision out of range".to_owned()))?,
            created_at: parse_ts(&self.created_at).map_err(SourceError::Storage)?,
        })
    }
}

/// Column list for node queries, shared by `list_nodes` and `get_node`.
///
/// WHY: `source_label` uses COALESCE to prefer the persisted column value
/// (set to `"manual"` for manually-imported nodes) and fall back to the
/// `node_source_bindings` JOIN for source-bound nodes whose `source_label`
/// column is empty (e.g. pre-migration rows or nodes inserted via reconcile
/// where the protocol parser leaves `source_label` empty).
pub(crate) const NODE_COLUMNS: &str = "n.id, n.display_name, n.protocol_kind, n.host, n.port, \
     n.protocol_config_json, n.authentication_json, n.tls_json, n.transport_json, \
     n.udp_capability, n.multiplex_json, n.obfuscation_json, n.congestion_json, \
     n.region, n.extras_json, n.imported_at, n.revision, n.status, \
     n.missing_from_source, n.created_at, \
     COALESCE(NULLIF(n.source_label, ''), \
      (SELECT s.name FROM node_source_bindings b \
       JOIN sources s ON s.id = b.source_id \
       WHERE b.node_id = n.id LIMIT 1)) AS source_label";
