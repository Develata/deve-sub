//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{CreateUserRequest, CreateUserResponse, ListUsersResponse, UserDto};

pub const ROLES: &[&str] = &["admin", "user"];

/// Modal state machine for the users page.
#[derive(Clone, PartialEq)]
pub enum Modal {
    None,
    Create,
    Disable(UserDto),
    ForceLogout(UserDto),
}
