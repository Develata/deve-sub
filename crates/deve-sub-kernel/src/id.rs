//! Strong-typed entity identifiers backed by ULID.
//!
//! ULIDs are server-monotonically generated and lexically sortable by time.
//! The database also stores `created_at` separately. ULIDs identify entities
//! only; they must not be used for login sessions, subscription tokens,
//! recovery codes, or any other secret. See `docs/plan/00-engineering-constitution.md`
//! §"Data and security".

use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::{KernelError, Result};

macro_rules! entity_id {
    ($name:ident, $error_variant:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Ulid);

        impl $name {
            /// Generate a new identifier using the current time and a random
            /// component. The application layer is responsible for ensuring
            /// server-monotonic generation at a single allocation point.
            #[must_use]
            pub fn new() -> Self {
                Self(Ulid::new())
            }

            /// Create from a raw [`Ulid`].
            #[must_use]
            pub fn from_ulid(ulid: Ulid) -> Self {
                Self(ulid)
            }

            /// Return the underlying [`Ulid`].
            #[must_use]
            pub const fn as_ulid(&self) -> Ulid {
                self.0
            }

            /// Parse from a ULID string.
            ///
            /// # Errors
            /// Returns [`KernelError`] if the string is not a valid ULID.
            pub fn parse(s: &str) -> Result<Self> {
                Ulid::from_string(s)
                    .map(Self)
                    .map_err(|_| KernelError::$error_variant(s.to_owned()))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = KernelError;
            fn from_str(s: &str) -> Result<Self> {
                Self::parse(s)
            }
        }
    };
}

entity_id!(
    NodeId,
    InvalidNodeId,
    "Strong-typed identifier for a node in the unified node pool."
);

entity_id!(
    TagId,
    InvalidTagId,
    "Strong-typed identifier for a user-defined tag."
);

entity_id!(
    UserId,
    InvalidUserId,
    "Strong-typed identifier for a user account."
);

entity_id!(
    SessionId,
    InvalidSessionId,
    "Strong-typed identifier for a login session. \
     Not a session token — ULIDs identify entities, not secrets."
);

entity_id!(
    AuditLogId,
    InvalidAuditLogId,
    "Strong-typed identifier for an audit log entry."
);

entity_id!(
    OutboxEventId,
    InvalidOutboxEventId,
    "Strong-typed identifier for an outbox event."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_roundtrip() {
        let id = NodeId::new();
        let s = id.to_string();
        let parsed = NodeId::parse(&s).expect("parse should succeed");
        assert_eq!(id, parsed);
    }

    #[test]
    fn node_id_parse_invalid() {
        assert!(NodeId::parse("not-a-ulid").is_err());
    }

    #[test]
    fn node_id_lexical_ordering() {
        // Construct ULIDs with explicit timestamps to avoid wall-clock
        // flakiness. ULID lexical order follows (timestamp_ms, random); the
        // first 10 chars encode the 48-bit ms timestamp in Crockford base32.
        let a =
            NodeId::from_ulid(Ulid::from_string("00000000000000000000000000").expect("valid ULID"));
        let b =
            NodeId::from_ulid(Ulid::from_string("00000000010000000000000000").expect("valid ULID"));
        assert!(
            a < b,
            "ULIDs with later timestamps should sort after earlier ones"
        );
    }

    #[test]
    fn tag_id_roundtrip() {
        let id = TagId::new();
        let s = id.to_string();
        let parsed = TagId::parse(&s).expect("parse should succeed");
        assert_eq!(id, parsed);
    }

    #[test]
    fn node_id_serializes_as_string() {
        let id = NodeId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        assert!(json.starts_with('"'));
        assert!(json.ends_with('"'));
        let parsed: NodeId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, parsed);
    }
}
