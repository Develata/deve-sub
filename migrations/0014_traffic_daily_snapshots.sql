-- Migration 0014: Traffic daily snapshots
--
-- Daily traffic snapshots per subscription, computed by the M10 aggregation
-- job. Each row stores the summed upload/download for one subscription on
-- one UTC day, plus a per-source-kind breakdown as JSON.
--
-- The (subscription_id, date) UNIQUE constraint makes the upsert idempotent:
-- re-running the aggregation for an already-computed day replaces the row
-- rather than duplicating it.
--
-- See docs/plan/milestones/M10-observability-and-audit.md §"Traffic daily
-- snapshot model".

CREATE TABLE traffic_daily_snapshots (
    subscription_id       TEXT    NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    date                  TEXT    NOT NULL,
    total_upload          INTEGER NOT NULL DEFAULT 0,
    total_download        INTEGER NOT NULL DEFAULT 0,
    source_breakdown_json TEXT    NOT NULL DEFAULT '{}',
    computed_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (subscription_id, date)
);

CREATE INDEX idx_traffic_daily_snapshots_date ON traffic_daily_snapshots(date);
