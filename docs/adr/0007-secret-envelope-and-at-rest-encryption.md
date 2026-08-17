# ADR-0007: Secret Envelope and At-Rest Encryption

- **Status**: Accepted
- **Date**: 2026-08-16

## Context

The engineering constitution (§"Data and security") requires that sensitive
fields — subscription URLs, cookies, custom headers — are encrypted with
XChaCha20-Poly1305 at rest. Audit findings DS-AUD-026 through DS-AUD-035
identified that this requirement is not met: source URLs, proxy credentials,
raw share URIs, and node authentication data are all stored as plaintext in
SQLite. The backup archive is world-readable (mode 0644) and contains no key
fingerprint. The `/sub/{token}` route is logged with the raw token in the URI
path. The setup endpoint runs Argon2 before checking initialization state,
enabling unauthenticated CPU exhaustion.

The XChaCha20-Poly1305 primitive and `MasterKey` already exist in
`deve-sub-security` and are used for TOTP secrets and probe adapter auth.
This ADR extends their use to all sensitive at-rest fields.

## Decision

### 1. Secret envelope format

Define a versioned envelope: `v2:{ciphertext_b64url}:{nonce_b64url}`. The
`v2:` prefix distinguishes this from the never-released `v1` format (which
lacked AAD and HKDF subkey derivation; `v1` was superseded before any tagged
release, so no production data carries it). Each `seal` call derives a
column-bound subkey from the master key via HKDF-SHA256 with a domain tag,
then encrypts with XChaCha20-Poly1305 using AAD bound to the column context
(table + column + purpose). The column-bound AAD prevents ciphertext
cut-and-paste across columns; the HKDF subkey limits blast radius if a
per-column nonce is ever reused. The `deve-sub-security` crate exposes
`seal(master_key, context, plaintext) -> String` and
`open(master_key, context, envelope) -> Vec<u8>` as the canonical API.

Existing probe adapter and TOTP encryption (`{ct}:{nonce}` without prefix or
AAD) is **deferred** from this slice — they use raw `cipher::encrypt`/
`decrypt` directly and remain on the pre-envelope path. Migrating them to the
versioned envelope is tracked separately; the `v2:` prefix lets a future
reader distinguish envelope-protected from raw-cipher fields unambiguously.

### 2. Encrypted fields at rest

Encrypt these SQLite columns via the envelope:

| Table | Column | Sensitive content |
|---|---|---|
| `sources` | `url_encrypted` | Subscription URL (may embed credentials) |
| `sources` | `headers_encrypted` | Cookies, auth headers |
| `source_items` | `raw_uri_encrypted` | Raw share URI (embeds passwords, UUIDs) |
| `node_source_bindings` | `raw_uri_encrypted` | Raw share URI |
| `nodes` | `authentication_json_encrypted` | Passwords, UUIDs |
| `nodes` | `protocol_config_json_encrypted` | Protocol passwords (ShadowTLS, Hysteria2) |
| `nodes` | `tls_json_encrypted` | Reality public keys, certificate pins |
| `nodes` | `transport_json_encrypted` | WebSocket headers with auth |
| `nodes` | `obfuscation_json_encrypted` | Obfuscation passwords |
| `nodes` | `extras_json_encrypted` | Unknown content (defense-in-depth) |

Non-sensitive columns (`multiplex_json`, `congestion_json`, `region`,
`display_name`, `host`, `port`, `protocol_kind`) remain plaintext for query
and index efficiency.

### 3. Migration strategy

Forward-only, single-step:

1. **Migration 0015** (schema): add `_encrypted` columns and drop the
   corresponding plaintext columns in the same migration. No dual-write
   window, no backfill command.

This is safe because no tagged release has been published — no production
database holds data that needs backfilling. A pre-release database that
still has plaintext columns is disposable; rollback is by restoring the
pre-migration backup (constraint #13). Skipping the dual-write phase avoids
the complexity of a temporary read-fallback path and a deferred cleanup
migration.

### 4. Key fingerprint

Compute `key_fingerprint = HMAC-SHA256(key, "deve-sub-key-fingerprint-v1")`,
store the hex digest in the backup manifest. The fingerprint proves the
backup and the current master key are paired without exposing the key
itself. Restore **fails closed** when a fingerprint is present in the
manifest but the loaded key's fingerprint does not match — a mismatched key
would silently produce unreadable encrypted columns, so refusing the restore
is safer than warning. When the manifest has no fingerprint (backup predates
at-rest encryption) and no key is loaded, restore proceeds with a warning;
when the manifest has no fingerprint but a key is loaded, restore also
proceeds (the key is simply unused by the unencrypted backup).

### 5. Redaction boundary

- **API**: source DTOs return masked URLs (`https://example.com/***`); full
  URL only in explicit rotate/test operations with confirmation.
- **CLI**: passwords via `--password-stdin` or env, never argv; URLs masked
  in output unless `--reveal` flag is passed.
- **Logging**: a redacting `tracing` layer masks subscription tokens in URI
  paths and known secret patterns. The `/sub/{token}` route uses a custom
  span that redacts the token. Fetch errors cap body length and strip URLs.
- **Canary scan**: a test asserts DB dump, backup, CLI output, and trace
  logs contain zero recognizable URI/password/token patterns.

### 6. Setup endpoint guard (DS-AUD-035)

Add a fast-path `user_repo.count()` check before `hash_password()` in
`setup_admin`. The atomic `create_if_empty` guard remains as the TOCTOU
backstop. This prevents unauthenticated Argon2 CPU exhaustion when the
system is already initialized.

### 7. Backup/restore hardening (DS-AUD-031/032/033/034)

- Backup archive entries use mode 0600; the archive file itself is written
  via `NamedTempFile` (0600), `fsync`ed, then `persist`ed atomically.
- Restore writes to a staging file, `fsync`s the staged DB, hard-links the
  existing DB to a `.pre-restore` rollback file (keeping the production
  entry in place), atomically renames staging to the target, then `fsync`s
  the parent directory. A crash at any point leaves either the old or new
  DB at the production path — never missing.
- `allow_master_key_generation` config defaults to `false`; auto-generation
  requires explicit opt-in.
- **Process-level lifecycle lock**: an exclusive `flock` (via `fs2`, a safe
  wrapper around `flock(2)`) on the DB file, acquired at `serve` startup and
  held for the process lifetime. `restore` acquires the same lock before
  staging, guaranteeing no `serve` process is running. This complements the
  SQLite `BEGIN IMMEDIATE` probe in `backup` (which detects concurrent write
  transactions but not idle processes between transactions). The workspace
  `unsafe_code = "forbid"` policy is unaffected — `fs2` encapsulates the
  `unsafe` syscall inside its own crate boundary.

## Consequences

- All sensitive data is encrypted at rest; a database file compromise alone
  does not reveal credentials.
- Repository adapters encrypt on write and decrypt on read; the domain layer
  handles plaintext only in memory.
- List queries decrypt ~10 fields per node; XChaCha20-Poly1305 at ~1 GB/s
  keeps 10k-node decryption under 100 ms.
- Backup portability requires the same master key; the fingerprint makes
  key mismatches detectable.
- The redaction boundary adds a tracing layer but does not change the
  application's logging calls.

## Alternatives considered

1. **Encrypt entire SQLite file** (SQLCipher) — rejected: adds a C dependency,
   breaks standard SQLite tooling, and prevents `VACUUM INTO` backups.
2. **Application-level transparent encryption without versioned envelope** —
   rejected: no algorithm migration path; ambiguous plaintext vs ciphertext
   during migration.
3. **Encrypt all JSON columns including non-sensitive ones** — rejected:
   unnecessary CPU cost on list queries for columns with no secret content.
4. **Drop raw_uri retention entirely** — rejected: raw URIs are needed for
   debugging and re-parsing; encrypting at rest preserves functionality
   while protecting credentials.
