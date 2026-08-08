//! Source aggregate and source type enum.

use deve_sub_kernel::{SourceId, Timestamp};
use serde::{Deserialize, Serialize};

/// The input format of a subscription source.
///
/// Stored as TEXT in the database and serialized as `snake_case` in JSON.
/// The `auto` variant instructs the fetcher to auto-detect the format from
/// the response content type and body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    /// Auto-detect from content type and body.
    Auto,
    /// Base64-encoded URI list.
    Base64,
    /// One URI per line.
    UriList,
    /// Mihomo (Clash) YAML.
    MihomoYaml,
    /// sing-box JSON.
    SingboxJson,
    /// Xray JSON.
    XrayJson,
    /// V2Ray JSON.
    V2rayJson,
    /// Shadowrocket share list.
    Shadowrocket,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Base64 => write!(f, "base64"),
            Self::UriList => write!(f, "uri_list"),
            Self::MihomoYaml => write!(f, "mihomo_yaml"),
            Self::SingboxJson => write!(f, "singbox_json"),
            Self::XrayJson => write!(f, "xray_json"),
            Self::V2rayJson => write!(f, "v2ray_json"),
            Self::Shadowrocket => write!(f, "shadowrocket"),
        }
    }
}

impl std::str::FromStr for SourceType {
    type Err = super::error::SourceError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "base64" => Ok(Self::Base64),
            "uri_list" => Ok(Self::UriList),
            "mihomo_yaml" => Ok(Self::MihomoYaml),
            "singbox_json" => Ok(Self::SingboxJson),
            "xray_json" => Ok(Self::XrayJson),
            "v2ray_json" => Ok(Self::V2rayJson),
            "shadowrocket" => Ok(Self::Shadowrocket),
            other => Err(super::error::SourceError::InvalidSourceType(
                other.to_owned(),
            )),
        }
    }
}

/// Include/exclude filter rules applied to parsed nodes before reconcile
/// (SRC-010). Entries that do not match an include rule, or that match an
/// exclude rule, are dropped (`node = None`, status = `Filtered`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFilterRules {
    /// Protocols to keep; empty means keep all. Compared case-insensitively
    /// against [`ProtocolKind::as_filter_key`](crate::ProtocolKind::as_filter_key).
    pub include_protocols: Vec<String>,
    /// Protocols to drop; takes precedence over `include_protocols`.
    pub exclude_protocols: Vec<String>,
    /// Regions to keep; empty means keep all. Compared case-insensitively
    /// against the node's region value.
    pub include_regions: Vec<String>,
    /// Regions to drop; takes precedence over `include_regions`.
    pub exclude_regions: Vec<String>,
}

/// The source aggregate root.
///
/// Represents a subscription source: a URL that is periodically fetched and
/// parsed into the unified node pool. Each refresh creates a
/// [`SourceSnapshot`](super::snapshot::SourceSnapshot) recording the fetched
/// content and resulting node count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Unique identifier (ULID).
    pub id: SourceId,
    /// Human-readable name.
    pub name: String,
    /// Input format. `Auto` lets the fetcher detect.
    pub source_type: SourceType,
    /// Subscription URL.
    pub url: String,
    /// HTTP method (currently only `GET`).
    pub http_method: String,
    /// Encrypted custom headers (ciphertext, base64). `None` if no custom
    /// headers.
    pub headers_encrypted: Option<String>,
    /// Whether automatic refresh is enabled.
    pub auto_update: bool,
    /// Refresh interval in seconds.
    pub update_interval_secs: u64,
    /// Whether the source is active.
    pub enabled: bool,
    /// Whether to keep existing nodes if a refresh fails. When `false`, a
    /// failed refresh marks the source as errored.
    pub keep_on_fail: bool,
    /// Include/exclude filter rules applied to parsed nodes before reconcile
    /// (SRC-010). `None` means no filtering.
    pub filter_rules: Option<SourceFilterRules>,
    /// Creation time.
    pub created_at: Timestamp,
}

impl Source {
    /// Create a new enabled source with default settings.
    #[must_use]
    pub fn new(name: &str, source_type: SourceType, url: String) -> Self {
        Self {
            id: SourceId::new(),
            name: name.to_owned(),
            source_type,
            url,
            http_method: "GET".to_owned(),
            headers_encrypted: None,
            auto_update: false,
            update_interval_secs: 3600,
            enabled: true,
            keep_on_fail: true,
            filter_rules: None,
            created_at: Timestamp::now(),
        }
    }
}
