//! Monotonic version counter for the unified node pool and subscriptions.

use serde::{Deserialize, Serialize};

/// A monotonically increasing revision number.
///
/// Used as a cache-key component for subscription generation. The node pool
/// revision changes when nodes are added, removed, or modified. See
/// `docs/plan/01-terminology.md` §"Node Revision".
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct Revision(u64);

impl Revision {
    /// Create a revision from a raw value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw value.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }

    /// Increment the revision by one.
    pub fn increment(&mut self) {
        self.0 += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_increment() {
        let mut r = Revision::new(41);
        assert_eq!(r.value(), 41);
        r.increment();
        assert_eq!(r.value(), 42);
    }

    #[test]
    fn revision_ordering() {
        assert!(Revision::new(1) < Revision::new(2));
    }

    #[test]
    fn revision_default_is_zero() {
        assert_eq!(Revision::default().value(), 0);
    }
}
