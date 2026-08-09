-- Migration 0010: Subscription short codes and temp links
--
-- Adds the short-code and temp-link delivery surfaces (M6 Slice 3):
--   subscription_short_codes  — CSPRNG base62 public lookup keys for GET /s/{code}
--   subscription_temp_links   — revocable, expiry-bounded alternative delivery tokens
--   subscriptions.short_code_id — logical reference to the active short code row
--
-- See docs/plan/milestones/M6-subscription-distribution.md §"Slicing" Slice 3
-- and §"Token and short-code security model".

-- Short codes: CSPRNG-generated base62 strings (8–12 chars, ≥47 bits at 8).
-- Stored in the clear — they are public lookup keys, not secrets. The UNIQUE
-- constraint on `code` enables atomic conflict rejection (OUT-013); the
-- application layer retries with a fresh CSPRNG code on conflict.
CREATE TABLE subscription_short_codes (
    id              TEXT    PRIMARY KEY,
    subscription_id TEXT    NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    code            TEXT    NOT NULL UNIQUE,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_subscription_short_codes_subscription ON subscription_short_codes(subscription_id);

-- Temp links: alternative delivery tokens with mandatory expiry and revocation.
-- Like permanent delivery tokens, the plaintext is CSPRNG-generated and stored
-- only as an HMAC-SHA256 digest; the plaintext is returned once at creation.
-- Delivery via GET /sub/{temp_token} resolves the digest, checks revoked and
-- expires_at, then delegates to the standard delivery pipeline.
CREATE TABLE subscription_temp_links (
    id              TEXT    PRIMARY KEY,
    subscription_id TEXT    NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    token_digest    TEXT    NOT NULL UNIQUE,
    expires_at      TEXT    NOT NULL,
    revoked         INTEGER NOT NULL DEFAULT 0 CHECK (revoked IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_subscription_temp_links_subscription ON subscription_temp_links(subscription_id);

-- WHY: short_code_id is a logical reference to subscription_short_codes(id)
-- without a FOREIGN KEY constraint, mirroring the token_id pattern in
-- migration 0009. The insert order is subscriptions-then-short-code (the
-- short code table references subscriptions(id) with ON DELETE CASCADE), so a
-- FK on subscriptions.short_code_id would fail at insert time. Application-
-- layer consistency guarantees the short code row exists when this column is
-- non-NULL. The column is nullable: a subscription may have zero or one short
-- code at a time.
ALTER TABLE subscriptions ADD COLUMN short_code_id TEXT;
