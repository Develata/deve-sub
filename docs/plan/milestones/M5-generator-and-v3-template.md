# Milestone 5 — Generator and V3 Template

## Scope

V3 template lifecycle (schema, CRUD, versioning, rollback), node selection
(dynamic and fixed-snapshot), proxy group model with chain proxy as a directed
graph, profile compatibility matrix, container-format emitters (Mihomo YAML,
sing-box JSON, Xray JSON, V2Ray JSON, Shadowrocket), generation pipeline with
cache and atomic publish. M5 delivers the `deve-sub-compatibility` crate, the
`deve-sub-template` domain module, the `deve-sub-generator` application module,
container emitters in `deve-sub-emitter`, storage adapters, and the
`/api/v1/templates/*` and `/api/v1/generate/*` REST API surface.

Subscription distribution (profile URL, short code, token auth, ETag, traffic
policy) is deferred to M6 (Subscription Distribution). Client compatibility
validation against real client binaries (OUT-001 through OUT-007) is deferred
to M6/M8; M5 generates valid format-conforming output and reports per-node
compatibility, but does not verify against live clients.

## Dependency

M3 (Protocol Engine) must be complete. The `deve-sub-protocol` parsers and
`deve-sub-emitter` URI emitters are prerequisites: the generation pipeline
emits canonical `Node` values to target formats. The `deve-sub-domain` `Node`,
`NodePoolEntry`, `ProtocolKind`, and `ProtocolConfig` models are the input to
generation.

M4 (Sources and Node Pool) must be complete. The unified node pool, node
override, and `NodePoolRepository` are prerequisites: the generator selects
nodes from the pool, applies overrides, and emits them.

M1 (Infrastructure) provides the SQLite pool, migration framework, and Axum
server. M2 (Auth) provides the `AdminUser` guard for template management
endpoints.

## Vertical slice

```text
deve-sub template add --name "default-mihomo" --target mihomo
    → creates V3 template (version 1)

POST /api/v1/templates
    → validates schema, persists template + version

POST /api/v1/templates/{id}/generate
    { profile: "mihomo", node_selection: { mode: "dynamic", filters: [...] } }
    → selects nodes from pool
    → checks compatibility against mihomo profile
    → assembles proxy groups from template spec
    → emits Mihomo YAML
    → caches by (template_version, profile, pool_revision, selection_hash)
    → returns generated YAML + compatibility report
```

## Deliverables

- Domain template module: `SubscriptionTemplate` aggregate,
  `TemplateVersion` entity, `TemplateSpec` value object (nodeSelector,
  proxyGroups, rules, dns, tun, output, targetProfiles, variables),
  `ProxyGroup` value object (name, type, members, filter, sortOrder),
  `GroupMember` (node or group reference), `ChainEdge` (directed graph edge),
  `TemplateRepository` and `TemplateVersionRepository` port traits.
- Domain generator module: `GenerationRequest`, `GenerationResult`,
  `CompatibilityReport`, `ExcludedNode`, `NodeSelection` (dynamic or fixed),
  `PoolRevision` value object.
- `deve-sub-compatibility` crate: `ProfileCapability` matrix per target
  profile (supported protocols, transports, TLS fields, chain support, group
  types, output format). `check_node` and `check_group` functions.
- `deve-sub-emitter` container module: `emit_mihomo`, `emit_singbox`,
  `emit_xray`, `emit_v2ray`, `emit_shadowrocket` functions. Each takes
  `&[Node]`, `&[ProxyGroup]`, `&[Rule]`, and profile-specific config, returns
  format-conforming string.
- Application generator module: `generate` command (select → compat check →
  assemble → emit → cache → publish), `preview` query, template CRUD
  commands, version rollback command.
- Storage adapter: `SqliteTemplateRepository`, `SqliteTemplateVersionRepository`,
  `SqliteGenerationCacheRepository`.
- Migration 0007: `templates`, `template_versions`, `generation_cache` tables.
- Server: template routes (`/api/v1/templates/*`), generation route
  (`POST /api/v1/templates/{id}/generate`), preview route
  (`POST /api/v1/templates/{id}/preview`).
- Contract DTOs: template create/update/response, generation request/response,
  compatibility report, proxy group DTO.
- CLI: `deve-sub template add/list/get/update/delete/rollback` commands.

## Slicing

M5 is delivered in five slices:

1. **V3 Template domain + schema + CRUD + versioning**: `SubscriptionTemplate`
   aggregate, `TemplateSpec` schema with YAML alias/depth/size limits (SEC-005
   parity), migration 0007, template CRUD API, CLI `template` commands,
   version history, rollback. Acceptance: GEN-001, GEN-002, GEN-003, GEN-004.
2. **Proxy group model + node selection + quick group + sort**: `ProxyGroup`
   with seven types (select, url-test, fallback, load-balance, relay, direct,
   reject), `NodeSelection` (dynamic filters / fixed node IDs + revision),
   quick-group by region/protocol/tag, drag-sort persistence, node-deletion
   reference handling. Acceptance: GEN-005, GEN-006, GEN-007, GEN-008,
   GEN-009, GEN-011.
3. **Chain proxy graph + cycle detection**: `ChainEdge` directed graph
   (node→node, node→group, group→node, group→group), DFS three-color cycle
   detection on save, cycle path reporting, dependency display. Acceptance:
   GEN-010, GEN-012.
4. **Compatibility matrix + container emitters**: `deve-sub-compatibility`
   crate with `ProfileCapability` per target, `emit_mihomo`, `emit_singbox`,
   `emit_xray`, `emit_v2ray`, `emit_shadowrocket` in `deve-sub-emitter`.
   Per-node compatibility check, exclusion report. Acceptance: GEN-013.
5. **Generation pipeline + cache + atomic publish**: `generate` command
   integrating selection → compat → assemble → emit → validate → cache →
   publish, cache key composition (template_version + profile + pool_revision
   + selection_hash), strict mode (fail on incompatible), atomic publish
   (activate new version, preserve old on failure), preview consistency.
   Acceptance: GEN-014, GEN-015, GEN-016.

## Architecture

### V3 Template schema

```yaml
apiVersion: deve-sub.io/v1
kind: SubscriptionTemplate

metadata:
  name: default-mihomo
  description: Default Mihomo template
  version: 1                  # monotonic, server-assigned

spec:
  targetProfiles: [mihomo, sing-box, xray, v2ray, shadowrocket, uri_list]
  variables: {}               # user-defined variables for rules/dns/tun
  nodeSelector:
    mode: dynamic             # dynamic | fixed
    filters: []               # NodeFilter (protocol, region, tag, source)
    nodeIds: []               # fixed mode only
    nodeRevision: 0           # fixed mode only
  proxyGroups: []             # ProxyGroup[]
  rules: []                   # Rule[]
  dns: {}                     # profile-specific DNS config
  tun: {}                     # profile-specific TUN config
  output: {}                  # profile-specific output options
```

Schema validation enforces: required fields, enum values, YAML alias depth
≤ 10, total size ≤ 1 MiB (SEC-005 parity), no script tags (constraint #10),
proxy group names unique, group member references valid.

### Proxy group model

Seven group types per spec §11.2:

| Type | Behavior | Chain |
|---|---|---|
| `select` | Manual selection | No |
| `url-test` | Lowest latency | No |
| `fallback` | First available | No |
| `load-balance` | Distributed | No |
| `relay` | Sequential chain | Yes |
| `direct` | Bypass proxy | No |
| `reject` | Block traffic | No |

`relay` groups model chain proxy edges. The full chain graph includes
node→node, node→group, group→node, group→group edges (spec §832-839). Cycle
detection runs on save via DFS three-color marking; cycles are rejected with
the full path (GEN-012).

Quick-group filters auto-populate members: by region (`region: US`), by
protocol (`protocol: trojan`), by tag (`tag: production`). Filters are
evaluated at generation time for dynamic selections, snapshot at save time
for fixed selections.

### Generation pipeline

```text
GenerationRequest
  → resolve NodeSelection
      dynamic: apply filters to pool → NodeSet
      fixed: lookup nodeIds against the live pool (node_revision is
             advisory metadata, not an enforcement point) → NodeSet
  → apply NodeOverride (merge upstream + manual)
  → check compatibility (ProfileCapability per node)
      compatible nodes → keep
      incompatible nodes → ExcludedNode (reason code)
      strict mode: any excluded → GenerationError
  → sort + dedup (by sort key, then endpoint+protocol)
  → resolve chain graph (expand relay groups, detect cycles)
  → assemble ProxyGroups (populate members, apply quick-group filters)
  → assemble template IR (nodes + groups + rules + dns + tun + output)
  → emit to target profile format (full IR for mihomo; proxy-only for
    other profiles, with a warning when groups/rules/dns/tun are present)
  → validate output (non-empty, well-formed; zero compatible nodes → error)
  → cache lookup (key = hash of request params)
      hit: return cached content
      miss: store, then publish
  → atomic publish (activate new generation, deactivate old)
  → GenerationResult { content, included, excluded, excluded_nodes, warnings }
```

### Compatibility matrix

`deve-sub-compatibility` defines `ProfileCapability` per target profile:

```text
ProfileCapability {
  profile: ProfileKind,
  supported_protocols: HashSet<ProtocolKind>,
  supported_transports: HashSet<TransportKind>,
  supported_tls_fields: HashSet<TlsField>,
  chain_support: bool,
  supported_group_types: HashSet<GroupType>,
  output_format: OutputFormat,
}
```

`check_node` returns `Ok(())` if the node's protocol, transport, and TLS
fields are all supported by the profile, or `Err(CompatibilityReason)` with a
reason code (e.g. `UNSUPPORTED_PROTOCOL`, `UNSUPPORTED_TRANSPORT`). Incompatible
nodes are excluded and reported (constraint #7: no silent dropping). Strict
mode (GEN-014) fails generation if any node is excluded.

### Generation cache

Cache key composition (spec §983-991):

```text
cache_key = hash(
  template_id,
  template_version,
  profile,
  node_selection_mode,
  selection_payload,    # filters (dynamic) or nodeIds+revision (fixed)
  pool_revision,        # dynamic mode: current pool revision
)
```

Cache hit returns stored content without regeneration. Cache miss generates,
validates, stores, then atomically publishes. On generation failure, the
previous active generation remains served (constraint #19, GEN-015).

## Failure/recovery

- Template schema validation failure: reject with field-level errors
  (GEN-002). No partial template is persisted.
- Cycle detection failure: reject template save, return cycle path (GEN-012).
  The template version is not incremented.
- Node deletion: group references to deleted nodes are marked `missing`. The
  generation report includes missing references as warnings (GEN-011). The
  admin can replace or remove the reference.
- Compatibility failure (strict mode): generation returns
  `GenerationError::IncompatibleNodes` with the exclusion report (GEN-014).
  No content is published.
- Generation failure (emitter error, validation error): the previous active
  generation remains served (GEN-015, constraint #19). The failure is logged.
- Empty compatible pool (all nodes excluded or unavailable): the pipeline
  returns `NoCompatibleNodes` before any cache mutation, preserving the
  previous active generation (GEN-015b, constraint #19).
- Migration 0007 has a recovery test (constraint #13): apply migration,
  verify schema, restore from pre-migration backup, verify rollback.
  Migrations are forward-only (`docs/plan/13-storage.md`); rollback is
  always backup-restore, never a down migration.

## Authority

- Template schema: this blueprint §"V3 Template schema"
- Proxy group model: this blueprint §"Proxy group model"
- Compatibility matrix: `docs/contracts/module-boundaries.md` +
  `deve-sub-compatibility` crate
- Architecture: `docs/plan/03-architecture.md`
- Module boundaries: `docs/contracts/module-boundaries.md`
- Protocol engine: `docs/plan/05-protocol-engine.md`
- V3 template namespace: `docs/plan/00-engineering-constitution.md` §"Naming"
- Generation: GEN-001 through GEN-016 acceptance cases

## Verification

- Template CRUD round-trip: create → list → get → update → delete.
  Acceptance: GEN-001.
- Schema validation: invalid template → field-level errors. Acceptance:
  GEN-002.
- Version history: edit creates new version, rollback restores prior.
  Acceptance: GEN-003, GEN-004.
- Node selection: dynamic includes new nodes, fixed excludes. Acceptance:
  GEN-005, GEN-006.
- Quick group: by region, by protocol. Acceptance: GEN-007, GEN-008.
- Drag sort: order persists across save/reload. Acceptance: GEN-009.
- Node deletion: references marked missing, report includes warning.
  Acceptance: GEN-011.
- Chain proxy: multi-relay group generates correctly. Acceptance: GEN-010.
- Cycle detection: cyclic graph rejected with path. Acceptance: GEN-012.
- Compatibility: incompatible nodes excluded with report. Acceptance:
  GEN-013.
- Strict mode: incompatible nodes → generation fails. Acceptance: GEN-014.
- Atomic publish: generation failure preserves old version. Acceptance:
  GEN-015.
- Preview consistency: preview output equals published output. Acceptance:
  GEN-016.
- Migration recovery test. Acceptance: DEPLOY-001 (migration subset).
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
- OpenAPI spec regenerated and up to date.
