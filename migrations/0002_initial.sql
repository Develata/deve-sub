-- Migration 0002: Initial schema
--
-- Creates the core infrastructure tables: users, sessions, audit_log, and
-- outbox_event. Real domain schema (sources, nodes, subscriptions, templates)
-- arrives with M2. See docs/data-model/core-er.md for the entity model and
-- docs/plan/13-storage.md for the storage policy.

-- Users: authentication and authorization identities.
CREATE TABLE users (
    id              TEXT    PRIMARY KEY,
    username        TEXT    NOT NULL UNIQUE,
    password_hash   TEXT    NOT NULL,
    role            TEXT    NOT NULL DEFAULT 'user',
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    expires_at      TEXT,
    traffic_quota   INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Sessions: authenticated user sessions with HMAC-hashed tokens.
CREATE TABLE sessions (
    id          TEXT    PRIMARY KEY,
    user_id     TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    expires_at  TEXT    NOT NULL,
    revoked     INTEGER NOT NULL DEFAULT 0 CHECK (revoked IN (0, 1))
);

CREATE INDEX idx_sessions_user_id    ON sessions(user_id);
CREATE INDEX idx_sessions_expires_at ON sessions(expires_at);

-- Audit log: append-only record of actor actions on targets.
CREATE TABLE audit_log (
    id            TEXT    PRIMARY KEY,
    actor_id      TEXT    REFERENCES users(id) ON DELETE SET NULL,
    action        TEXT    NOT NULL,
    target_type   TEXT,
    target_id     TEXT,
    details_json  TEXT,
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_audit_log_actor_id   ON audit_log(actor_id);
CREATE INDEX idx_audit_log_created_at ON audit_log(created_at);

-- Outbox event: persistent outbox for reliable event dispatch.
-- See docs/plan/00-engineering-constitution.md §20 (no full event sourcing).
CREATE TABLE outbox_event (
    id              TEXT    PRIMARY KEY,
    aggregate_type  TEXT    NOT NULL,
    aggregate_id    TEXT    NOT NULL,
    event_type      TEXT    NOT NULL,
    payload_json    TEXT    NOT NULL,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    processed_at    TEXT
);

CREATE INDEX idx_outbox_event_unprocessed ON outbox_event(processed_at) WHERE processed_at IS NULL;
CREATE INDEX idx_outbox_event_created_at  ON outbox_event(created_at);
