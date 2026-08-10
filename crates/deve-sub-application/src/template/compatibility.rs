//! Compatibility check: filter resolved nodes by target profile capability.
//!
//! Given a resolved set of node IDs and a target profile, this module reports
//! which nodes are included in and excluded from generation. Incompatible
//! nodes are never silently dropped (constraint #7): they appear in the
//! `excluded` list with a [`CompatibilityReason`].
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Compatibility
//! matrix".

use deve_sub_compatibility::{ProfileKind, capability_for, check_node};
use deve_sub_domain::source::{NodePoolEntry, NodePoolRepository};
use deve_sub_domain::{CompatibilityReport, ExcludedNode};
use deve_sub_kernel::NodeId;

use super::error::TemplateAppError;

/// Check which resolved nodes are compatible with a target profile.
///
/// `node_ids` is the resolved set (from `resolve_template`). Each node is
/// fetched from the pool; its protocol, transport, and config are checked
/// against the profile's capability matrix. Compatible nodes go into
/// `included_node_ids`; incompatible nodes go into `excluded` with a reason.
///
/// Returns a domain [`CompatibilityReport`]; the delivery layer maps it to
/// `CompatibilityReportDto` at the API boundary.
///
/// This is a read-only operation.
pub async fn check_compatibility(
    node_ids: &[NodeId],
    profile: ProfileKind,
    pool_repo: &dyn NodePoolRepository,
) -> Result<CompatibilityReport, TemplateAppError> {
    let cap = capability_for(profile);
    let entries = fetch_nodes(node_ids, pool_repo).await?;

    let mut included = Vec::new();
    let mut excluded = Vec::new();

    for id in node_ids {
        match entries.get(id) {
            Some(entry) => match check_node(&entry.node, &cap) {
                Ok(()) => included.push(*id),
                Err(reason) => excluded.push(ExcludedNode {
                    node_id: *id,
                    display_name: entry.node.display_name.clone(),
                    reason: reason.to_string(),
                }),
            },
            None => excluded.push(ExcludedNode {
                node_id: *id,
                display_name: String::new(),
                reason: "node not found in pool".to_owned(),
            }),
        }
    }

    Ok(CompatibilityReport {
        profile: profile.as_kebab().to_owned(),
        included_node_ids: included,
        excluded,
    })
}

async fn fetch_nodes(
    node_ids: &[NodeId],
    pool_repo: &dyn NodePoolRepository,
) -> Result<std::collections::HashMap<NodeId, NodePoolEntry>, TemplateAppError> {
    let mut map = std::collections::HashMap::new();
    for id in node_ids {
        match pool_repo.get_node(*id).await {
            Ok(Some(entry)) => {
                map.insert(*id, entry);
            }
            Ok(None) => {}
            Err(e) => return Err(TemplateAppError::Storage(e.to_string())),
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deve_sub_domain::source::SourceError;

    struct EmptyPool;

    #[async_trait::async_trait]
    impl NodePoolRepository for EmptyPool {
        async fn reconcile(
            &self,
            _input: deve_sub_domain::source::ReconcileInput<'_>,
        ) -> Result<deve_sub_domain::source::ReconcileResult, SourceError> {
            unimplemented!()
        }
        async fn list_nodes(
            &self,
            _filter: &deve_sub_domain::source::NodeFilter,
            _cursor: Option<NodeId>,
            _limit: u32,
        ) -> Result<Vec<NodePoolEntry>, SourceError> {
            unimplemented!()
        }
        async fn get_node(&self, _id: NodeId) -> Result<Option<NodePoolEntry>, SourceError> {
            Ok(None)
        }
        async fn import_nodes(
            &self,
            _nodes: Vec<deve_sub_domain::Node>,
        ) -> Result<deve_sub_domain::source::ImportResult, SourceError> {
            unimplemented!()
        }
        async fn list_node_chains(
            &self,
        ) -> Result<Vec<deve_sub_domain::NodeChainEntry>, SourceError> {
            Ok(Vec::new())
        }
        async fn existing_node_ids(&self, _ids: &[NodeId]) -> Result<Vec<NodeId>, SourceError> {
            Ok(Vec::new())
        }
        async fn set_node_chain(
            &self,
            _node_id: NodeId,
            _chain: Option<&[NodeId]>,
        ) -> Result<(), SourceError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn unknown_node_reported_as_excluded() {
        let id = NodeId::parse("01KZAAAAAAAAAAAAAAAAAAAAAA").expect("ulid");
        let pool = EmptyPool;
        let report = check_compatibility(&[id], ProfileKind::Mihomo, &pool)
            .await
            .expect("compat");
        assert!(report.included_node_ids.is_empty());
        assert_eq!(report.excluded.len(), 1);
        assert_eq!(report.excluded[0].node_id, id);
        assert_eq!(report.excluded[0].reason, "node not found in pool");
    }

    #[test]
    fn profile_kind_mihomo_roundtrip() {
        assert_eq!(ProfileKind::from_kebab("mihomo"), Some(ProfileKind::Mihomo));
        assert_eq!(ProfileKind::Mihomo.as_kebab(), "mihomo");
    }
}
