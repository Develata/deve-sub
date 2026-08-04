//! Auth and user DTOs for the `/api/v1/auth` and `/api/v1/users` endpoints.
//!
//! These DTOs are the wire format for authentication and user management.
//! They are owned by the contract crate per ADR-0004: DTOs and `ToSchema`
//! derives live here, not in the API crate.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// User role for authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto {
    /// Full administrative access.
    Admin,
    /// Regular user.
    User,
}

/// User information returned by auth and user management endpoints.
///
/// Never includes `password_hash` or other secrets.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserDto {
    /// ULID identifier.
    pub id: String,
    /// Login name.
    pub username: String,
    /// Authorization role.
    pub role: RoleDto,
    /// Whether the user can log in.
    pub enabled: bool,
    /// Optional account expiry (ISO 8601 UTC). `None` means no expiry.
    pub expires_at: Option<String>,
    /// Traffic quota in bytes (0 = unlimited).
    pub traffic_quota: u64,
    /// Account creation time (ISO 8601 UTC).
    pub created_at: String,
}

/// Request body for `POST /api/v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoginRequest {
    /// Username.
    pub username: String,
    /// Plaintext password.
    pub password: String,
}

/// Response body for `POST /api/v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoginResponse {
    /// The authenticated user.
    pub user: UserDto,
}

/// Request body for `POST /api/v1/auth/setup` (initial admin creation).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupAdminRequest {
    /// Admin username.
    pub username: String,
    /// Admin password.
    pub password: String,
}

/// Response body for `POST /api/v1/auth/setup`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupAdminResponse {
    /// The created admin user.
    pub user: UserDto,
}

/// Response body for `GET /api/v1/auth/me`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CurrentUserResponse {
    /// The authenticated user.
    pub user: UserDto,
}

/// Standard error response for auth endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    /// Machine-readable error code (e.g. `"unauthorized"`, `"forbidden"`).
    pub error: String,
    /// Human-readable error message.
    pub message: String,
}

/// Request body for `POST /api/v1/users` (admin-only user creation).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    /// Username.
    pub username: String,
    /// Plaintext password.
    pub password: String,
    /// Authorization role.
    pub role: RoleDto,
}

/// Response body for `POST /api/v1/users`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateUserResponse {
    /// The created user.
    pub user: UserDto,
}

/// Response body for `GET /api/v1/users` (cursor-paginated user list).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListUsersResponse {
    /// Users in the current page.
    pub users: Vec<UserDto>,
    /// Cursor for the next page (`None` if no more results).
    pub next_cursor: Option<String>,
}
