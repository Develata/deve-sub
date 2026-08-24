-- Migration 0023: Add missing covering indexes (R3-25, R3-26)
--
-- Two query paths full-scanned because no index covered their predicate:
--
-- R3-25: reconcile and import_nodes look up a missing node by
-- `identity_fingerprint = ? AND missing_from_source = 1` (one lookup per
-- parsed entry inside the write transaction). Migration 0017 added a
-- partial UNIQUE index on `identity_fingerprint WHERE missing_from_source = 0
-- AND identity_fingerprint != ''`, but the missing-node lookup uses the
-- opposite filter (`missing_from_source = 1`), so it had no index and
-- full-scanned the nodes table per entry — O(entries x pool) per refresh.
--
-- R3-26: get_probe_traffic_attributions filters `subscription_traffic` by
-- `source_kind = 'P'` (Probe) with no index on `source_kind`. Migration
-- 0011 indexed `subscription_id` only. A probe-attribution query
-- full-scanned the traffic table. Index `(source_kind, recorded_at)` so
-- the probe-attribution filter (and any time-bounded variant) is indexed.
--
-- Pre-release: no tagged release; adding indexes is safe (no data
-- migration, no existing query changes). Both indexes are partial or
-- composite to cover the actual predicates without bloating the index.

-- R3-25: cover the missing-node identity lookup used in reconcile + import.
CREATE INDEX idx_nodes_missing_fingerprint
    ON nodes(identity_fingerprint)
    WHERE missing_from_source = 1;

-- R3-26: cover the probe-attribution filter (source_kind = 'P') and
-- time-bounded variants that also restrict by recorded_at.
CREATE INDEX idx_subscription_traffic_source_kind_recorded_at
    ON subscription_traffic(source_kind, recorded_at);
