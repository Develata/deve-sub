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

-- WHY: partial unique index guarantees at most one active (non-missing)
-- node per identity fingerprint. Missing nodes are exempt so a node can
-- be marked missing and later reactivated without violating the
-- constraint. Multiple missing rows with the same fingerprint are
-- allowed (the dedup queries take the first).
CREATE UNIQUE INDEX idx_nodes_dedup ON nodes(identity_fingerprint) WHERE missing_from_source = 0;
