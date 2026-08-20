//! DTO types for the users page, matching `deve-sub-contract::auth`.

#![cfg(target_family = "wasm")]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub role: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub traffic_quota: u64,
    pub two_factor_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListUsersResponse {
    pub users: Vec<UserDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserResponse {
    pub user: UserDto,
}

pub const ROLES: &[&str] = &["admin", "user"];

/// Modal state machine for the users page.
#[derive(Clone, PartialEq)]
pub enum Modal {
    None,
    Create,
    Disable(UserDto),
    ForceLogout(UserDto),
}
