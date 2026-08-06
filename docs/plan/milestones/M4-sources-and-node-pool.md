# Milestone 4 — Sources and Node Pool

## Scope

Source lifecycle (CRUD, fetch, parse, snapshot, diff), node pool (dedup,
override, manual region, source binding), and SSRF guards for source
fetching. M4 delivers the `source` and `node` domain modules, the
`source` application module, a `source_repository` and `node_repository`
storage adapter, an HTTP fetcher adapter with SSRF protection, and the
`/api/v1/sources/*` and `/api/v1/nodes/*` REST API surface.

Node testing (TCP/QUIC latency, real proxy speed test — NODE-012 through
NODE-016) and chain proxy (NODE-017, NODE-018) are deferred to M7 (Probes
and Detection) where the probe runner infrastructure is built. The node
pool schema includes `chain` fields for forward compatibility, but the
chain validation and cycle-detection logic ships with M7.

## Dependency

M3 (Protocol Engine) must be complete. The `deve-sub-protocol` parsers
and `deve-sub-emitter` are prerequisites: the source refresh job parses
fetched subscription content into canonical `Node` values using
`parse_uri`, `parse_uri_list`, `parse_base64_subscription`,
`parse_mihomo_yaml`, `parse_singbox_json`, `parse_xray_json`, and
`parse_v2ray_json`. The `deve-sub-domain` `Node` model is the unified
node pool entity.

M1 (Infrastructure) provides the SQLite pool, migration framework, Axum
server, and CLI. M2 (Auth) provides the `AdminUser` guard for source
management endpoints.

## Vertical slice

```text
deve-sub source add --name "My Source" --url "https://example.com/sub"
    → creates source record

deve-sub serve
    → POST /api/v1/sources { name, url, source_type }
        → creates source (admin only)
    → POST /api/v1/sources/{id}/refresh
        → fetches URL (SSRF-checked), parses content, creates snapshot,
          inserts nodes into pool, marks diff
    → GET /api/v1/sources/{id}/snapshots
        → lists snapshots with node counts
    → GET /api/v1/nodes
        → lists unified node pool with protocol/region filters
    → PATCH /api/v1/nodes/{id}/override
        → sets manual display name, region, SNI override
```

## Deliverables

- Domain source module: `Source` aggregate, `SourceSnapshot` entity,
  `SourceItem` entity, `SourceType` enum, `SourceRepository` and
  `SourceSnapshotRepository` port traits.
- Domain node-pool module: `NodePoolEntry` (extends `Node` with pool
  metadata), `NodeOverride` entity, `NodeSourceBinding` entity,
  `NodePoolRepository` and `NodeOverrideRepository` port traits.
- Migration 0004: `sources`, `source_snapshots`, `source_items`,
  `nodes`, `node_overrides`, `node_source_bindings`, `tags`,
  `node_tags` tables.
- Application source module: `create_source`, `update_source`,
  `delete_source`, `refresh_source`, `list_sources`, `get_source`
  commands/queries; `list_snapshots` query.
- Application node-pool module: `list_nodes`, `get_node`,
  `update_override`, `batch_set_enabled`, `batch_set_tags` commands/
  queries.
- HTTP fetcher adapter: `HttpFetcher` implementing a `SubscriptionFetcher`
  port, with SSRF guard (reject localhost, private networks, DNS
  rebinding, redirect-to-internal), ETag/If-None-Match, timeout, response
  size limit, gzip/deflate decompression.
- Storage adapter: `SqliteSourceRepository`,
  `SqliteSourceSnapshotRepository`, `SqliteNodePoolRepository`,
  `SqliteNodeOverrideRepository`.
- Server: source routes (`/api/v1/sources/*`), node routes
  (`/api/v1/nodes/*`), all admin-guarded except node list (read-only for
  authenticated users).
- Contract DTOs: source create/update/response, snapshot response, node
  list response, node override request/response.
- SSRF guard: `SsrfGuard` checking resolved IPs against private ranges,
  localhost, link-local, and DNS rebinding (resolved IP != connect IP).

## Slicing

M4 is delivered in five slices:

1. **Source domain + migration + CRUD**: `Source` aggregate, migration
   0004 (sources + source_snapshots + source_items + nodes +
   node_overrides + node_source_bindings + tags + node_tags), source
   CRUD API (`POST/GET/PATCH/DELETE /api/v1/sources`), CLI `source add`
   command. Acceptance: SRC-001.
2. **Source refresh job**: HTTP fetcher adapter with SSRF guard, ETag,
   timeout, size limit, gzip. `refresh_source` command: fetch → parse →
   snapshot → node pool insert → diff. Background job infrastructure
   (observable, cancellable — constraint #20). Acceptance: SRC-002,
   SRC-003, SRC-005, SRC-006, SRC-007, SRC-008, SRC-012, SRC-014,
   SEC-001 through SEC-005.
3. **Node pool + dedup + diff**: Node pool queries with protocol/region
   filters, dedup by endpoint+protocol, diff (new/unchanged/missing),
   `missing_from_source` flag. Manual node import (paste batch, file
   import). Acceptance: NODE-001, NODE-002, NODE-003, NODE-011.
4. **Node override + manual region**: `NodeOverride` CRUD, manual region
   assignment, auto-region (IPv4/IPv6 GeoIP lookup), dual-stack domain
   detection, batch enable/disable, batch tags. Acceptance: NODE-004,
   NODE-005, NODE-006, NODE-007, NODE-008, NODE-009, NODE-010.
5. **Source filtering + multi-refresh**: Conditional fetch (If-Modified-
   Since), multiple concurrent refreshes, source filtering by
   protocol/region. Acceptance: SRC-009, SRC-010, SRC-011, SRC-013.

## Architecture

### Source refresh flow

```text
POST /api/v1/sources/{id}/refresh
    ↓
RefreshSourceCommand
    ↓
SsrfGuard.check(url)  →  resolve DNS  →  check IP ranges
    ↓
HttpFetcher.fetch(url, etag)  →  200/304/timeout/error
    ↓
parse_content(content_type, body)  →  Vec<Node>  (deve-sub-protocol)
    ↓
NodeReconciler.reconcile(source_id, nodes)
    ↓  (single transaction)
  - create SourceSnapshot (version, etag, node_count)
  - insert SourceItems (raw_uri, parse_status)
  - dedup nodes by (protocol, host, port)
  - upsert into node pool (new nodes get status=active)
  - mark missing nodes (missing_from_source=true)
  - create NodeSourceBindings
    ↓
SourceRefreshed event → outbox
```

### SSRF guard

Resolves the URL hostname to IP addresses. Rejects if any resolved IP is:
- Loopback (127.0.0.0/8, ::1)
- Private (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, fc00::/7)
- Link-local (169.254.0.0/16, fe80::/10)
- Multicast (224.0.0.0/4)
- Reserved/undocumented ranges

DNS rebinding: after SSRF check passes, the fetcher connects to the
checked IP, not re-resolving the hostname. If the HTTP client re-resolves,
the guard wraps the resolver to pin the IP.

Redirects: follow up to 3 redirects, re-checking SSRF on each redirect
target. Reject if a redirect points to an internal address (SEC-004).

### Node pool dedup

Two nodes are duplicates if they share the same `(protocol_kind, host,
port)` tuple. The first-seen node wins; subsequent duplicates are
recorded as `SourceItem` with `parse_status=duplicate` but do not create
a new pool entry. The `NodeSourceBinding` table tracks which sources
contribute to each pool node.

### Node override

Overrides are per-node patches applied on top of the parsed `Node`:
display name, region, enabled flag, SNI, skip_cert_verify, fingerprint,
sort order. The override does not mutate the original parsed node; the
effective node is `parsed_node.apply_override(override)`. Removing an
override reverts to the parsed value.

## Failure/recovery

- Source fetch failure (timeout, HTTP error, parse error): the last
  successful snapshot remains active. If `keep_on_fail` is false, the
  source is marked as errored. The job records the error in the `jobs`
  table. No partial node pool mutation occurs — the refresh is
  transactional: either the new snapshot is committed and the old one
  deactivated, or nothing changes (constraint #19: on failure, preserve
  the last successful subscription version).
- SSRF rejection: the refresh is rejected before any network connection.
  The error is recorded in the job and returned to the API caller.
- Migration 0004 has a recovery test (constraint #13): apply migration,
  verify schema, restore from pre-migration backup, verify rollback.

## Authority

- Entity model: `docs/data-model/core-er.md`
- Storage policy: `docs/plan/13-storage.md`
- Architecture: `docs/plan/03-architecture.md`
- Module boundaries: `docs/contracts/module-boundaries.md`
- Protocol engine: `docs/plan/05-protocol-engine.md`
- SSRF: SEC-001 through SEC-005 acceptance cases

## Verification

- Source CRUD round-trip: create → list → get → update → delete.
  Acceptance: SRC-001.
- Source refresh: fetch → parse → snapshot → node pool. Acceptance:
  SRC-002, SRC-005, SRC-006.
- SSRF: localhost rejected, private network rejected, DNS rebinding
  rejected, redirect-to-internal rejected, YAML bomb rejected.
  Acceptance: SEC-001 through SEC-005.
- Node pool: dedup, diff (new/unchanged/missing), filters. Acceptance:
  NODE-001, NODE-003, NODE-011.
- Node override: manual region, enable/disable, tags. Acceptance:
  NODE-004, NODE-005, NODE-006.
- Migration recovery test. Acceptance: DEPLOY-001 (migration subset).
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
- OpenAPI spec regenerated and up to date.
