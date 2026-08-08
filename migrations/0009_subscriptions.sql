-- Migration 0009: Subscriptions and subscription tokens
--
-- Creates the subscription distribution tables: subscriptions (aggregate
-- root binding a template with delivery config) and subscription_tokens
-- (HMAC-SHA256 token digests for /sub/{token} delivery). See
-- docs/plan/milestones/M6-subscription-distribution.md for the milestone
-- blueprint and docs/plan/00-engineering-constitution.md §"Data and security"
-- for the token security model (CSPRNG-generated, HMAC-SHA256 digested,
-- redacted in logs).

-- Subscriptions: independent aggregate root. Binds a template (optionally
-- pinned to a specific version), carries its own node selection, and owns
-- delivery config (token, traffic limit, expiry). Template updates never
-- silently mutate an existing subscription's selection.
--
-- WHY: token_id is a logical reference to subscription_tokens(id) without a
-- FOREIGN KEY constraint. The insert order is subscriptions-then-tokens (the
-- token row references subscriptions(id) with ON DELETE CASCADE), so a FK on
-- subscriptions.token_id would fail at insert time. Application-layer
-- consistency guarantees the token row exists.
CREATE TABLE subscriptions (
    id                      TEXT    PRIMARY KEY,
    name                    TEXT    NOT NULL,
    slug                    TEXT    NOT NULL,
    owner_id                TEXT    NOT NULL REFERENCES users(id),
    template_id             TEXT    NOT NULL REFERENCES templates(id),
    template_version_pin    INTEGER,
    profile                 TEXT    NOT NULL,
    node_selection          TEXT    NOT NULL,
    traffic_limit           INTEGER,
    expires_at              TEXT,
    token_id                TEXT    NOT NULL,
    enabled                 INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (owner_id, slug)
);

CREATE INDEX idx_subscriptions_owner ON subscriptions(owner_id, id);

-- Subscription tokens: HMAC-SHA256 digests of CSPRNG-generated plaintext
-- tokens. The plaintext is never persisted; delivery lookup is by digest.
-- During rotation grace, previous_token_digest retains the prior digest and
-- rotation_grace_until marks the expiry (NULL = permanent grace).
CREATE TABLE subscription_tokens (
    id                      TEXT    PRIMARY KEY,
    subscription_id         TEXT    NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    token_digest            TEXT    NOT NULL UNIQUE,
    previous_token_digest   TEXT,
    rotation_grace_until    TEXT,
    issued_at               TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_subscription_tokens_subscription ON subscription_tokens(subscription_id);
