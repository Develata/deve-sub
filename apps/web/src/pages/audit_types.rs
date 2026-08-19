//! DTO types for the audit log page, matching `deve-sub-contract::audit`.

#![cfg(target_family = "wasm")]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogDto {
    pub id: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub details_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAuditLogsResponse {
    pub entries: Vec<AuditLogDto>,
    pub next_cursor: Option<String>,
}

pub const ACTIONS: &[&str] = &[
    "auth.login",
    "auth.login_2fa",
    "auth.logout",
    "auth.2fa.enable",
    "auth.2fa.disable",
    "user.create",
    "user.disable",
    "user.force_logout",
    "source.create",
    "source.update",
    "source.delete",
    "source.refresh",
    "subscription.create",
    "subscription.update",
    "subscription.delete",
    "subscription.rotate_token",
    "subscription.regen_short_code",
    "template.create",
    "template.update",
    "template.delete",
    "template.rollback",
    "template.generate",
    "template.preview",
];

pub const TARGET_TYPES: &[&str] = &[
    "user",
    "source",
    "subscription",
    "template",
    "node",
    "probe",
];
