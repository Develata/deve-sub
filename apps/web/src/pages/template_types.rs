//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{
    ActiveTemplateVersionResponse, CreateTemplateRequest, GenerationResultDto,
    ListTemplatesResponse, ListVersionsResponse, RollbackRequest, RollbackTemplateResponse,
    TemplateDto, TemplateResponse, TemplateVersionDto, UpdateTemplateRequest,
};

pub const DEFAULT_CLASH_YAML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/templates/clash-routing.yaml"
));

pub const PROFILES: &[&str] = &[
    "mihomo",
    "sing-box",
    "xray",
    "v2ray",
    "shadowrocket",
    "uri_list",
];

/// Modal state machine for the templates page.
#[derive(Clone, PartialEq)]
pub enum Modal {
    None,
    Create,
    Edit(TemplateDto),
    Delete(TemplateDto),
    Versions(TemplateDto),
    Rollback {
        template: TemplateDto,
        version: TemplateVersionDto,
    },
    Generate(TemplateDto),
    Preview(TemplateDto),
}
