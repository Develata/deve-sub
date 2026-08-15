//! Row mapping helpers for the `nodes` table.
//!
//! [`NodeRow`] is the sqlx row type shared by `list_nodes` and `get_node`.
//! [`NODE_COLUMNS`] is the shared column list with a COALESCE for
//! `source_label` that prefers the persisted column value and falls back to
//! the `node_source_bindings` JOIN for source-bound nodes whose
//! `source_label` column is empty.

use deve_sub_domain::{
    Host, Node, NodeOverride, NodePoolEntry, NodeSource, RegionAssignment, RegionMethod,
    SourceError, Tag,
};
use deve_sub_kernel::{NodeId, NodeOverrideId};
use deve_sub_security::{MasterKey, envelope};

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

/// Decrypt an encrypted column, falling back to the plaintext column when the
/// encrypted column is NULL or no key is set. See ADR-0007.
fn open_field(
    key: Option<&MasterKey>,
    encrypted: &Option<String>,
    plaintext: &str,
) -> Result<String, SourceError> {
    match (key, encrypted) {
        (Some(k), Some(env)) => {
            let bytes = envelope::open(k.as_bytes(), env)
                .map_err(|e| SourceError::Storage(format!("decryption failed: {e}")))?;
            String::from_utf8(bytes)
                .map_err(|e| SourceError::Storage(format!("decrypted value is not UTF-8: {e}")))
        }
        _ => Ok(plaintext.to_owned()),
    }
}

/// Decrypt an optional encrypted column, falling back to the plaintext column.
fn open_field_opt(
    key: Option<&MasterKey>,
    encrypted: &Option<String>,
    plaintext: Option<&str>,
) -> Result<Option<String>, SourceError> {
    match (key, encrypted) {
        (Some(k), Some(env)) => {
            let bytes = envelope::open(k.as_bytes(), env)
                .map_err(|e| SourceError::Storage(format!("decryption failed: {e}")))?;
            let s = String::from_utf8(bytes)
                .map_err(|e| SourceError::Storage(format!("decrypted value is not UTF-8: {e}")))?;
            Ok(Some(s))
        }
        _ => Ok(plaintext.map(str::to_owned)),
    }
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
    protocol_config_json_encrypted: Option<String>,
    authentication_json: String,
    authentication_json_encrypted: Option<String>,
    tls_json: Option<String>,
    tls_json_encrypted: Option<String>,
    transport_json: Option<String>,
    transport_json_encrypted: Option<String>,
    udp_capability: Option<String>,
    multiplex_json: Option<String>,
    obfuscation_json: Option<String>,
    obfuscation_json_encrypted: Option<String>,
    congestion_json: Option<String>,
    region: Option<String>,
    chain_json: Option<String>,
    extras_json: String,
    extras_json_encrypted: Option<String>,
    imported_at: String,
    revision: i64,
    status: String,
    missing_from_source: i64,
    created_at: String,
    source_label: Option<String>,
    override_id: Option<String>,
    override_display_name: Option<String>,
    override_region: Option<String>,
    override_enabled: Option<i64>,
    override_sni: Option<String>,
    override_skip_cert_verify: Option<i64>,
    override_fingerprint: Option<String>,
    override_sort_order: Option<i64>,
    tags_json: Option<String>,
}

impl NodeRow {
    /// Reconstruct a full [`NodePoolEntry`] from the denormalized row.
    ///
    /// Effective field resolution applies the [`NodeOverride`] (if present)
    /// on top of the parsed node: `display_name` and `region` fall back to
    /// the override value when set, and `is_active` is forced by
    /// `override_enabled` when `Some` (NODE-004). Tags are parsed from the
    /// `tags_json` JSON array produced by the `json_group_array` subquery.
    pub(crate) fn to_pool_entry(
        &self,
        key: Option<&MasterKey>,
    ) -> Result<NodePoolEntry, SourceError> {
        let node_id = NodeId::parse(&self.id).map_err(|e| SourceError::Storage(e.to_string()))?;

        let override_info = match &self.override_id {
            Some(id_str) => Some(NodeOverride {
                id: NodeOverrideId::parse(id_str)
                    .map_err(|e| SourceError::Storage(e.to_string()))?,
                node_id,
                display_name: self.override_display_name.clone(),
                region: self.override_region.clone(),
                enabled: self.override_enabled.map(|e| e != 0),
                sni: self.override_sni.clone(),
                skip_cert_verify: self.override_skip_cert_verify.map(|e| e != 0),
                fingerprint: self.override_fingerprint.clone(),
                sort_order: self.override_sort_order.unwrap_or(0),
            }),
            None => None,
        };

        let effective_display_name = self
            .override_display_name
            .clone()
            .unwrap_or_else(|| self.display_name.clone());

        // WHY: an override region switches the method to Manual (NODE-006);
        // absent an override, the method stays Auto with the persisted value.
        let region = if self.override_region.is_some() {
            RegionAssignment {
                method: RegionMethod::Manual,
                value: self.override_region.clone(),
            }
        } else {
            RegionAssignment {
                method: RegionMethod::Auto,
                value: self.region.clone(),
            }
        };

        let tags: Vec<Tag> = from_json_opt(self.tags_json.as_deref())?.unwrap_or_default();

        let config_json = open_field(
            key,
            &self.protocol_config_json_encrypted,
            &self.protocol_config_json,
        )?;
        let auth_json = open_field(
            key,
            &self.authentication_json_encrypted,
            &self.authentication_json,
        )?;
        let tls_json = open_field_opt(key, &self.tls_json_encrypted, self.tls_json.as_deref())?;
        let transport_json = open_field_opt(
            key,
            &self.transport_json_encrypted,
            self.transport_json.as_deref(),
        )?;
        let obfuscation_json = open_field_opt(
            key,
            &self.obfuscation_json_encrypted,
            self.obfuscation_json.as_deref(),
        )?;
        let extras_json = open_field(key, &self.extras_json_encrypted, &self.extras_json)?;

        let node = Node {
            id: node_id,
            display_name: effective_display_name,
            protocol: from_json(&self.protocol_kind)?,
            config: from_json(&config_json)?,
            endpoint: deve_sub_domain::Endpoint {
                host: Host::parse_uri_host(&self.host),
                port: u16::try_from(self.port)
                    .map_err(|_| SourceError::Storage("port out of range".to_owned()))?,
            },
            authentication: from_json(&auth_json)?,
            transport: from_json_opt(transport_json.as_deref())?,
            tls: from_json_opt(tls_json.as_deref())?,
            udp: self
                .udp_capability
                .as_deref()
                .map(from_json)
                .transpose()?
                .unwrap_or_default(),
            multiplex: from_json_opt(self.multiplex_json.as_deref())?,
            obfuscation: from_json_opt(obfuscation_json.as_deref())?,
            congestion: from_json_opt(self.congestion_json.as_deref())?,
            chain: from_json_opt(self.chain_json.as_deref())?,
            source: NodeSource {
                source_label: self.source_label.clone().unwrap_or_default(),
                raw_uri: None,
                imported_at: parse_ts(&self.imported_at).map_err(SourceError::Storage)?,
            },
            tags: Vec::new(),
            region,
            extras: from_json(&extras_json)?,
        };

        // WHY: override_enabled=Some forces active/inactive; None keeps the
        // node's natural status (NODE-004).
        let is_active = self
            .override_enabled
            .map_or(self.status == "active", |e| e != 0);

        Ok(NodePoolEntry {
            node,
            missing_from_source: self.missing_from_source != 0,
            is_active,
            revision: u64::try_from(self.revision)
                .map_err(|_| SourceError::Storage("revision out of range".to_owned()))?,
            created_at: parse_ts(&self.created_at).map_err(SourceError::Storage)?,
            override_info,
            tags,
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
     n.protocol_config_json, n.protocol_config_json_encrypted, \
     n.authentication_json, n.authentication_json_encrypted, \
     n.tls_json, n.tls_json_encrypted, \
     n.transport_json, n.transport_json_encrypted, \
     n.udp_capability, n.multiplex_json, \
     n.obfuscation_json, n.obfuscation_json_encrypted, \
     n.congestion_json, n.region, n.chain_json, \
     n.extras_json, n.extras_json_encrypted, \
     n.imported_at, n.revision, n.status, \
     n.missing_from_source, n.created_at, \
     COALESCE(NULLIF(n.source_label, ''), \
      (SELECT s.name FROM node_source_bindings b \
       JOIN sources s ON s.id = b.source_id \
       WHERE b.node_id = n.id LIMIT 1)) AS source_label, \
     o.id AS override_id, o.display_name AS override_display_name, o.region AS override_region, \
     o.enabled AS override_enabled, o.sni AS override_sni, o.skip_cert_verify AS override_skip_cert_verify, \
     o.fingerprint AS override_fingerprint, o.sort_order AS override_sort_order, \
     (SELECT json_group_array(json_object('id', t.id, 'name', t.name, 'color', t.color)) \
      FROM node_tags nt JOIN tags t ON t.id = nt.tag_id WHERE nt.node_id = n.id) AS tags_json";
