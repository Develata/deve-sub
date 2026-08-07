-- Migration 0005: Add source_label column to the nodes table.
--
-- WHY: manually-imported nodes have no node_source_bindings row, so the
-- source_label derived from the bindings JOIN is always NULL. Persisting
-- source_label directly on the node ensures manual imports show "manual"
-- rather than an empty string (NODE-001). The read path uses
-- COALESCE(NULLIF(n.source_label, ''), <bindings subquery>) so existing
-- source-bound nodes (whose source_label defaults to '') continue to
-- resolve their label from the bindings JOIN.
--
-- See docs/plan/milestones/M4-sources-and-node-pool.md Slice 3.

ALTER TABLE nodes ADD COLUMN source_label TEXT NOT NULL DEFAULT '';
