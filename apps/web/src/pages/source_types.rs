//! DTO types for the sources page, matching `deve-sub-contract::source`.

#![cfg(target_family = "wasm")]

use serde::{Deserialize, Serialize};

use crate::i18n::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTypeDto {
    Auto,
    Base64,
    UriList,
    MihomoYaml,
    SingboxJson,
    XrayJson,
    V2rayJson,
    Shadowrocket,
}

impl SourceTypeDto {
    pub fn label(self, l: Language) -> &'static str {
        match (l, self) {
            (Language::Zh, Self::Auto) => "自动检测",
            (Language::En, Self::Auto) => "Auto",
            (_, Self::Base64) => "Base64",
            (Language::Zh, Self::UriList) => "URI 列表",
            (Language::En, Self::UriList) => "URI List",
            (_, Self::MihomoYaml) => "Mihomo YAML",
            (_, Self::SingboxJson) => "sing-box JSON",
            (_, Self::XrayJson) => "Xray JSON",
            (_, Self::V2rayJson) => "V2Ray JSON",
            (_, Self::Shadowrocket) => "Shadowrocket",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Base64 => "base64",
            Self::UriList => "uri_list",
            Self::MihomoYaml => "mihomo_yaml",
            Self::SingboxJson => "singbox_json",
            Self::XrayJson => "xray_json",
            Self::V2rayJson => "v2ray_json",
            Self::Shadowrocket => "shadowrocket",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "base64" => Self::Base64,
            "uri_list" => Self::UriList,
            "mihomo_yaml" => Self::MihomoYaml,
            "singbox_json" => Self::SingboxJson,
            "xray_json" => Self::XrayJson,
            "v2ray_json" => Self::V2rayJson,
            "shadowrocket" => Self::Shadowrocket,
            _ => Self::Auto,
        }
    }

    pub const ALL: [Self; 8] = [
        Self::Auto,
        Self::Base64,
        Self::UriList,
        Self::MihomoYaml,
        Self::SingboxJson,
        Self::XrayJson,
        Self::V2rayJson,
        Self::Shadowrocket,
    ];
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceFilterRulesDto {
    #[serde(default)]
    pub include_protocols: Vec<String>,
    #[serde(default)]
    pub exclude_protocols: Vec<String>,
    #[serde(default)]
    pub include_regions: Vec<String>,
    #[serde(default)]
    pub exclude_regions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDto {
    pub id: String,
    pub name: String,
    pub source_type: SourceTypeDto,
    pub url: String,
    pub auto_update: bool,
    pub update_interval_secs: u64,
    pub enabled: bool,
    pub keep_on_fail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_rules: Option<SourceFilterRulesDto>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSourcesResponse {
    pub sources: Vec<SourceDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSourceRequest {
    pub name: String,
    pub source_type: SourceTypeDto,
    pub url: String,
    pub auto_update: bool,
    pub update_interval_secs: u64,
    pub keep_on_fail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_rules: Option<SourceFilterRulesDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSourceRequest {
    pub name: String,
    pub source_type: SourceTypeDto,
    pub url: String,
    pub auto_update: bool,
    pub update_interval_secs: u64,
    pub enabled: bool,
    pub keep_on_fail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_rules: Option<SourceFilterRulesDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResponse {
    pub source: SourceDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileCountsDto {
    pub new_nodes: u64,
    pub duplicate_nodes: u64,
    pub reactivated_nodes: u64,
    pub missing_nodes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshSourceResponse {
    pub snapshot_id: String,
    pub version: u64,
    pub not_modified: bool,
    pub node_count: u64,
    pub reconcile: ReconcileCountsDto,
}
