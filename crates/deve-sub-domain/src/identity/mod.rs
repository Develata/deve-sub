//! Identity module: user, session, TOTP secret, recovery codes, role, and
//! port traits.
//!
//! This module owns the domain model for authentication and authorization.
//! It defines the `User` aggregate, `Session` entity, `TotpSecret` and
//! `RecoveryCode` entities, `Role` enum, and the `UserRepository` /
//! `SessionRepository` / `TotpSecretRepository` / `RecoveryCodeRepository`
//! port traits. Adapters implement the port traits; the application layer
//! calls them. See `docs/plan/03-architecture.md` for the hexagonal layering.

pub mod error;
pub mod ports;
pub mod recovery_code;
pub mod session;
pub mod totp;
pub mod user;

pub use error::IdentityError;
pub use ports::{RecoveryCodeRepository, SessionRepository, TotpSecretRepository, UserRepository};
pub use recovery_code::RecoveryCode;
pub use session::Session;
pub use totp::TotpSecret;
pub use user::{Role, User};
