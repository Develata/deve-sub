//! Cursor-based pagination primitives.
//!
//! The cursor encodes `(created_at, id)` as an opaque string. Clients must not
//! depend on the internal format; it may change between versions. See
//! `docs/plan/00-engineering-constitution.md` §"Data and security".

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::{KernelError, Result};
use crate::time::Timestamp;

/// Opaque pagination cursor encoding `(created_at, id)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub created_at: Timestamp,
    pub id: Ulid,
}

impl Cursor {
    /// Encode the cursor as an opaque string.
    ///
    /// The format is `{unix_ms}:{ulid}`. This is an internal implementation
    /// detail; clients must treat the result as opaque.
    #[must_use]
    pub fn encode(&self) -> String {
        format!("{}:{}", self.created_at.unix_ms(), self.id)
    }

    /// Decode an opaque cursor string.
    ///
    /// # Errors
    /// Returns [`KernelError::InvalidCursor`] if the string is malformed.
    pub fn decode(s: &str) -> Result<Self> {
        let (ts_part, id_part) = s.split_once(':').ok_or(KernelError::InvalidCursor)?;
        let ms: i64 = ts_part.parse().map_err(|_| KernelError::InvalidCursor)?;
        let id = Ulid::from_string(id_part).map_err(|_| KernelError::InvalidCursor)?;
        let created_at = Timestamp::from_unix_ms(ms)?;
        Ok(Self { created_at, id })
    }
}

/// Pagination request with cursor-based navigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pagination {
    /// Maximum number of items to return.
    pub limit: u32,
    /// Opaque cursor from the previous page; `None` for the first page.
    pub cursor: Option<String>,
}

/// Paginated response carrying a cursor for the next page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Opaque cursor for the next page; `None` if this is the last page.
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_encode_decode_roundtrip() {
        let cursor = Cursor {
            created_at: Timestamp::from_unix_ms(1_700_000_000_000).expect("valid timestamp"),
            id: Ulid::new(),
        };
        let encoded = cursor.encode();
        let decoded = Cursor::decode(&encoded).expect("decode should succeed");
        assert_eq!(cursor, decoded);
    }

    #[test]
    fn cursor_decode_invalid() {
        assert!(Cursor::decode("garbage").is_err());
        assert!(Cursor::decode("notanumber:01HXYZ").is_err());
        assert!(Cursor::decode("12345:badulid").is_err());
    }

    #[test]
    fn page_serialization() {
        let page = Page {
            items: vec![1, 2, 3],
            next_cursor: Some("cursor123".to_owned()),
        };
        let json = serde_json::to_string(&page).expect("serialize");
        let recovered: Page<i32> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(page.items, recovered.items);
        assert_eq!(page.next_cursor, recovered.next_cursor);
    }
}
