# Milestone 7 — Probes and Detection

## Scope

Node latency probing (TCP connect RTT, QUIC handshake RTT for HY2/TUIC, real
proxy request latency), probe runner infrastructure (semaphore-bounded
concurrency, observable, cancellable, safe shutdown — constraint #20), node
chain proxy (node-level chain save, test, and cycle detection — deferred from
M4), external traffic probe adapters (Nezha, DStatus, Komari) that populate
the `TrafficSourceKind::Probe` variant reserved in M6, probe source failure
handling (preserve stale stats, mark expired), multi-source traffic aggregation
with traceable data provenance, and a dashboard API surface.

M7 delivers the `deve-sub-domain` `probe` module, the `deve-sub-application`
`probe` module, probe adapters in `deve-sub-adapters`, storage adapters, the
`/api/v1/probe-sources/*`, `/api/v1/probe-runs/*`, and `/api/v1/dashboard/*`
REST API surfaces, and `deve-sub probe` CLI commands.

Client compatibility validation against real client binaries (OUT-001 through
OUT-007) remains deferred to M8; M7 does not touch the emitter layer.

## Dependency

M6 (Subscription Distribution) must be complete. The `TrafficRecord` model,
`TrafficSourceKind::Probe` variant (reserved, not populated in M6), and
`TrafficRepository` port are prerequisites: M7 probe adapters write traffic
samples through the existing traffic infrastructure. The delivery-time quota
enforcement (M6 Slice 5) consumes M7 probe traffic data without modification.

M4 (Sources and Node Pool) must be complete. The unified node pool and
`NodePoolRepository` back node latency probing. M4 deferred NODE-012 through
NODE-018 to M7; the node pool schema includes `chain` fields for forward
compatibility (M4 §13-16), but chain validation and cycle detection ship here.

M3 (Protocol Engine) must be complete. The QUIC handshake probe for HY2/TUIC
nodes uses protocol knowledge from `deve-sub-protocol` to construct valid
handshake initial packets.

M1 (Infrastructure) provides the SQLite pool, migration framework, Axum server,
and CLI.

## Vertical slice

```text
POST /api/v1/probe-runs
    { node_ids: [...], probe_type: "tcp_connect" }
    → creates ProbeRun (status: pending)
    → runner picks up, semaphore-bounded TCP connects to each node endpoint
    → records LatencyRecord (rtt_ms, error_class) per node
    → ProbeRun status: completed
GET /api/v1/probe-runs/{id}
    → returns ProbeRun + per-node results
DELETE /api/v1/probe-runs/{id}
    → cancels pending/in-flight probes, marks cancelled
```

## Deliverables

- Domain probe module: `ProbeSource` aggregate (id, kind, name, endpoint_url,
  auth_config encrypted, subscription_id binding, enabled, last_sync_at,
  last_sync_status, created_at, updated_at), `ProbeSourceKind` enum (Nezha,
  DStatus, Komari), `LatencyRecord` value object (node_id, probe_type, rtt_ms,
  error_class, measured_at), `ProbeType` enum (TcpConnect, QuicHandshake,
  RealProxy), `ProbeRun` aggregate (id, probe_type, node_ids, status, results,
  created_at, completed_at), `ProbeRunStatus` enum (Pending, Running,
  Completed, Cancelled, Failed), `ProbeSourceRepository`, `LatencyRecordRepository`,
  `ProbeRunRepository` port traits.
- Domain node chain: `NodeChain` value object (chain: `Vec<NodeId>`), chain
  validation (non-empty, no self-reference, all nodes exist), cycle detection
  via directed graph DFS (reuses the graph algorithm pattern from M5
  `ChainGraph`, applied at node level).
- Application probe module: `create_probe_source`, `update_probe_source`,
  `delete_probe_source`, `list_probe_sources`, `sync_probe_traffic` commands;
  `start_probe_run`, `cancel_probe_run`, `get_probe_run`, `list_latency_records`
  commands/queries; `ProbeRunner` service (semaphore-bounded concurrency,
  observable progress, cancellable via `CancellationToken`, safe shutdown on
  server stop — constraint #20).
- Application traffic extension: `sync_probe_traffic` writes `TrafficRecord`
  rows with `source_kind = Probe`, populating the variant M6 reserved.
  Aggregation query reports per-source breakdown (dashboard traceability,
  terminology §125-127).
- Adapter: `NezhaProbeAdapter`, `DStatusProbeAdapter`, `KomariProbeAdapter`
  implementing the `ProbeSourceAdapter` port trait (HTTP client, response
  parsing, traffic sample extraction). `TcpConnectProbe`, `QuicHandshakeProbe`,
  `RealProxyProbe` implementing the `LatencyProbe` port trait.
- Storage adapter: `SqliteProbeSourceRepository`, `SqliteLatencyRecordRepository`,
  `SqliteProbeRunRepository`.
- Migration 0012: `probe_sources`, `latency_records`, `probe_runs`,
  `probe_run_results` tables. Migration 0013: `nodes.chain` column (JSON array
  of `NodeId`, nullable — forward compat field from M4).
- Server: probe source CRUD routes (`/api/v1/probe-sources/*`), probe run
  routes (`/api/v1/probe-runs/*`), latency query routes
  (`/api/v1/nodes/{id}/latency`), dashboard routes
  (`/api/v1/dashboard/latency`, `/api/v1/dashboard/traffic`), node chain
  update route (`PUT /api/v1/nodes/{id}/chain`).
- Contract DTOs: probe source create/update/response, probe run create/response,
  latency record, dashboard latency summary, dashboard traffic summary, node
  chain request. All DTOs and `ToSchema` derives in `deve-sub-contract` per
  ADR-0004.
- CLI: `deve-sub probe source add/list/get/update/delete/sync`,
  `deve-sub probe run start/cancel/get`, `deve-sub probe latency list`.
- Terminology: new entries in `docs/plan/01-terminology.md` for Probe Source,
  Latency Record, Probe Run, Probe Type (see Authority).
- Contracts: `docs/contracts/module-boundaries.md` amended to name the probe
  adapter Port and the dashboard surface.
- Coverage matrix: M7 row mapping `07-probes-and-detection` to `PROBE-*` and
  `NODE-012` through `NODE-018`.

## Slicing

M7 is delivered in six slices:

1. **Probe domain + runner framework + TCP/QUIC latency**: `ProbeSource`,
   `LatencyRecord`, `ProbeRun` domain model, `ProbeRunner` service
   (semaphore-bounded, cancellable), TCP connect probe, QUIC handshake probe
   for HY2/TUIC, UDP no-response handling (no fake RTT, no auto-kill —
   NODE-014), migration 0012, probe run API, latency query API. Acceptance:
   NODE-012 (TCP RTT + error class), NODE-013 (QUIC handshake RTT), NODE-014
   (UDP no-response, regression).
2. **Real proxy speed test + batch cancel**: `RealProxyProbe` (connects through
   the node as a proxy, measures request RTT), batch cancel of in-flight probe
   runs (constraint #20). Acceptance: NODE-015 (runner result correctness),
   NODE-016 (batch cancel, E2E).
3. **Node chain proxy**: migration 0013 (`nodes.chain`), `NodeChain` value
   object, chain validation, cycle detection via DFS, `PUT /api/v1/nodes/{id}/chain`
   route. Acceptance: NODE-017 (chain save + test), NODE-018 (cycle rejection,
   regression).
4. **Nezha traffic adapter**: `ProbeSourceAdapter` port trait, `NezhaProbeAdapter`
   (HTTP client, API token auth, server traffic sync), `sync_probe_traffic`
   command writing `TrafficRecord` (source_kind = Probe), probe source CRUD
   API + CLI. Acceptance: PROBE-001 (Nezha traffic sync, contract).
5. **DStatus + Komari adapters**: `DStatusProbeAdapter`, `KomariProbeAdapter`
   (same port, different API). Acceptance: PROBE-002 (DStatus), PROBE-003
   (Komari).
6. **Failure handling + aggregation + dashboard**: probe source failure
   (preserve stale stats, mark `last_sync_status = Failed`, `expired` flag —
   PROBE-004), multi-source traffic aggregation query with per-source breakdown
   (dashboard traceability — PROBE-005), dashboard API
   (`/api/v1/dashboard/latency`, `/api/v1/dashboard/traffic`). Acceptance:
   PROBE-004 (failure handling), PROBE-005 (aggregation traceability).

## Architecture

### Probe source model

```text
ProbeSource {
  id: ProbeSourceId,
  kind: ProbeSourceKind,         // Nezha, DStatus, Komari
  name: String,
  endpoint_url: String,           // panel base URL
  auth_config: EncryptedConfig,   // XChaCha20-Poly1305 encrypted API token
  subscription_id: Option<SubscriptionId>, // traffic binding
  enabled: bool,
  last_sync_at: Option<Timestamp>,
  last_sync_status: Option<SyncStatus>,  // Ok | Failed(msg) | Stale
  last_counter_snapshot: Option<EncryptedConfig>, // cumulative-counter state for Nezha/Komari delta computation
  created_at, updated_at: Timestamp,
}
```

`auth_config` is encrypted with XChaCha20-Poly1305 (constitution §157-158),
same master key as source cookies/headers. The adapter decrypts at sync time;
plaintext is never persisted or logged.

### Probe source adapter Port

```text
Port trait: ProbeSourceAdapter
  async fn sync_traffic(&self, source: &ProbeSource)
      -> Result<Vec<ProbeTrafficSample>, ProbeError>

ProbeTrafficSample {
  external_server_id: String,    // ID in the external panel
  upload: u64,                   // bytes (delta since last sync)
  download: u64,                 // bytes (delta since last sync)
  recorded_at: Timestamp,
}
```

Each adapter (Nezha, DStatus, Komari) implements this trait. The application
`sync_probe_traffic` command calls the adapter, maps `external_server_id` to
`subscription_id` via the probe source binding, and writes `TrafficRecord`
rows with `source_kind = Probe`.

The three panels differ in auth, identifier, and traffic semantics. Each
adapter normalizes these to the unified `ProbeTrafficSample` (upload/download
deltas in bytes):

| Panel | Auth | Endpoint | Identifier | Raw fields | Semantics |
|---|---|---|---|---|---|
| Nezha | `Authorization: Bearer nzp_...` (PAT) | `GET /api/v1/server` | `id` (uint64) + `uuid` | `state.net_in_transfer` / `state.net_out_transfer` | Cumulative counters — adapter stores last counter and computes delta |
| DStatus | None (anonymous public API) | `GET /api/allnode_status` | node ID string | `traffic_stats.used` / `traffic_stats.limit` | Billing-cycle used/limit quota — adapter returns current `used` as the delta (reset on billing cycle) |
| Komari | None (guest, unless site is private) | `GET /api/records/load?uuid=...&load_type=network` | `uuid` (string) | `net_total_up` / `net_total_down` | Cumulative counters — same delta computation as Nezha |

The adapter-local counter state (for Nezha/Komari cumulative models) is
persisted in the `probe_sources` table as `last_counter_snapshot` (encrypted
JSON). On sync, the adapter reads the snapshot, computes the delta, writes
`TrafficRecord` rows, and updates the snapshot. Counter resets (panel
restart, counter rollover) are detected: if the new counter is less than the
snapshot, the adapter treats the new value as the full delta (no negative
traffic).

`auth_config` stores the Nezha PAT token (DStatus/Komari need no token but
the field remains for forward compatibility and private-site session tokens).

### Latency probe model

```text
Port trait: LatencyProbe
  async fn probe(&self, node: &Node) -> Result<LatencyResult, ProbeError>

LatencyResult {
  rtt_ms: Option<u32>,           // None = no response (NODE-014)
  error_class: Option<ErrorClass>, // Timeout | Refused | DnsFailed | TlsFailed | QuicFailed | Ok
}

ProbeType: TcpConnect | QuicHandshake | RealProxy
```

- **TcpConnect**: TCP connect to `node.endpoint.host:port`, measure RTT.
  Error classification: connection refused, DNS failure, timeout.
- **QuicHandshake**: QUIC handshake for HY2/TUIC nodes only. Measures
  handshake RTT. Other UDP protocols do not get a fake "UDP ping" (spec §98,
  NODE-014). Error class: timeout, handshake failure.
- **RealProxyProbe**: connects through the node as a proxy (using its protocol
  config), sends a minimal HTTP request to a test target, measures RTT. This
  is the most accurate latency metric (spec §94).

UDP no-response (NODE-014): if a UDP/QUIC probe gets no response, the record
stores `rtt_ms = None` and `error_class = Timeout`. The node is not
automatically disabled or killed (no fake latency, no auto-death).

### Probe runner

```text
ProbeRunner {
  semaphore: bounded concurrency (configurable, default 32),
  cancellation_token: CancellationToken,
  progress: observable via ProbeRun status polling,
}

start_probe_run(node_ids, probe_type):
  → create ProbeRun (status: Pending)
  → spawn task: acquire semaphore permits, probe each node
  → update ProbeRun status: Running → Completed | Cancelled | Failed
  → write LatencyRecord per node

cancel_probe_run(run_id):
  → fire CancellationToken
  → in-flight probes abort
  → status: Cancelled
  → pending (not yet started) probes skipped
```

The runner is a built-in background job, not a separate service (constraint
#16, #17). It is observable (ProbeRun status + per-node results queryable),
cancellable (CancellationToken, NODE-016), and safely shut down on server
stop (joins all in-flight tasks with a timeout, constraint #20).

### Node chain proxy

```text
Node.chain: Option<Vec<NodeId>>   // None = no chain, direct connection

Chain validation (on save):
  1. non-empty if present
  2. no self-reference (node_id not in its own chain)
  3. all referenced nodes exist in the pool
  4. no cycle: build directed graph (node → chain targets), DFS cycle
     detection — reject if a cycle is found, return the cycle path
     (NODE-018, regression)
```

M5's `ChainGraph` handles proxy-group-level chain dependency. Node-level chain
is a separate graph: each node's `chain` field lists the nodes its traffic
traverses. The DFS cycle-detection algorithm is reused from M5's pattern but
applied to the node-level graph.

### Traffic aggregation

```text
TrafficAggregate (extended from M6):
  per source_kind:
    AirportHeader: { upload, download, records: N, last_at }
    ManualCorrection: { upload, download, records: N, last_at }
    Probe: {
      Nezha: { upload, download, last_sync, status }
      DStatus: { upload, download, last_sync, status }
      Komari: { upload, download, last_sync, status }
    }
  total: { upload, download }
  data_source: traceability summary (terminology §125-127)
```

Dashboard shows the per-source breakdown so admins can trace which probe
contributed what data (PROBE-005). When a probe source fails, its last
successful data is preserved but marked stale/expired (PROBE-004); the
dashboard shows the staleness.

## Failure/recovery

- Probe source sync failure (network error, auth error, API change): the
  adapter returns `ProbeError`. The application command sets
  `last_sync_status = Failed(msg)` and `last_sync_at` unchanged. Previous
  `TrafficRecord` rows from this source are preserved but marked stale
  (PROBE-004). The dashboard shows the failure and staleness. No traffic data
  is silently dropped or fabricated.
- Probe runner crash (process kill): on restart, `ProbeRun` rows in `Running`
  status are marked `Failed` (crash recovery). Latency records already written
  are preserved; partial results are visible.
- Probe cancellation (NODE-016): `cancel_probe_run` fires the
  `CancellationToken`. In-flight TCP/QUIC connections are dropped. Pending
  (not yet started) nodes are skipped. The `ProbeRun` status becomes
  `Cancelled` with partial results. No zombie tasks remain.
- Node chain cycle (NODE-018): the save is rejected before any database write.
  The error response includes the cycle path (e.g., `A → B → C → A`). No
  partial state.
- QUIC probe no-response (NODE-014): `rtt_ms = None`, `error_class = Timeout`.
  The node is not disabled. The dashboard shows "no response" rather than a
  fake latency value.
- Server shutdown during probe run: the runner joins all in-flight tasks with
  a configurable timeout (default 5s). Tasks that do not finish are logged and
  dropped; the `ProbeRun` is marked `Failed` on next startup.
- Migration 0012 and 0013 have recovery tests (constraint #13): apply
  migration, verify schema, restore from pre-migration backup, verify
  rollback.

## Authority

- Probe source model: this blueprint §"Probe source model"
- Latency probe model: this blueprint §"Latency probe model" + spec §89-100
- Traffic data model: `docs/plan/01-terminology.md` §"Traffic"
- Traffic record infrastructure: M6 blueprint §"Traffic and expiry policy
  framework" (M7 populates the `Probe` variant)
- Node chain: this blueprint §"Node chain proxy" + M4 blueprint §13-16
  (deferred to M7)
- Probe runner: this blueprint §"Probe runner" + constraint #20
- DTO ownership: ADR-0004 (DTOs + `ToSchema` in `deve-sub-contract`)
- Module boundaries: `docs/contracts/module-boundaries.md` (amended in M7 to
  name the probe adapter Port and dashboard surface)
- New terminology (Probe Source, Latency Record, Probe Run, Probe Type): added
  to `docs/plan/01-terminology.md` in Slice 1
- Acceptance: PROBE-001 through PROBE-005, NODE-012 through NODE-018

## Verification

- TCP latency probe: probe a known endpoint, verify RTT recorded and error
  class correct. Acceptance: NODE-012.
- QUIC latency probe: probe a HY2/TUIC endpoint, verify handshake RTT
  recorded. Acceptance: NODE-013.
- UDP no-response: probe a non-responsive UDP endpoint, verify `rtt_ms = None`,
  node not disabled. Acceptance: NODE-014 (regression).
- Real proxy speed test: probe a node via real proxy request, verify result
  correctness. Acceptance: NODE-015.
- Batch cancel: start a probe run with many nodes, cancel mid-run, verify
  in-flight aborted and pending skipped. Acceptance: NODE-016 (E2E).
- Node chain save: save a chain on a node, verify it persists and can be
  tested. Acceptance: NODE-017.
- Node chain cycle: attempt to save a cycle (A → B → A), verify rejection with
  cycle path. Acceptance: NODE-018 (regression).
- Nezha traffic sync: configure a Nezha probe source, sync, verify
  `TrafficRecord` rows with `source_kind = Probe`. Acceptance: PROBE-001.
- DStatus traffic sync: same for DStatus. Acceptance: PROBE-002.
- Komari traffic sync: same for Komari. Acceptance: PROBE-003.
- Probe source failure: point a probe source at an invalid endpoint, sync,
  verify old stats preserved and marked stale. Acceptance: PROBE-004.
- Multi-source aggregation: configure multiple probe sources, sync all,
  verify dashboard shows per-source breakdown with traceability.
  Acceptance: PROBE-005.
- Migration recovery tests. Acceptance: DEPLOY-001 (migration subset).
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
- OpenAPI spec regenerated and up to date.
