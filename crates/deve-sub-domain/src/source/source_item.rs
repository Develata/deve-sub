//! Source item entity: a raw entry within a source snapshot.

use std::fmt::{self, Display};
use std::str::FromStr;

use deve_sub_kernel::{SourceItemId, SourceSnapshotId};
use serde::{Deserialize, Serialize};

/// A raw entry within a source snapshot, recording its original text and
/// parse outcome.
///
/// Each entry in a fetched subscription response becomes a `SourceItem`.
/// Entries that parse successfully into a [`crate::Node`] are also upserted
/// into the node pool; entries that fail or are duplicates are recorded for
/// diagnostics but do not create new pool entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItem {
    /// Unique identifier (ULID).
    pub id: SourceItemId,
    /// The snapshot this item belongs to.
    pub snapshot_id: SourceSnapshotId,
    /// Raw text of the entry: a share URI for URI-based formats, or a
    /// serialized fragment for container formats.
    pub raw_uri: String,
    /// Outcome of parsing this entry.
    pub parse_status: ItemParseStatus,
}

/// Parse outcome for a source item.
///
/// Stored as `snake_case` TEXT in the `source_items` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemParseStatus {
    /// Successfully parsed into a supported node.
    Parsed,
    /// The node is a duplicate of an existing pool entry (same protocol,
    /// host, port). Set by the reconciler, not the parser.
    Duplicate,
    /// The protocol is recognized but not P0-typed; the node is preserved
    /// as `ProtocolConfig::Unsupported`.
    Unsupported,
    /// The entry could not be parsed at all.
    Failed,
    /// The node was dropped by source-level include/exclude filter rules
    /// (SRC-010) before reconcile.
    Filtered,
}

impl Display for ItemParseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parsed => write!(f, "parsed"),
            Self::Duplicate => write!(f, "duplicate"),
            Self::Unsupported => write!(f, "unsupported"),
            Self::Failed => write!(f, "failed"),
            Self::Filtered => write!(f, "filtered"),
        }
    }
}

impl FromStr for ItemParseStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "parsed" => Ok(Self::Parsed),
            "duplicate" => Ok(Self::Duplicate),
            "unsupported" => Ok(Self::Unsupported),
            "failed" => Ok(Self::Failed),
            "filtered" => Ok(Self::Filtered),
            other => Err(format!("invalid parse status: {other}")),
        }
    }
}
