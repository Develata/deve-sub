-- Explicitly reviewed recovery actions only; never infer affected node IDs.
CREATE TABLE node_security_review_batches (
    plan_sha256 TEXT PRIMARY KEY,
    expected_pool_revision INTEGER NOT NULL,
    resulting_pool_revision INTEGER NOT NULL,
    action_count INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE node_security_recovery (
    node_id TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('quarantined', 'repaired')),
    plan_sha256 TEXT NOT NULL REFERENCES node_security_review_batches(plan_sha256),
    evidence_sha256 TEXT NOT NULL,
    expected_revision INTEGER NOT NULL,
    expected_fingerprint TEXT NOT NULL,
    resulting_revision INTEGER NOT NULL,
    resulting_fingerprint TEXT NOT NULL
);
CREATE TABLE node_security_recovery_events (
    plan_sha256 TEXT NOT NULL REFERENCES node_security_review_batches(plan_sha256),
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('quarantined', 'repaired')),
    evidence_sha256 TEXT NOT NULL,
    expected_revision INTEGER NOT NULL,
    expected_fingerprint TEXT NOT NULL,
    resulting_revision INTEGER NOT NULL,
    resulting_fingerprint TEXT NOT NULL,
    PRIMARY KEY (plan_sha256, node_id)
);
