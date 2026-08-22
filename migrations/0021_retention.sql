-- Migration 0021: Retention support (review C-5)
--
-- idx_latency_records_measured: supports the global list_recent query
-- (ORDER BY measured_at DESC LIMIT n) and any measured_at-range scan
-- without a full table scan. Previously only the composite
-- (node_id, measured_at DESC) index existed, which cannot serve the
-- node-unfiltered path.
--
-- Retention policy (enforced in application/storage code, not triggers):
--   - source_snapshots: newest 10 per source, pruned in the reconcile tx
--     (cascade removes source_items).
--   - generation_cache: newest 8 inactive per (template_id, profile)
--     pruned on store; the active entry is never pruned.
--   - probe_runs: older than 30 days pruned by the daily maintenance
--     scheduler tick (cascade removes latency_records).

CREATE INDEX idx_latency_records_measured ON latency_records(measured_at DESC);
