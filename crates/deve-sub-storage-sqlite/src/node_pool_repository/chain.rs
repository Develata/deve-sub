//! Validate and publish chain changes from one protected graph snapshot.
use super::*;
use deve_sub_domain::{NodeChain, NodeChainEntry, NodeChainError, NodeChainGraph};

impl SqliteNodePoolRepository {
    pub(super) async fn set_chain_checked(
        &self,
        node_id: NodeId,
        chain: Option<&[NodeId]>,
    ) -> Result<(), SourceError> {
        // SQLite serializes writers before the graph read, so two opposing
        // updates cannot both validate an obsolete acyclic snapshot.
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(storage)?;
        let target: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?)")
            .bind(node_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(storage)?;
        if !target {
            return Err(SourceError::NodeNotFound(node_id.to_string()));
        }
        if let Some(ids) = chain {
            NodeChain::new(ids.to_vec())?.validate_structure(node_id)?;
            for id in ids {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?)")
                        .bind(id.to_string())
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(storage)?;
                if !exists {
                    return Err(NodeChainError::NodeNotFound(vec![*id]).into());
                }
            }
            let rows: Vec<(String, String)> =
                sqlx::query_as("SELECT id, chain_json FROM nodes WHERE chain_json IS NOT NULL")
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(storage)?;
            let entries = rows
                .into_iter()
                .map(|(id, json)| {
                    Ok(NodeChainEntry {
                        node_id: NodeId::parse(&id)
                            .map_err(|e| SourceError::Storage(e.to_string()))?,
                        chain: serde_json::from_str(&json)
                            .map_err(|e| SourceError::Storage(e.to_string()))?,
                    })
                })
                .collect::<Result<Vec<_>, SourceError>>()?;
            if let Some(cycle) = NodeChainGraph::validate_update(&entries, node_id, ids) {
                return Err(NodeChainError::Cycle(cycle).into());
            }
        }
        let json = chain.map(to_json).transpose()?;
        sqlx::query("UPDATE nodes SET chain_json = ? WHERE id = ?")
            .bind(json)
            .bind(node_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        crate::pool_meta_repository::bump_revision_tx(&mut tx).await?;
        tx.commit().await.map_err(storage)?;
        Ok(())
    }
}

fn storage(error: sqlx::Error) -> SourceError {
    SourceError::Storage(error.to_string())
}
