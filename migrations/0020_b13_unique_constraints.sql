-- Migration 0020: UNIQUE constraints for B-13 atomicity (template versions,
-- subscription tokens, subscription short codes).
--
-- The audit (B-13) found that `template_versions`, `subscription_tokens`, and
-- `subscription_short_codes` lacked UNIQUE constraints that the composite
-- repository methods rely on for correctness. Without these, a bug or race
-- in the application layer could silently insert duplicate rows that violate
-- the "one active version per template", "one token per subscription", and
-- "one short code per subscription" invariants.
--
-- The composite repository methods (create_with_version, update_with_version,
-- create_with_token, replace — committed in 3b5a1b3) are the sole write
-- paths and are designed to keep at most one row per logical key:
--
-- - template_versions: `update_with_version` inserts a new row with
--   `version = old.active_version + 1` (monotonic), so no two rows share
--   `(template_id, version)`.
-- - subscription_tokens: `rotate` performs an in-place UPDATE (not INSERT),
--   keeping a single token row per subscription with a stable id. The only
--   INSERT path is `create_with_token`, which runs once at subscription
--   creation.
-- - subscription_short_codes: `replace` deletes the old row before inserting
--   the new one inside one transaction, so at most one row exists per
--   subscription at any time.
--
-- These indexes replace the non-unique indexes created in migrations 0007,
-- 0009, and 0010. The partial-unique `idx_template_versions_single_active`
-- (migration 0007) remains — it enforces a complementary invariant (at most
-- one *active* version), while this index enforces uniqueness of the version
-- *number* regardless of active state.

-- template_versions: at most one row per (template_id, version). Replaces
-- the non-unique idx_template_versions_template from migration 0007.
DROP INDEX IF EXISTS idx_template_versions_template;
CREATE UNIQUE INDEX idx_template_versions_template
    ON template_versions(template_id, version);

-- subscription_tokens: at most one token row per subscription. The rotate
-- path UPDATEs in place, so this constraint never blocks legitimate rotations.
-- Replaces the non-unique idx_subscription_tokens_subscription from migration
-- 0009.
DROP INDEX IF EXISTS idx_subscription_tokens_subscription;
CREATE UNIQUE INDEX idx_subscription_tokens_subscription
    ON subscription_tokens(subscription_id);

-- subscription_short_codes: at most one short code per subscription. The
-- replace path deletes the old row before inserting the new one in one
-- transaction, so this constraint never blocks legitimate regeneration.
-- Replaces the non-unique idx_subscription_short_codes_subscription from
-- migration 0010.
DROP INDEX IF EXISTS idx_subscription_short_codes_subscription;
CREATE UNIQUE INDEX idx_subscription_short_codes_subscription
    ON subscription_short_codes(subscription_id);
