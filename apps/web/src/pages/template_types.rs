//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{
    CreateTemplateRequest, GenerationResultDto, GetTemplateResponse, ListTemplatesResponse,
    ListVersionsResponse, RollbackRequest, RollbackTemplateResponse, TemplateDto, TemplateResponse,
    TemplateVersionDto, UpdateTemplateRequest,
};

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
