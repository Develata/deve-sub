-- Prevent duplicate/lost deltas from concurrent probe sync or source edits.
ALTER TABLE probe_sources ADD COLUMN revision INTEGER NOT NULL DEFAULT 0
    CHECK (typeof(revision) = 'integer' AND revision >= 0);
