//! Node selection revisions keep completed requests from erasing newer intent.
#![cfg(target_family = "wasm")]

use std::collections::HashSet;

/// Page-owned selection and its monotonic local intent revision.
#[derive(Clone, Default, PartialEq)]
pub struct NodeSelection {
    ids: HashSet<String>,
    revision: u64,
}

impl NodeSelection {
    /// Read selected IDs without allowing untracked mutation.
    pub fn ids(&self) -> &HashSet<String> {
        &self.ids
    }

    /// Capture this revision with a submitted batch operation.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Replace selection in response to an explicit user intent.
    pub fn replace(&mut self, ids: HashSet<String>) {
        self.ids = ids;
        self.revision += 1;
    }

    /// Clear selection for a user action or a changed category.
    pub fn clear(&mut self) {
        self.ids.clear();
        self.revision += 1;
    }

    /// Set one checkbox; even reselection of identical IDs is newer intent.
    pub fn set(&mut self, id: String, checked: bool) {
        if checked {
            self.ids.insert(id);
        } else {
            self.ids.remove(&id);
        }
        self.revision += 1;
    }

    /// Apply row selection using the same revision path as its checkbox.
    pub fn toggle(&mut self, id: String) {
        let checked = !self.ids.contains(&id);
        self.set(id, checked);
    }

    /// Drop nodes that are no longer loaded in the active category.
    pub fn retain(&mut self, available: &HashSet<&str>) {
        let previous = self.ids.len();
        self.ids.retain(|id| available.contains(id.as_str()));
        if self.ids.len() != previous {
            self.revision += 1;
        }
    }

    /// A completed batch owns cleanup only while selection remains unchanged.
    pub fn clear_if_unchanged(&mut self, revision: u64) {
        if self.revision == revision {
            self.clear();
        }
    }
}
