# 14 — Secret Encryption Migration

## Scope

This chapter defines the migration plan for encrypting sensitive fields at
rest, per ADR-0007. It covers schema changes, rollback, and the test plan.
The physical schema source of truth is `migrations/`; this chapter is the
design authority for the migration sequence.

See ADR-0007 for the envelope format (`v2:` prefix, HKDF-SHA256 subkey,
column-bound AAD) and field selection rationale.

## Authority

- Envelope format and field selection: ADR-0007
- Forward-only migration policy: `docs/plan/13-storage.md`, constraint #13
- Physical schema: `migrations/0015_secret_envelope.sql`

## Migration sequence

### Single-step migration (migration 0015)

Migration 0015 adds `_encrypted` columns and drops the corresponding
plaintext columns in one step. No dual-write window, no backfill CLI, no
deferred cleanup migration.

This is safe because no tagged release has been published — no production
database holds data that needs backfilling. A pre-release database that
still has plaintext columns is disposable; rollback is by restoring the
pre-migration backup (constraint #13).

```sql
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
```

After migration 0015, repository adapters encrypt on write and decrypt on
read using the envelope v2 API (`seal(master_key, context, plaintext)` /
`open(master_key, context, envelope)`). Reads are fail-closed: a NULL
`_encrypted` column or a missing master key produces an error, not a
silent fallback to plaintext.

### Deferred: probe adapter and TOTP encryption

Probe adapter auth and TOTP secret encryption use the raw `cipher::encrypt`/
`decrypt` API (not the versioned envelope). These predate the envelope and
are not migrated in this slice. The `v2:` prefix on envelope-protected
fields lets a future reader distinguish envelope-protected from raw-cipher
fields unambiguously. Migration is tracked separately.

## Rollback

Rollback is by restoring the pre-migration backup (constraint #13). There
is no dual-write phase, so data written after 0015 cannot be rolled back to
a pre-0015 schema — the plaintext columns no longer exist. This is
acceptable for pre-release: a failed migration means restoring the backup
and re-running.

If the master key is lost after encryption, the encrypted columns are
unrecoverable. Mitigation: the backup manifest records the key fingerprint
(HMAC-SHA256), and `allow_master_key_generation` defaults to `false` to
prevent silent key rotation on a lost key.

## Test plan

### Recovery test (constraint #13)

`apps/cli/tests/backup_restore.rs::backup004_restore_runs_forward_migrations`
covers the forward migration path (schema 13 → 15 via restore). A dedicated
`migration_0015_applies_and_schema_is_correct` test in
`migration_recovery.rs` verifies column existence and plaintext column
absence.

### Round-trip test

`crates/deve-sub-security/src/envelope.rs` tests verify the envelope v2
seal/open round-trip, wrong-key rejection, wrong-context rejection, and
`v1:` prefix rejection. `crates/deve-sub-storage-sqlite/tests/node_credential_encryption.rs`
verifies that node JSON columns are encrypted at rest and decrypted on read.

### Fail-closed test

`source_repository.rs` and `node_credential_encryption.rs` tests verify
that reading an encrypted column without a master key produces an error,
and that a NULL encrypted column with a key produces an error.

### Key fingerprint test

`backup_restore.rs::backup006_manifest_records_key_fingerprint` verifies
the manifest records the HMAC-SHA256 fingerprint.
`backup_restore.rs::backup008_restore_with_mismatched_key_refused` verifies
restore fails closed on fingerprint mismatch.

## Failure/recovery

- **Migration 0015 applied but key lost**: encrypted columns are
  unrecoverable. Restore from a pre-migration backup and re-run with a new
  key. This is why `allow_master_key_generation` defaults to `false`.
- **Master key changed after deployment**: all encrypted columns become
  unreadable. Restore from a pre-key-change backup and re-encrypt with the
  new key. There is no re-encryption CLI (pre-release; no production data
  to re-encrypt).
- **Restore with wrong key**: the fingerprint check fails closed, refusing
  the restore and leaving the existing production DB intact (DS-AUD-034).

## Canary scanning (deferred)

ADR-0007 §5 describes a canary scan that asserts zero plaintext secrets in
DB dumps, backup archives, CLI output, and trace logs. This is not yet
implemented. The redaction boundary (masked URLs, token redaction in
logging) is in place, but the end-to-end canary assertion is deferred to a
future slice.
