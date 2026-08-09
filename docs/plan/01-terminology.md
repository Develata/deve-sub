# 01 — Terminology

This document defines terms that require precise meaning across the codebase.
Terms not listed here use their standard industry meaning.

## Protocols and clients

### Shadowsocks vs Shadowrocket

- **Shadowsocks**: a proxy protocol. Deve Sub parses and emits it.
- **Shadowrocket**: an Apple-platform client application. Deve Sub emits
  configuration for it.

Do not conflate the protocol with the client.

### Protocol vs input format

- **Protocol**: the wire-level proxy protocol (VLESS, VMess, Trojan,
  Shadowsocks, Hysteria2, TUIC v5, NaiveProxy, etc.).
- **Input format**: the container format of a subscription response (share URI
  list, Base64, Mihomo/Clash YAML, sing-box JSON, Xray JSON, V2Ray JSON,
  Shadowrocket share list).

A single protocol may appear in multiple input formats. Parsing separates
format detection from protocol extraction.

## Node model

### Canonical Node Model

The single normalized representation of a proxy node, independent of input
format and output target. All parsers produce it; all emitters consume it.
See ADR-0003.

### ProtocolKind

An enum with fifteen typed protocol variants plus `Unknown(String)` for
protocols not yet supported. `ProtocolKind` identifies what protocol a node
uses; it does not carry configuration.

### ProtocolConfig

A typed payload carrying protocol-specific parameters. Only the seven P0
protocols have typed `ProtocolConfig` variants in Phase 1: VLESS Reality,
Hysteria2, TUIC v5, NaiveProxy, Shadowsocks, VMess, Trojan.

### UnsupportedNode

A node whose protocol is not P0 or not recognized. The system preserves its
raw data but does not pass it to emitters or claim support for it. See
ADR-0003.

### Override

A human-authored modification layered on top of the upstream node raw model.
The effective node is the merge of upstream raw + human override. Remote
subscription updates must not overwrite overrides.

### Snapshot

An immutable point-in-time capture of a subscription source's parsed nodes.
Snapshots are versioned; the active snapshot is switched atomically. A new
snapshot that parses to zero nodes does not replace the previous active
snapshot.

### Node Revision

A monotonic version counter for the unified node pool. Changes when nodes are
added, removed, or modified. Used as a cache key component for subscription
generation.

## Security fields

### Three-state TLS verification

TLS certificate verification (`skip_cert_verify` / `allowInsecure`) uses three
states, not a boolean default:

- `None` — the parameter was not provided. The system must not silently fill
  in a default.
- `Some(false)` — explicitly secure (e.g. `allowInsecure=0`). Certificate
  verification is required.
- `Some(true)` — explicitly insecure (e.g. `allowInsecure=1`). Certificate
  verification is skipped.

The system must never auto-set `skip_cert_verify=true` for compatibility. See
ADR-0005.

## Latency and probing

### TCP Connect RTT

The round-trip time to establish a TCP connection to the node endpoint.

### TLS / QUIC Handshake RTT

The round-trip time to complete a TLS or QUIC handshake. Distinct from TCP
connect.

### Real proxy RTT

The round-trip time for a real HTTP request through the proxy. Measured by a
pluggable runner (sing-box or mihomo subprocess/sidecar).

### UDP / QUIC reachability

A boolean reachability check, not a latency metric. Arbitrary UDP ports may
not return data; timeout does not prove node failure. The system must not
generate meaningless "UDP ping" values. Hysteria2 and TUIC may measure QUIC
handshake RTT; other UDP capability is verified by the real proxy tester.

## Probes

### Probe source

An external monitoring panel (Nezha, DStatus, Komari) configured as a traffic
data source. Each probe source binds to a subscription and syncs traffic
samples (upload/download) via HTTP API. The `auth_config` (API token) is
encrypted with XChaCha20-Poly1305. On sync failure, the last successful data
is preserved and marked stale (PROBE-004).

### Latency record

A single latency measurement for one node: `probe_type` (TCP connect, QUIC
handshake, real proxy), `rtt_ms` (optional — `None` means no response),
`error_class` (timeout, refused, DNS failure, TLS/QUIC failure), and
`measured_at`. UDP no-response stores `rtt_ms = None` and does not disable the
node (NODE-014: no fake latency, no auto-kill).

### Probe run

A batch latency probing job: a set of node IDs, a probe type, a status
(pending, running, completed, cancelled, failed), and per-node results. The
probe runner executes with semaphore-bounded concurrency, is observable via
status polling, cancellable via `CancellationToken` (NODE-016), and safely
shuts down on server stop (constraint #20). The runner is a built-in
background job, not a separate service.

### Probe type

Three latency probe types: `TcpConnect` (TCP connect RTT to the node
endpoint), `QuicHandshake` (QUIC handshake RTT for HY2/TUIC nodes only — other
UDP protocols do not get a fake ping), `RealProxy` (real HTTP request through
the proxy, the most accurate latency metric).

## Traffic

### Traffic measurement

The subscription aggregator cannot measure real proxy traffic that users
consume through nodes. Traffic data must come from external probes (Nezha,
DStatus, Komari), airport response headers (`subscription-userinfo`), or
manual input. The system may stop distributing subscriptions based on probe
results but must not infer real proxy traffic from subscription download
counts.

### Traffic data model

The data model distinguishes: probe upload, probe download, source upload,
source download, manual correction, and the final aggregated value. Quota
calculations must be traceable; the dashboard must show data source.

## Architecture

### Port

An interface defining a boundary between layers in the hexagonal architecture.
Ports are defined in the domain or application layer; adapters implement them.
Dependencies point inward only: Delivery → Application → Domain → Ports →
Adapters.

### Modular monolith

A single deployable binary with clear internal module boundaries, not a
collection of microservices. Module boundaries are enforced by Ports, not by
network calls.

### Thin frontend

The web UI renders server-owned state, collects user intent, and dispatches
typed requests to `/api/v1`. No node parsing, protocol conversion,
subscription generation, compatibility judgment, security-field correction, or
permission logic in the frontend.

## Subscription distribution

### Subscription

An independent aggregate root that binds one `SubscriptionTemplate` (by id,
optionally pinned to a specific version), carries its own node-selection
configuration, and owns its delivery configuration (token, traffic limit,
expiry). Template updates never silently mutate an existing Subscription's
selection snapshot; the Subscription is regenerated on demand at delivery
time. See `docs/plan/milestones/M6-subscription-distribution.md`.

### Profile

A target output format identifier: `mihomo`, `sing-box`, `xray`, `v2ray`,
`shadowrocket`, or `uri_list`. A Subscription targets one profile; delivery
serves the cached generation for `(template, version, profile)` or generates
on demand on cache miss. The `ProfileKind` enum in `deve-sub-compatibility`
enumerates the valid values; the domain stores the profile as a kebab-case
string.

### Subscription Token

A CSPRNG-generated plaintext (at least 32 bytes, Base64URL no padding) used
to authenticate `/sub/{token}` delivery requests. Only the HMAC-SHA256 digest
is persisted; the plaintext is returned once at creation or rotation time and
never logged. ULIDs must not be used as subscription tokens. See
`docs/plan/00-engineering-constitution.md` §"Data and security".

### Short Code

A high-entropy CSPRNG-generated short string mapping to a Subscription's
delivery URL (`/s/{code}` → `/sub/{token}/{profile}`). Short codes are not
user-defined low-entropy values; probe attempts are rate-limited. A UNIQUE
constraint rejects collisions atomically.

### Temp Link

A temporary delivery URL with an explicit expiry. After expiry, the link
returns 404 (no existence leak). Temp links share the Subscription's token
semantics but carry their own shorter-lived credential.

### ETag

An HTTP entity tag returned with a subscription delivery response. A
conditional GET with a matching `If-None-Match` returns `304 Not Modified`,
avoiding regeneration and re-transfer. The ETag is derived from the generated
content or the generation cache entry.

### Delivery

The public subscription distribution surface (`/sub/{token}/{profile}` and
`/sub/{token}` for User-Agent auto-detect). Distinct from the admin REST
surface (`/api/v1/*`), the delivery handler is a thin adapter: it resolves
the token, delegates enforcement and generation to Application commands, and
contains no business rules.
