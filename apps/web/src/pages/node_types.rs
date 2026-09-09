//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{
    BatchEnabledRequest, BatchResultDto, BatchTagsRequest, CreateTagRequest, ImportNodesRequest,
    ImportNodesResponse, ListNodesResponse, ListTagsResponse, NodeChainResponse, NodeDto,
    NodeTagAssignmentDto, SetNodeChainRequest, SetNodeTagsRequest, SetRegionRequest, SourceTypeDto,
    TagDto, TagResponse, UpdateOverrideRequest,
};

/// Which modal is open on the Nodes page.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeModal {
    /// Modal is closed.
    None,
    /// Import nodes (manual paste).
    Import,
    /// Assign tags to nodes (single or batch). Carries node ULIDs.
    Tags(Vec<String>),
    /// Set manual region on a single node.
    SetRegion(String),
    /// Edit manual override on a single node.
    Override(String),
    /// Edit proxy chain on a single node. Carries node ID and the
    /// current chain (ordered node IDs) for initial display.
    Chain(String, Vec<String>),
}
