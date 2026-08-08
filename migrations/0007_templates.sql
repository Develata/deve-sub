-- Migration 0007: Templates, template versions, generation cache
--
-- Creates the V3 template lifecycle tables: templates (aggregate root),
-- template_versions (versioned spec snapshots), and generation_cache
-- (content cache keyed by generation parameters). See
-- docs/plan/milestones/M5-generator-and-v3-template.md for the milestone
-- blueprint and docs/plan/00-engineering-constitution.md §"Naming" for the
-- V3 template namespace.

-- Templates: V3 subscription template aggregate root. Each template
-- references its active version; the version history lives in
-- template_versions.
CREATE TABLE templates (
    id                  TEXT    PRIMARY KEY,
    name                TEXT    NOT NULL UNIQUE,
    description         TEXT    NOT NULL DEFAULT '',
    active_version_id   TEXT,
    active_version      INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Template versions: immutable snapshots of the template spec. Each create
-- or update inserts a new row with a monotonic version number; the previous
-- active version is deactivated in the same transaction.
CREATE TABLE template_versions (
    id              TEXT    PRIMARY KEY,
    template_id     TEXT    NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    spec_json       TEXT    NOT NULL,
    spec_yaml       TEXT    NOT NULL,
    is_active       INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_template_versions_template ON template_versions(template_id, version);
CREATE INDEX idx_template_versions_active ON template_versions(template_id) WHERE is_active = 1;

-- WHY: partial unique index guarantees at most one active version per
-- template at the DB level, complementing the deactivate-then-insert
-- transaction in TemplateVersionRepository. Defense-in-depth against a
-- future code path that might insert an active version without going
-- through that method.
CREATE UNIQUE INDEX idx_template_versions_single_active ON template_versions(template_id) WHERE is_active = 1;

-- Generation cache: stores generated subscription content keyed by a hash
-- of (template_id, template_version, profile, selection_mode,
-- selection_payload, pool_revision). See M5 blueprint §"Generation cache".
CREATE TABLE generation_cache (
    id                  TEXT    PRIMARY KEY,
    template_id         TEXT    NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    template_version    INTEGER NOT NULL,
    profile             TEXT    NOT NULL,
    selection_mode      TEXT    NOT NULL,
    selection_payload   TEXT    NOT NULL,
    pool_revision       INTEGER NOT NULL,
    cache_key           TEXT    NOT NULL UNIQUE,
    content             TEXT    NOT NULL,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX idx_generation_cache_lookup ON generation_cache(template_id, template_version, profile);
