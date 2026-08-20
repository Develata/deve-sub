-- Migration 0019: Source refresh jobs — persistent job state, progress, and
-- per-source lease (B-15).
--
-- The audit (B-15) found that source refresh did not implement the
-- job/progress/cancel semantics claimed in the acceptance matrix. The
-- refresh was a synchronous command; manual and scheduler refreshes could
-- run concurrently for the same source; and version computation lacked a
-- UNIQUE constraint on (source_id, version).
--
-- This migration adds:
-- 1. `source_refresh_jobs` — persistent job rows tracking status, phase,
--    timestamps, and error message for each refresh attempt.
-- 2. A partial UNIQUE index on (source_id) WHERE status = 'R' (Running)
--    to enforce the per-source lease at the DB level: at most one Running
--    job per source. This is the lease mechanism — inserting a second
--    Running job for the same source fails with a constraint violation.
-- 3. A UNIQUE index on source_snapshots (source_id, version) to prevent
--    duplicate version numbers from concurrent refreshes.
--
-- See docs/plan/milestones/M4-sources-and-node-pool.md §"Source refresh
-- job model" (B-15).

CREATE TABLE source_refresh_jobs (
    id              TEXT    PRIMARY KEY,
    source_id       TEXT    NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    status          TEXT    NOT NULL DEFAULT 'P'
                        CHECK (status IN ('P', 'R', 'C', 'F', 'X')),
    -- P = Pending, R = Running, C = Completed, F = Failed, X = Cancelled
    phase           TEXT    NOT NULL DEFAULT 'idle'
                        CHECK (phase IN ('idle', 'fetching', 'parsing', 'enriching', 'reconciling', 'publishing')),
    started_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    finished_at     TEXT,
    error_message   TEXT,
    -- Reconcile counts populated on success for diagnostics.
    new_nodes       INTEGER NOT NULL DEFAULT 0,
    duplicate_nodes INTEGER NOT NULL DEFAULT 0,
    reactivated_nodes INTEGER NOT NULL DEFAULT 0,
    missing_nodes   INTEGER NOT NULL DEFAULT 0,
    not_modified    INTEGER NOT NULL DEFAULT 0 CHECK (not_modified IN (0, 1))
);

-- WHY: partial unique index enforces the per-source lease. At most one
-- Running job per source at the DB level — even if two callers race past
-- an application-level check, the second INSERT fails here. This is the
-- single source of truth for "same source does not refresh concurrently"
-- (SRC-003).
CREATE UNIQUE INDEX idx_refresh_jobs_lease
    ON source_refresh_jobs(source_id)
    WHERE status = 'R';

CREATE INDEX idx_refresh_jobs_source ON source_refresh_jobs(source_id);

-- WHY: prevent duplicate (source_id, version) pairs from concurrent
-- refreshes that both computed the same next version. Without this, two
-- refreshes starting from version N could both try to insert version N+1.
-- The application-level version computation (active.version + 1) is a
-- read-then-write that needs DB-level protection against the race.
CREATE UNIQUE INDEX idx_snapshots_source_version_unique
    ON source_snapshots(source_id, version);
