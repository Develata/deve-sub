# Entity Catalog

## Scope

This catalog registers all planned entities for Deve Sub, grouped by milestone.
The core ER diagram (`core-er.md`) covers the entities required by the product
spine; this catalog includes additional entities planned for later milestones.

The physical schema source of truth is `migrations/`. This catalog is the
conceptual registry; when they disagree, migrations prevail.

## M1 — Infrastructure

| Entity | Description |
|---|---|
| User | System user with role (admin/user), credentials, quota, and expiry. |
| Session | Authenticated session with token hash, expiry, and revocation. |
| AuditLog | Immutable record of actor actions on targets. |
| OutboxEvent | Persistent outbox for reliable event dispatch. |
| Job | Background job (source refresh, node test, generation, probe sync). |
| AppConfig | Centralized product name, logo, theme, and feature flags. |

## M2 — Authentication and Users

| Entity | Description |
|---|---|
| RecoveryCode | Single-use 2FA recovery code, stored as hash. |
| TotpSecret | TOTP shared secret, encrypted at rest, with enabled flag. |
| SubscriptionToken | CSPRNG subscription access token, stored as HMAC-SHA256 digest. |

## M3 — Protocol Engine

| Entity | Description |
|---|---|
| Node | Canonical node in the unified pool. Core entity of the protocol engine. |

Value types embedded in Node (no identity, not separately ULID-identified):

| Value type | Description |
|---|---|
| ProtocolConfig | Typed payload for P0 protocol parameters. |
| UnsupportedNode | Non-P0 or unknown-protocol node preserving raw data. |

## M4 — Sources and Node Pool

| Entity | Description |
|---|---|
| Source | Subscription source with fetch config, filters, and update schedule. |
| SourceSnapshot | Immutable point-in-time capture of a source's parsed nodes. |
| SourceItem | Individual raw item within a snapshot, with parse status. |
| NodeOverride | Human-authored override layered on upstream node raw model. |
| NodeSourceBinding | Many-to-many binding between nodes and sources. |
| Tag | User-defined tag with name and color for node grouping. |
| RegionAssignment | Region and flag assignment (manual or GeoIP-derived). |
| NodeTestResult | Latest probe result: TCP RTT, TLS/QUIC RTT, proxy RTT, UDP reachability. |

## M5 — Generator and V3 Template

| Entity | Description |
|---|---|
| Template | V3 subscription template with versioned content. |
| TemplateVersion | Immutable version of a template's YAML content. |
| Subscription | Subscription configuration: profile, template, node selection, groups, rules. |
| SubscriptionVersion | Immutable generated output version with content hash. |
| ProxyGroup | Proxy group definition (select, url-test, fallback, load-balance, relay). |
| CompatibilityProfile | Client capability profile: supported protocols, transports, TLS fields. |

## M6 — Subscription Distribution

| Entity | Description |
|---|---|
| SubscriptionToken | (refined in M6) Token with short code, rotation grace, request count. |
| TemporaryLink | Time-limited subscription access link with expiry and request count. |

## M7 — Probes and Detection

| Entity | Description |
|---|---|
| ProbeConfig | Configuration for a probe adapter (Nezha, DStatus, Komari, HTTP, manual). |
| TrafficRecord | Distinguished traffic sample: probe up/down, source up/down, manual, aggregated. |
| TrafficAggregation | Final aggregated traffic value with traceable data sources. |

## M8 — Deployment and Hardening

| Entity | Description |
|---|---|
| UpdateManifest | Signed release manifest with SHA-256 and Ed25519 signature. |
| BackupRecord | Metadata for a database backup (online backup API or VACUUM INTO). |

## Rules

- Entities are added to this catalog when their owning milestone begins.
- Each entity must have a corresponding migration before implementation.
- Entity names in code use Rust naming (PascalCase structs); database tables
  use snake_case.
- ULIDs identify all entities. ULIDs are not secrets.
