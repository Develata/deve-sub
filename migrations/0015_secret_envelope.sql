-- Migration 0015: Secret envelope columns (ADR-0007)
--
-- Single-step at-rest encryption migration. Adds envelope v2 columns and
-- drops the plaintext columns in one step. No dual-write window.
--
-- Pre-release: no tagged release has been published, so no production
-- database holds data that needs backfilling. Rollback is by restoring
-- the pre-migration backup (constraint #13).
--
-- Envelope format: v2:{ciphertext_b64url}:{nonce_b64url} with HKDF-SHA256
-- subkey derivation and column-bound AAD. See
-- docs/adr/0007-secret-envelope-and-at-rest-encryption.md.

-- sources: subscription URL and custom headers
ALTER TABLE sources ADD COLUMN url_encrypted TEXT;
ALTER TABLE sources ADD COLUMN headers_encrypted_v2 TEXT;
ALTER TABLE sources DROP COLUMN url;
ALTER TABLE sources DROP COLUMN headers_encrypted;
ALTER TABLE sources RENAME COLUMN headers_encrypted_v2 TO headers_encrypted;

-- source_items: raw share URIs from snapshots
ALTER TABLE source_items ADD COLUMN raw_uri_encrypted TEXT;
ALTER TABLE source_items DROP COLUMN raw_uri;

-- node_source_bindings: raw share URIs per binding
ALTER TABLE node_source_bindings ADD COLUMN raw_uri_encrypted TEXT;
ALTER TABLE node_source_bindings DROP COLUMN raw_uri;

-- nodes: credential-bearing JSON columns
ALTER TABLE nodes ADD COLUMN protocol_config_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN authentication_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN tls_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN transport_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN obfuscation_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN extras_json_encrypted TEXT;
ALTER TABLE nodes DROP COLUMN protocol_config_json;
ALTER TABLE nodes DROP COLUMN authentication_json;
ALTER TABLE nodes DROP COLUMN tls_json;
ALTER TABLE nodes DROP COLUMN transport_json;
ALTER TABLE nodes DROP COLUMN obfuscation_json;
ALTER TABLE nodes DROP COLUMN extras_json;
