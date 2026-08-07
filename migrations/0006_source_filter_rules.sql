-- Migration 0006: Add filter_rules_json column to the sources table.
--
-- WHY: source-level include/exclude filter rules (SRC-010) let an admin
-- drop parsed nodes by protocol or region before reconcile. The rules are
-- stored as a JSON blob; NULL means no rules configured (all parsed nodes
-- pass through). See docs/plan/milestones/M4-sources-and-node-pool.md.

ALTER TABLE sources ADD COLUMN filter_rules_json TEXT;
