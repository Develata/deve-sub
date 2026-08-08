-- Migration 0008: Pool revision tracking + active generation marker
--
-- Adds the `pool_meta` singleton table for global node-pool revision
-- tracking and the `is_active` column on `generation_cache` for atomic
-- publish. See docs/plan/milestones/M5-generator-and-v3-template.md
-- §"Generation cache" (GEN-015: atomic publish, constraint #19: preserve
-- last successful subscription version on failure).

-- Pool meta: singleton row tracking the monotonic pool revision. The
-- revision is bumped on every node pool mutation (reconcile, import, node
-- update, delete). It serves as a cache-key component so stale cache entries
-- are invalidated when the pool changes.
CREATE TABLE pool_meta (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    revision    INTEGER NOT NULL DEFAULT 0
);

INSERT INTO pool_meta (id, revision) VALUES (1, 0);

-- Generation cache: add is_active column for atomic publish semantics.
-- At most one active cache entry per (template_id, profile) — enforced by
-- a partial unique index. The activate operation deactivates the previous
-- active entry and activates the new one in a single transaction.
ALTER TABLE generation_cache ADD COLUMN is_active INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1));

CREATE UNIQUE INDEX idx_generation_cache_single_active
    ON generation_cache(template_id, profile)
    WHERE is_active = 1;
