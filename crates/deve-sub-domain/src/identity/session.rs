//! Session entity for authenticated user sessions.

use deve_sub_kernel::{SessionId, Timestamp, UserId};

/// An authenticated session.
///
/// A session binds a random CSPRNG-generated token (known only to the client)
/// to a user. The database stores the HMAC-SHA256 digest of the token
/// (`token_hash`), never the raw token. Sessions are revocable and expire
/// automatically. See `docs/plan/00-engineering-constitution.md` §"Data and
/// security".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Unique identifier (ULID).
    pub id: SessionId,
    /// The user this session belongs to.
    pub user_id: UserId,
    /// HMAC-SHA256 digest of the raw session token (base64url-encoded).
    pub token_hash: String,
    /// Session creation time.
    pub created_at: Timestamp,
    /// Session expiry. Expired sessions are invalid.
    pub expires_at: Timestamp,
    /// Whether the session has been explicitly revoked.
    pub revoked: bool,
}

impl Session {
    /// Create a new unrevoked session with the given token hash and expiry.
    #[must_use]
    pub fn new(user_id: UserId, token_hash: String, expires_at: Timestamp) -> Self {
        Self {
            id: SessionId::new(),
            user_id,
            token_hash,
            created_at: Timestamp::now(),
            expires_at,
            revoked: false,
        }
    }

    /// Whether the session is still valid.
    ///
    /// A session is valid when not revoked and not expired.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.revoked && self.expires_at > Timestamp::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(expires_at: Timestamp, revoked: bool) -> Session {
        Session {
            id: SessionId::new(),
            user_id: UserId::new(),
            token_hash: "dummy-hash".to_owned(),
            created_at: Timestamp::now(),
            expires_at,
            revoked,
        }
    }

    #[test]
    fn valid_session_is_valid() {
        let future = Timestamp::now() + time::Duration::seconds(3600);
        let session = make_session(future, false);
        assert!(session.is_valid());
    }

    #[test]
    fn expired_session_is_invalid() {
        let past = Timestamp::now() - time::Duration::seconds(1);
        let session = make_session(past, false);
        assert!(!session.is_valid());
    }

    #[test]
    fn revoked_session_is_invalid() {
        let future = Timestamp::now() + time::Duration::seconds(3600);
        let session = make_session(future, true);
        assert!(!session.is_valid());
    }

    #[test]
    fn revoked_and_expired_session_is_invalid() {
        let past = Timestamp::now() - time::Duration::seconds(1);
        let session = make_session(past, true);
        assert!(!session.is_valid());
    }
}
