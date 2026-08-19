-- Migration 0018: Master key fingerprint binding (B-07)
--
-- Binds the database to the master key that created/owns it. On the first
-- keyed open (after migrate), the key's fingerprint (a one-way HMAC-SHA256
-- digest — the raw key cannot be recovered from it) is recorded here. On
-- every subsequent keyed open, the loaded key's fingerprint is compared
-- against this row; a mismatch fails closed.
--
-- This prevents a CLI management command from silently generating a NEW key
-- on a host with an existing DB whose key file was lost/misconfigured: the
-- new key's fingerprint would not match, and the command would refuse to
-- proceed (DS-AUD-B07, ADR-0007 §7).
--
-- The `previous_key_fingerprint` and `previous_key_retired_at` columns are
-- reserved for a future key-rotation mechanism (current-key write +
-- multi-key read). The rotation logic itself is NOT implemented for v0.1;
-- the audit requires only that the schema fields be reserved ("至少预留
-- schema 字段"). The columns are nullable so single-key deployments are
-- unaffected.

CREATE TABLE key_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_key_fingerprint TEXT NOT NULL,
    key_epoch INTEGER NOT NULL DEFAULT 1,
    previous_key_fingerprint TEXT,
    previous_key_retired_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
