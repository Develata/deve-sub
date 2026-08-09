-- Migration 0012: Probes (M7 Slice 1)
--
-- Adds tables for external probe sources (Nezha, DStatus, Komari), latency
-- records, and probe runs. See
-- docs/plan/milestones/M7-probes-and-detection.md.
--
-- probe_sources: external monitoring panels configured as traffic data
-- sources. auth_config and last_counter_snapshot are XChaCha20-Poly1305
-- encrypted blobs (constitution §157-158). kind discriminator:
--   N = Nezha, D = DStatus, K = Komari
--
-- latency_records: append-only per-node latency measurements. probe_type:
--   T = TcpConnect, Q = QuicHandshake, R = RealProxy
-- error_class (nullable, NULL = success):
--   R = Refused, D = DnsFailed, T = Timeout, L = TlsFailed, Q = QuicFailed,
--   O = Ok
-- rtt_ms is nullable: NULL means no response (NODE-014: no fake latency,
-- node not auto-disabled).
--
-- probe_runs: batch latency probing jobs. status:
--   P = Pending, R = Running, C = Completed, X = Cancelled, F = Failed
-- results is a JSON array of {node_id, rtt_ms, error_class, skipped}.

CREATE TABLE probe_sources (
    id                     TEXT    PRIMARY KEY,
    kind                   TEXT    NOT NULL CHECK (kind IN ('N', 'D', 'K')),
    name                   TEXT    NOT NULL UNIQUE,
    endpoint_url           TEXT    NOT NULL,
    auth_config            TEXT    NOT NULL DEFAULT '',
    subscription_id        TEXT    REFERENCES subscriptions(id) ON DELETE SET NULL,
    enabled                INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_sync_at           TEXT,
    last_sync_status       TEXT,
    last_sync_status_kind  TEXT    CHECK (last_sync_status_kind IN ('Ok', 'Failed', 'Stale')),
    last_counter_snapshot  TEXT,
    created_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_probe_sources_kind ON probe_sources(kind);
CREATE INDEX idx_probe_sources_subscription ON probe_sources(subscription_id);

CREATE TABLE probe_runs (
    id           TEXT    PRIMARY KEY,
    probe_type   TEXT    NOT NULL CHECK (probe_type IN ('T', 'Q', 'R')),
    node_ids     TEXT    NOT NULL,  -- JSON array of ULID strings
    status       TEXT    NOT NULL DEFAULT 'P' CHECK (status IN ('P', 'R', 'C', 'X', 'F')),
    results      TEXT    NOT NULL DEFAULT '[]',  -- JSON array
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    completed_at TEXT
);

CREATE INDEX idx_probe_runs_status ON probe_runs(status);

CREATE TABLE latency_records (
    id           TEXT    PRIMARY KEY,
    run_id       TEXT    NOT NULL REFERENCES probe_runs(id) ON DELETE CASCADE,
    node_id      TEXT    NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    probe_type   TEXT    NOT NULL CHECK (probe_type IN ('T', 'Q', 'R')),
    rtt_ms       INTEGER,  -- NULL = no response (NODE-014)
    error_class  TEXT    CHECK (error_class IN ('R', 'D', 'T', 'L', 'Q', 'O')),
    measured_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_latency_records_node ON latency_records(node_id, measured_at DESC);
CREATE INDEX idx_latency_records_run ON latency_records(run_id);
