//! Operator-mapped historical security recovery, preserving original identities.
use crate::{Node, TlsConfig};
use deve_sub_kernel::NodeId;

/// Unmodified canonical base fields, before any user override is applied.
pub struct NodeSecuritySnapshot {
    pub node: Node,
    pub revision: u64,
    pub fingerprint: String,
}

/// Explicit reviewed change. Other node data is not part of this write boundary.
pub enum NodeSecurityChange {
    Quarantine,
    RestoreTls(TlsConfig),
}

/// One exact old identity and its input evidence, never matched by display name.
pub struct NodeSecurityAction {
    pub node_id: NodeId,
    pub expected_revision: u64,
    pub expected_fingerprint: String,
    pub evidence_sha256: String,
    pub change: NodeSecurityChange,
}

/// A complete action batch bound to the reviewed key and pool snapshot.
pub struct NodeSecurityPlan {
    pub plan_sha256: String,
    pub key_fingerprint: String,
    pub expected_pool_revision: u64,
    pub actions: Vec<NodeSecurityAction>,
}
