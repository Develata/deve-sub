-- Migration 0003: Two-factor authentication (TOTP + recovery codes)
--
-- Adds TOTP secret storage (encrypted at rest with XChaCha20-Poly1305),
-- single-use recovery code hashes, and 2FA enable/last-login columns on the
-- users table. See docs/plan/milestones/M2-auth-and-users.md (Slice 4).

-- TOTP secrets: one per user, encrypted ciphertext + nonce.
-- The plaintext secret is 20 bytes; the ciphertext includes the 16-byte
-- Poly1305 authentication tag. Stored as BLOB for compactness.
CREATE TABLE totp_secrets (
    user_id           TEXT    PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret_ciphertext BLOB    NOT NULL,
    nonce             BLOB    NOT NULL,
    created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Recovery codes: single-use, stored as HMAC-SHA256 hashes.
-- The code_hash column is UNIQUE to enforce that each hash maps to at most
-- one row, preventing duplicate codes within a user's set.
CREATE TABLE recovery_codes (
    id          TEXT    PRIMARY KEY,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash   TEXT    NOT NULL UNIQUE,
    used        INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_recovery_codes_user ON recovery_codes(user_id);

-- Users: 2FA enable flag and last login timestamp.
ALTER TABLE users ADD COLUMN two_factor_enabled INTEGER NOT NULL DEFAULT 0 CHECK (two_factor_enabled IN (0, 1));
ALTER TABLE users ADD COLUMN last_login_at TEXT;
