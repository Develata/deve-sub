-- Migration 0017: Node identity fingerprint (B-12)
--
-- The dedup key changes from (protocol_kind, host, port) to a keyed HMAC
-- fingerprint over the canonical node identity (protocol, endpoint,
-- authentication, config, tls, transport, obfuscation). This prevents
-- distinct-credential nodes at the same host:port from being collapsed
-- into one pool entry (NODE-003, B-12).
--
-- Pre-release: no tagged release has been published. Existing rows get
-- the empty-string default fingerprint; they will be treated as unique
-- until re-imported (acceptable pre-release — there is no production
-- data to preserve). The old (protocol_kind, host, port) unique index
-- is dropped and replaced with one on identity_fingerprint.

ALTER TABLE nodes ADD COLUMN identity_fingerprint TEXT NOT NULL DEFAULT '';

DROP INDEX IF EXISTS idx_nodes_dedup;

-- WHY (P0-02): the partial unique index excludes rows with empty
-- fingerprints (identity_fingerprint != ''). Without this guard, a
-- populated DB upgraded through this migration would fail: all existing
-- rows receive the '' default, and two or more non-missing nodes would
-- violate a unique constraint on ''. Excluding '' lets legacy rows
-- coexist until re-imported with a real fingerprint; new nodes with
-- actual fingerprints are still uniquely constrained.
CREATE UNIQUE INDEX idx_nodes_dedup ON nodes(identity_fingerprint)
    WHERE missing_from_source = 0 AND identity_fingerprint != '';
