-- Migration 0013: Node chains (M7 Slice 3)
--
-- Adds the chain_json column to the nodes table, storing each node's
-- proxy chain as a JSON array of ULID strings (the serialized form of
-- NodeChain, which is #[serde(transparent)] over Vec<NodeId>). NULL means
-- no chain (direct connection). See docs/plan/milestones/M7-probes-and-detection.md
-- and acceptance NODE-017 / NODE-018.

ALTER TABLE nodes ADD COLUMN chain_json TEXT;
