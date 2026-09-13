-- M10: the cutoff query visits only the oldest bounded batch, independent of
-- the size of recent history. Existing immutable events are preserved.
CREATE INDEX idx_audit_log_retention ON audit_log(created_at, id);
