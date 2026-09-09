-- Complete lookup covers both active and missing reactivation candidates.
-- The existing partial unique index continues to enforce active dedup.
CREATE INDEX idx_nodes_identity_lookup ON nodes(identity_fingerprint, missing_from_source, id);
