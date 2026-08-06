-- Migration 0004: Sources, node pool, overrides, bindings, tags
--
-- Creates the source lifecycle tables (sources, source_snapshots,
-- source_items), the unified node pool (nodes, node_overrides,
-- node_source_bindings), and tag tables (tags, node_tags).
-- See docs/data-model/core-er.md for the entity model and
-- docs/plan/milestones/M4-sources-and-node-pool.md for the milestone
-- blueprint.

-- Sources: subscription URLs that are periodically fetched and parsed.
CREATE TABLE sources (
    id                  TEXT    PRIMARY KEY,
    name                TEXT    NOT NULL UNIQUE,
    source_type         TEXT    NOT NULL DEFAULT 'auto',
    url                 TEXT    NOT NULL,
    http_method         TEXT    NOT NULL DEFAULT 'GET',
    headers_encrypted   TEXT,
    auto_update         INTEGER NOT NULL DEFAULT 0 CHECK (auto_update IN (0, 1)),
    update_interval_secs INTEGER NOT NULL DEFAULT 3600,
    enabled             INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    keep_on_fail        INTEGER NOT NULL DEFAULT 1 CHECK (keep_on_fail IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Source snapshots: point-in-time record of a source refresh.
-- Only one snapshot per source is active at a time (is_active = 1).
CREATE TABLE source_snapshots (
    id          TEXT    PRIMARY KEY,
    source_id   TEXT    NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    fetched_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    etag        TEXT,
    node_count  INTEGER NOT NULL DEFAULT 0,
    is_active   INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1))
);

CREATE INDEX idx_snapshots_source_active ON source_snapshots(source_id, is_active);
CREATE INDEX idx_snapshots_source_version ON source_snapshots(source_id, version);

-- WHY: partial unique index guarantees at most one active snapshot per source
-- at the DB level, complementing the deactivate-then-insert transaction in
-- SourceSnapshotRepository. Defense-in-depth against a future code path that
-- might insert an active snapshot without going through that method.
CREATE UNIQUE INDEX idx_snapshots_single_active ON source_snapshots(source_id) WHERE is_active = 1;

-- Source items: raw URIs or entries from a snapshot, with parse status.
CREATE TABLE source_items (
    id            TEXT    PRIMARY KEY,
    snapshot_id   TEXT    NOT NULL REFERENCES source_snapshots(id) ON DELETE CASCADE,
    raw_uri       TEXT    NOT NULL,
    parse_status  TEXT    NOT NULL DEFAULT 'pending'
);

CREATE INDEX idx_source_items_snapshot ON source_items(snapshot_id);

-- Nodes: the unified node pool. Each node is deduplicated by
-- (protocol_kind, host, port). Nodes from multiple sources are linked
-- via node_source_bindings.
CREATE TABLE nodes (
    id                      TEXT    PRIMARY KEY,
    display_name            TEXT    NOT NULL DEFAULT '',
    protocol_kind           TEXT    NOT NULL,
    host                    TEXT    NOT NULL,
    port                    INTEGER NOT NULL,
    protocol_config_json    TEXT    NOT NULL DEFAULT '{}',
    authentication_json     TEXT    NOT NULL DEFAULT '{}',
    tls_json                TEXT,
    transport_json          TEXT,
    udp_capability          TEXT,
    multiplex_json          TEXT,
    obfuscation_json        TEXT,
    congestion_json         TEXT,
    region                  TEXT,
    extras_json             TEXT    NOT NULL DEFAULT '{}',
    imported_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    revision                INTEGER NOT NULL DEFAULT 0,
    status                  TEXT    NOT NULL DEFAULT 'active',
    missing_from_source     INTEGER NOT NULL DEFAULT 0 CHECK (missing_from_source IN (0, 1)),
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Dedup index: one active node per (protocol, host, port) tuple.
CREATE UNIQUE INDEX idx_nodes_dedup ON nodes(protocol_kind, host, port) WHERE missing_from_source = 0;

-- Node overrides: manual patches applied on top of the parsed node.
CREATE TABLE node_overrides (
    id              TEXT    PRIMARY KEY,
    node_id         TEXT    NOT NULL UNIQUE REFERENCES nodes(id) ON DELETE CASCADE,
    display_name    TEXT,
    region          TEXT,
    enabled         INTEGER CHECK (enabled IN (0, 1)),
    sni             TEXT,
    skip_cert_verify INTEGER CHECK (skip_cert_verify IN (0, 1)),
    fingerprint     TEXT,
    sort_order      INTEGER NOT NULL DEFAULT 0
);

-- Node-source bindings: tracks which sources contribute to each node.
CREATE TABLE node_source_bindings (
    id          TEXT    PRIMARY KEY,
    node_id     TEXT    NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    source_id   TEXT    NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    raw_uri     TEXT
);

CREATE INDEX idx_bindings_node ON node_source_bindings(node_id);
CREATE INDEX idx_bindings_source ON node_source_bindings(source_id);

-- Tags: user-defined labels for nodes.
CREATE TABLE tags (
    id      TEXT    PRIMARY KEY,
    name    TEXT    NOT NULL UNIQUE,
    color   TEXT
);

-- Node-tag junction table.
CREATE TABLE node_tags (
    node_id  TEXT    NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    tag_id   TEXT    NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (node_id, tag_id)
);

CREATE INDEX idx_node_tags_tag ON node_tags(tag_id);
