//! Shared SQLite error classification helpers.

/// SQLite extended result code for `SQLITE_CONSTRAINT_UNIQUE` — a UNIQUE
/// index or constraint rejected the write.
const SQLITE_CONSTRAINT_UNIQUE: &str = "2067";

/// True when `e` is a SQLite UNIQUE-constraint violation.
///
/// WHY: the previous detection matched the substring "UNIQUE" in the error
/// message, which is format- and content-fragile — any user-supplied value
/// or future SQLite message wording could flip the classification. The
/// extended result code (sqlx exposes it via `DatabaseError::code()`) is the
/// authoritative signal.
pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Database(db) => {
            matches!(db.code(), Some(code) if code.as_ref() == SQLITE_CONSTRAINT_UNIQUE)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_database_errors_are_not_unique_violations() {
        assert!(!is_unique_violation(&sqlx::Error::RowNotFound));
        assert!(!is_unique_violation(&sqlx::Error::PoolTimedOut));
    }
}
