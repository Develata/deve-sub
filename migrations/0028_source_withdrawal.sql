-- A source deletion revokes pre-deletion cached output, including late writers.
ALTER TABLE pool_meta ADD COLUMN withdrawal_revision INTEGER NOT NULL DEFAULT 0
    CHECK (withdrawal_revision >= 0 AND withdrawal_revision <= revision);

-- Older source deletes removed bindings but left orphaned remote nodes active.
-- Retain diagnostic rows and all independent imports; no credentials are deleted.
UPDATE pool_meta SET revision = revision + 1, withdrawal_revision = revision + 1
WHERE id = 1 AND EXISTS (
    SELECT 1 FROM nodes n WHERE n.source_label = '' AND n.missing_from_source = 0
    AND NOT EXISTS (SELECT 1 FROM node_source_bindings b WHERE b.node_id = n.id)
);
UPDATE nodes SET missing_from_source = 1, revision = revision + 1
WHERE source_label = '' AND missing_from_source = 0
AND NOT EXISTS (SELECT 1 FROM node_source_bindings b WHERE b.node_id = nodes.id);
