# 14 — Secret Encryption Migration

## Scope

This chapter defines the migration plan for encrypting sensitive fields at
rest, per ADR-0007. It covers schema changes, backfill, canary scanning,
rollback, and the test plan. The physical schema source of truth is
`migrations/`; this chapter is the design authority for the migration
sequence.

See ADR-0007 for the envelope format and field selection rationale.

## Authority

- Envelope format and field selection: ADR-0007
- Forward-only migration policy: `docs/plan/13-storage.md`, constraint #13
- Physical schema: `migrations/0015_secret_envelope.sql`

## Migration sequence

### Phase 1 — Schema addition (migration 0015)

Add `_encrypted` columns alongside existing plaintext columns. All new
application writes go to the `_encrypted` columns; reads prefer `_encrypted`
and fall back to plaintext if the encrypted column is NULL (transition
window).

```sql
-- sources
ALTER TABLE sources ADD COLUMN url_encrypted TEXT;
ALTER TABLE sources ADD COLUMN headers_encrypted_v2 TEXT;
-- source_items
ALTER TABLE source_items ADD COLUMN raw_uri_encrypted TEXT;
-- node_source_bindings
ALTER TABLE node_source_bindings ADD COLUMN raw_uri_encrypted TEXT;
-- nodes
ALTER TABLE nodes ADD COLUMN authentication_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN protocol_config_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN tls_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN transport_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN obfuscation_json_encrypted TEXT;
ALTER TABLE nodes ADD COLUMN extras_json_encrypted TEXT;
```

The `headers_encrypted` column already exists but is unused. The new
`headers_encrypted_v2` column holds the versioned envelope; the old column
is retained for one migration cycle and dropped in 0016.

### Phase 2 — Application dual-write

After migration 0015, the repository layer writes to both plaintext and
`_encrypted` columns. This ensures that a rollback to pre-0015 does not lose
data written after the migration. Reads prefer `_encrypted`, falling back
to plaintext when the encrypted column is NULL.

### Phase 3 — Backfill (CLI command)

```text
deve-sub secrets encrypt [--dry-run] [--limit N]
```

Reads plaintext columns, encrypts with the master key, writes the envelope
to `_encrypted` columns. Idempotent: rows with non-NULL `_encrypted` are
skipped. Resumable: processes in batches of `--limit` rows per table.
`--dry-run` reports counts without writing.

The command must be run after migration 0015 and before migration 0016.

### Phase 4 — Plaintext column drop (migration 0016, deferred)

After all deployments have verified encrypted-column reads and the backfill
is complete, migration 0016 drops the plaintext columns. This migration is
deferred until the next release cycle after Phase D ships.

## Canary scanning

A canary scan verifies zero plaintext secrets remain in persisted artifacts.

### Scan targets

1. **DB dump**: `sqlite3 .dump` output must not contain recognizable URI
   schemes with credentials (`vless://`, `vmess://`, `trojan://`, etc.) or
   `password` JSON keys with non-encrypted values.
2. **Backup archive**: the tar must not contain plaintext secrets; the
   manifest must include `key_fingerprint`.
3. **CLI output**: `deve-sub source list`, `deve-sub node list` output must
   not contain full URLs or credential values.
4. **Trace logs**: a test tracing subscriber captures all output and
   asserts no raw subscription tokens, URLs, or passwords appear.

### Scan tool

`deve-sub secrets scan` — scans the DB, backup, and a captured log sample
for recognizable secret patterns. Returns non-zero if any plaintext secret
is found. Used in CI and as a post-migration verification step.

Patterns scanned:

- URI schemes: `ss://`, `ssr://`, `vmess://`, `vless://`, `trojan://`,
  `hysteria2://`, `hy2://`, `tuic://`, `naive+https://`, `socks5://`,
  `http://` with credentials.
- JSON keys: `"password"`, `"uuid"`, `"secret"`, `"token"` with non-envelope
  values (values not starting with `v1:`).
- Subscription token pattern: 32+ char base64url in `/sub/` paths.

## Rollback

Rollback is by restoring the pre-migration backup (constraint #13). The
dual-write phase ensures data written after 0015 survives a rollback to
pre-0015 because the plaintext columns are still populated.

If the master key is lost after encryption, the encrypted columns are
unrecoverable. Mitigation: the backup manifest records the key fingerprint,
and `allow_master_key_generation` defaults to `false` to prevent silent key
rotation.

## Test plan

### Recovery test (constraint #13)

1. Start with a pre-0015 DB containing plaintext sources and nodes.
2. Run migration 0015.
3. Run `deve-sub secrets encrypt`.
4. Verify `_encrypted` columns are non-NULL and contain `v1:` envelopes.
5. Run `deve-sub secrets scan` — must report zero plaintext secrets.
6. Restore the pre-migration backup — must produce a working DB with
   plaintext columns intact.

### Round-trip test

1. Create a source with a URL containing credentials.
2. Read it back — the domain layer receives the original plaintext URL.
3. Inspect the DB — the `url_encrypted` column contains `v1:...`, the `url`
   column contains the plaintext (dual-write phase) or is NULL (post-0016).

### Canary test

1. Generate a DB with known secret patterns.
2. Run `deve-sub secrets encrypt`.
3. Run `deve-sub secrets scan` — must exit zero.
4. Capture `deve-sub source list` output — must not contain the full URL.
5. Capture trace logs from a subscription delivery — must not contain the
   raw token.

### Key fingerprint test

1. Create a backup.
2. Verify the manifest contains `key_fingerprint`.
3. Restore with a different master key — must warn about fingerprint
   mismatch.

## Failure/recovery

- **Backfill interrupted**: re-run `deve-sub secrets encrypt`; idempotent.
- **Master key changed after backfill**: encrypted columns are unreadable.
  Restore from pre-key-change backup and re-backfill with the new key.
- **Migration 0015 applied but backfill not run**: reads fall back to
  plaintext; the system works but secrets are not yet protected. The
  `doctor` command warns if `_encrypted` columns are NULL for rows that
  have plaintext.
