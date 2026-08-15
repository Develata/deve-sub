-- Migration 0015: Secret envelope columns (ADR-0007)
--
-- Adds _encrypted columns alongside existing plaintext columns for sensitive
-- fields. The application writes to both during the transition window
-- (dual-write). A CLI command `deve-sub secrets encrypt` backfills encrypted
-- columns from plaintext for existing rows. Reads prefer _encrypted and fall
-- back to plaintext when the encrypted column is NULL.
--
-- Plaintext columns are retained and dropped in a later migration (0016)
-- after all deployments have verified encrypted-column reads.
--
-- See docs/adr/0007-secret-envelope-and-at-rest-encryption.md and
-- docs/plan/14-secret-encryption-migration.md.

-- sources: subscription URL and custom headers
ALTER TABLE sources ADD COLUMN url_encrypted TEXT;
ALTER TABLE sources ADD COLUMN headers_encrypted_v2 TEXT;

-- source_items: raw share URIs from snapshots
ALTER TABLE source_items ADD COLUMN raw_uri_encrypted TEXT;

-- node_source_bindings: raw share URIs per binding
ALTER TABLE node_source_bindings ADD COLUMN raw_uri_encrypted TEXT;

-- nodes: credential-bearing JSON columns
ALTER TABLE nodes ADD COLUMN authentication_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN protocol_config_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN tls_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN transport_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN obfuscation_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN extras_json_encrypted TEXT;
