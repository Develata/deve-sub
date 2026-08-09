//! User aggregate and role enum.

use deve_sub_kernel::{Timestamp, UserId};
use serde::{Deserialize, Serialize};

/// User role for authorization.
///
/// Stored as TEXT in the database (`"admin"` or `"user"`) and serialized as
/// `snake_case` in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full administrative access: user management, force logout, system config.
    Admin,
    /// Regular user: can manage own subscriptions and view own resources.
    User,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admin => write!(f, "admin"),
            Self::User => write!(f, "user"),
        }
    }
}

impl std::str::FromStr for Role {
    type Err = super::error::IdentityError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Self::Admin),
            "user" => Ok(Self::User),
            other => Err(super::error::IdentityError::InvalidRole(other.to_owned())),
        }
    }
}

/// The user aggregate root.
///
/// Represents an authenticated identity with credentials, role, and lifecycle
/// state. Sessions reference a user by [`UserId`]; disabling a user revokes
/// all their sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Unique identifier (ULID).
    pub id: UserId,
    /// Unique login name.
    pub username: String,
    /// Argon2id PHC-format password hash.
    pub password_hash: String,
    /// Authorization role.
    pub role: Role,
    /// Whether the user can log in. Disabled users' sessions are revoked.
    pub enabled: bool,
    /// Optional account expiry. Expired users cannot log in.
    pub expires_at: Option<Timestamp>,
    /// Traffic quota in bytes (0 = unlimited). Enforced in M6.
    pub traffic_quota: u64,
    /// Whether two-factor authentication is enabled.
    pub two_factor_enabled: bool,
    /// Timestamp of the last successful login. `None` if never logged in.
    pub last_login_at: Option<Timestamp>,
    /// Account creation time.
    pub created_at: Timestamp,
}

impl User {
    /// Create a new enabled user with no expiry, zero quota, and 2FA disabled.
    #[must_use]
    pub fn new(username: &str, password_hash: String, role: Role) -> Self {
        Self {
            id: UserId::new(),
            username: username.to_owned(),
            password_hash,
            role,
            enabled: true,
            expires_at: None,
            traffic_quota: 0,
            two_factor_enabled: false,
            last_login_at: None,
            created_at: Timestamp::now(),
        }
    }

    /// Whether the user can authenticate.
    ///
    /// A user is active when enabled and not expired.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.expires_at.is_none_or(|e| e > Timestamp::now())
    }

    /// Whether the user's account has expired.
    ///
    /// Distinct from `!is_active()`: a disabled-but-unexpired user returns
    /// `false` here. Delivery uses this to distinguish 404 (disabled, no
    /// existence leak) from 403 (expired, clear error per OUT-010).
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|e| e <= Timestamp::now())
    }

    /// Whether the user's traffic quota is exceeded.
    ///
    /// `traffic_quota == 0` means unlimited (never exceeded). Otherwise the
    /// consumed total must exceed the quota. The caller supplies the consumed
    /// total (aggregated from the user's subscriptions' traffic records).
    #[must_use]
    pub fn is_traffic_exceeded(&self, consumed: u64) -> bool {
        self.traffic_quota > 0 && consumed > self.traffic_quota
    }
}
