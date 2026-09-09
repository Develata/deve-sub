//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{
    CreateSourceRequest, ListSourcesResponse, RefreshJobAcceptedResponse, SourceDto,
    SourceRefreshJobDto, SourceResponse, SourceTypeDto, UpdateSourceRequest,
};

use crate::i18n::Language;

/// Presentation labels and selection options for the shared source type.
pub trait SourceTypePresentation: Sized {
    fn label(self, language: Language) -> &'static str;
    fn as_str(self) -> &'static str;
    fn from_str(value: &str) -> Self;
    const ALL: [Self; 8];
}

impl SourceTypePresentation for SourceTypeDto {
    fn label(self, l: Language) -> &'static str {
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

    fn as_str(self) -> &'static str {
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

    fn from_str(s: &str) -> Self {
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

    const ALL: [Self; 8] = [
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
