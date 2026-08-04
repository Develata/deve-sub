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
    /// Whether two-factor authentication is enabled.
    pub two_factor_enabled: bool,
    /// Last successful login time (ISO 8601 UTC). `None` if never logged in.
    pub last_login_at: Option<String>,
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
///
/// When `requires_2fa` is `true`, the client must complete the 2FA flow
/// using the `challenge_token` via `POST /api/v1/auth/login/2fa`. The
/// session cookie is NOT set in this case.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoginResponse {
    /// The authenticated user.
    pub user: UserDto,
    /// Whether 2FA verification is required to complete login.
    pub requires_2fa: bool,
    /// Challenge token for the 2FA login endpoint. Present only when
    /// `requires_2fa` is `true`.
    pub challenge_token: Option<String>,
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

/// Response body for `POST /api/v1/auth/2fa/setup`.
///
/// Returns the TOTP secret (Base32) for manual entry and an `otpauth://` URI
/// for QR code generation. The secret is not yet active until verified.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TwoFactorSetupResponse {
    /// Base32-encoded TOTP secret (e.g. `JBSWY3DPEHPK3PXP`).
    pub secret: String,
    /// `otpauth://` URI for QR code generation.
    pub otpauth_uri: String,
}

/// Request body for `POST /api/v1/auth/2fa/verify`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TwoFactorVerifyRequest {
    /// 6-digit TOTP code from the user's authenticator app.
    pub code: String,
}

/// Response body for `POST /api/v1/auth/2fa/verify`.
///
/// Recovery codes are shown once. The user must store them securely.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TwoFactorVerifyResponse {
    /// Single-use recovery codes.
    pub recovery_codes: Vec<String>,
}

/// Request body for `POST /api/v1/auth/2fa/disable`.
///
/// Requires the current password to prevent unauthorized disabling from a
/// hijacked session.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TwoFactorDisableRequest {
    /// Current password for re-authentication.
    pub password: String,
}

/// Request body for `POST /api/v1/auth/2fa/recovery-codes`.
///
/// Requires the current password to prevent unauthorized regeneration.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegenerateRecoveryCodesRequest {
    /// Current password for re-authentication.
    pub password: String,
}

/// Response body for `POST /api/v1/auth/2fa/recovery-codes`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegenerateRecoveryCodesResponse {
    /// New single-use recovery codes (old codes are invalidated).
    pub recovery_codes: Vec<String>,
}

/// Request body for `POST /api/v1/auth/login/2fa`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoginTwoFactorRequest {
    /// Challenge token from the login response.
    pub challenge_token: String,
    /// 6-digit TOTP code or a recovery code.
    pub code: String,
}
