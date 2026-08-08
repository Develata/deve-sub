# Milestone 6 — Subscription Distribution

## Scope

Subscription lifecycle (create, update, delete, token management), public
profile-URL delivery (`/sub/{token}/{profile}`), token authentication
(CSPRNG-generated, HMAC-SHA256 digested), custom short codes (high-entropy
CSPRNG, rate-limited), temporary links with expiry, ETag/304 conditional
responses, token rotation with configurable grace period (including permanent),
and traffic/expiry policy enforcement (User-level and Subscription-level
enforced independently). M6 delivers the `deve-sub-domain` `subscription`
module, the `deve-sub-application` `subscription` module, storage adapters, the
`/api/v1/subscriptions/*` admin REST surface, the `/sub/{token}/{profile}`
public delivery surface, and `deve-sub subscription` CLI commands.

`Subscription` is an independent aggregate root: it binds one `Template` (by
id + version pinning), carries its own node-selection configuration, and owns
its delivery configuration (token, short code, traffic limit, expiry). Template
changes do not silently mutate an existing Subscription's selection snapshot;
the Subscription is regenerated on demand at delivery time.

Traffic enforcement in M6 covers the policy framework plus data sources
available without probes: airport `subscription-userinfo` response headers and
manual correction input. Probe integration (Nezha, DStatus, Komari) is deferred
to M7 (Probes and Detection). M6 enforces both User-level
(`users.expires_at`, `users.traffic_quota`) and Subscription-level limits
independently at delivery time.

Client compatibility validation against real client binaries (OUT-001 through
OUT-007) remains deferred to M6/M8; M6 generates valid format-conforming output
via the M5 emitter layer and reports per-node compatibility, but does not
verify against live clients.

## Dependency

M5 (Generator and V3 Template) must be complete. The generation pipeline,
`GenerationCacheRepository`, atomic publish, and `pool_meta` revision are
prerequisites: delivery serves the cached active generation for a
`(template_id, profile)` pair, falling back to on-demand generation on cache
miss.

M4 (Sources and Node Pool) must be complete. The unified node pool and
`NodePoolRepository` back on-demand generation for Subscriptions whose
selection mode is dynamic.

M2 (Auth and Users) must be complete. The `User` aggregate carries
`expires_at` and `traffic_quota` (explicitly marked "Enforced in M6") that M6
enforces at delivery. The `AdminUser` guard protects subscription management
endpoints. The session token pattern (CSPRNG token, HMAC-SHA256 digest stored,
redacted in logs) is the established model M6 subscription tokens mirror.

M1 (Infrastructure) provides the SQLite pool, migration framework, and Axum
server. ADR-0001 assigns the public subscription endpoints to Axum.

## Vertical slice

```text
POST /api/v1/subscriptions
    { name, slug, template_id, profile, node_selection, traffic_limit, expires_at }
    → creates Subscription aggregate (version 1)
    → generates CSPRNG token (>=32 bytes, Base64URL no padding)
    → stores HMAC-SHA256 digest, redacts token in logs
    → returns Subscription + plaintext token (shown once)

GET /sub/{token}/mihomo
    → resolve token digest → Subscription
    → check User expiry/quota, Subscription expiry/quota
    → resolve (template_id, template_version, profile) in generation cache
        hit:  serve cached content + ETag
        miss: generate on demand, store (inactive), activate, serve
    → set ETag, Last-Modified, Content-Type, Content-Disposition,
      Cache-Control: private, no-cache, subscription-userinfo
    → conditional GET (If-None-Match) → 304 Not Modified
```

## Deliverables

- Domain subscription module: `Subscription` aggregate (name, slug, owner_id,
  template_id, template_version_pin, profile, node_selection, traffic_limit,
  expires_at, token_id, short_code_id, enabled, last_generation_status,
  last_successful_version, created_at, updated_at), `SubscriptionToken` value
  object (token_digest, rotation_grace_until, previous_token_digest),
  `ShortCode` value object (code, subscription_id), `TempLink` value object
  (token, expires_at), `TrafficPolicy` value object (limit, action_on_exceed),
  `ExpiryPolicy` value object (expires_at, action_on_expire),
  `SubscriptionRepository`, `SubscriptionTokenRepository`,
  `ShortCodeRepository`, `TempLinkRepository` port traits.
- Domain traffic module: `TrafficRecord` (subscription_id, source_kind,
  upload, download, recorded_at, source_ref), `TrafficSourceKind` enum
  (AirportHeader, ManualCorrection, Probe — Probe variant reserved for M7),
  `TrafficAggregate` value object (total_upload, total_download, data_source),
  `TrafficRepository` port trait.
- Application subscription module: `create_subscription`, `update_subscription`,
  `delete_subscription`, `list_subscriptions`, `get_subscription`,
  `rotate_token`, `regenerate_short_code`, `create_temp_link`,
  `revoke_temp_link` commands; `deliver_subscription` query (token →
  subscription → template → cache/generate → content + headers); traffic
  accounting (`record_traffic`, `get_traffic_summary`), `apply_manual_correction`
  command.
- Storage adapter: `SqliteSubscriptionRepository`,
  `SqliteSubscriptionTokenRepository`, `SqliteShortCodeRepository`,
  `SqliteTempLinkRepository`, `SqliteTrafficRepository`.
- Migration 0009: `subscriptions`, `subscription_tokens`,
  `subscription_short_codes`, `subscription_temp_links`, `subscription_traffic`
  tables. `subscription_tokens` stores HMAC-SHA256 digests only (no plaintext).
  `subscription_short_codes` has a UNIQUE constraint on `code` for atomic
  conflict rejection (OUT-013).
- Server: admin subscription routes (`/api/v1/subscriptions/*`), public
  delivery route (`GET /sub/{token}/{profile}` and
  `GET /sub/{token}` for User-Agent auto-detect), short-code redirect route
  (`GET /s/{code}`).
- Contract DTOs: subscription create/update/response, token rotation response,
  short code response, temp link response, traffic summary, delivery error
  responses. All DTOs and `ToSchema` derives in `deve-sub-contract` per
  ADR-0004.
- CLI: `deve-sub subscription add/list/get/update/delete/rotate-token/
  regenerate-short-code/create-temp-link/revoke-temp-link` commands.
- Terminology: new entries in `docs/plan/01-terminology.md` for Subscription,
  Profile, Short Code, Temp Link, Token, ETag, Delivery (see Authority).
- Contracts: `docs/contracts/module-boundaries.md` amended to separate the
  REST admin surface from the public-subscription delivery surface and name
  `deve-sub-contract` as the DTO owner for both.
- Coverage matrix: new M6 row in `docs/coverage-matrix.md` mapping
  `06-output-profiles` to `OUT-*`.

## Slicing

M6 is delivered in five slices:

1. **Subscription domain + CRUD + migration 0009**: `Subscription` aggregate,
   `SubscriptionToken` (CSPRNG generation, HMAC-SHA256 digest storage),
   migration 0009, subscription CRUD API, CLI `subscription` commands,
   terminology additions, contract amendments. Acceptance: AUTH-009 (token
   reset, partial — rotate command exists), SEC-009 (token log redaction,
   regression guard).
2. **Profile-URL delivery + ETag + token auth**: `deliver_subscription` query,
   `/sub/{token}/{profile}` and `/sub/{token}` routes, generation cache
   lookup with on-demand generation fallback, ETag/Last-Modified/304
   conditional responses, `subscription-userinfo` response header,
   `Cache-Control: private, no-cache`, `Content-Disposition`, 404 on bad
   token (no existence leak). Acceptance: OUT-008 (ETag/304), OUT-009 (token
   error), OUT-014 (concurrent generation, via atomic publish reuse).
3. **Short code + temp link**: high-entropy CSPRNG short code generation,
   `GET /s/{code}` redirect to `/sub/{token}/{profile}`, short-code UNIQUE
   conflict atomic rejection, temp link with expiry, short-code probe
   rate limiting. Acceptance: OUT-013 (short code conflict).
4. **Token rotation grace period**: `rotate_token` command, previous token
   digest retained with `rotation_grace_until`, both tokens valid during
   grace, grace expiry cleanup, `Option<Duration>` grace config (`None` =
   permanent). Acceptance: OUT-012 (token rotation), AUTH-009 (token reset,
   complete).
5. **Traffic/expiry policy enforcement**: `TrafficRecord` accounting,
   `subscription-userinfo` header parsing on upstream source refresh,
   `apply_manual_correction` command, delivery-time enforcement of
   User-level `expires_at`/`traffic_quota` and Subscription-level
   `traffic_limit`/`expires_at`, configurable action on exceed
   (warn / stop download / clear error; default stop download + clear HTTP
   error, no fake empty config per spec §1254). Acceptance: OUT-010 (user
   expiry), OUT-011 (traffic exceed).

## Architecture

### Subscription aggregate

```text
Subscription {
  id: SubscriptionId,
  name: String,
  slug: String,                     // URL-safe, unique per owner
  owner_id: UserId,
  template_id: TemplateId,
  template_version_pin: Option<u64>, // None = follow template active version
  profile: ProfileKind,
  node_selection: NodeSelection,     // dynamic filters or fixed nodeIds+revision
  traffic_limit: Option<u64>,        // bytes, None = unlimited
  expires_at: Option<Timestamp>,     // None = never expires
  token_id: SubscriptionTokenId,
  short_code: Option<ShortCodeId>,
  enabled: bool,
  last_generation_status: Option<GenerationStatus>,
  last_successful_version: Option<Revision>,
  created_at, updated_at: Timestamp,
}

SubscriptionToken {
  id: SubscriptionTokenId,
  subscription_id: SubscriptionId,
  token_digest: String,              // HMAC-SHA256 of plaintext, base64url
  previous_token_digest: Option<String>, // set during rotation grace
  rotation_grace_until: Option<Timestamp>, // None = permanent grace
  issued_at: Timestamp,
}
```

`template_version_pin` controls whether the Subscription follows the template's
current active version (`None`) or is pinned to a specific version (`Some(n)`).
This is the Subscription-independent-aggregate decision: the Subscription owns
its selection and version pin; Template updates never silently mutate it.

### Token and short-code security model

```text
Token generation:
  plaintext = CSPRNG(32 bytes) → Base64URL no padding
  token_digest = HMAC-SHA256(master_key, plaintext)
  store: token_digest only (plaintext never persisted)
  return plaintext once at creation/rotation time
  log: redacted (SHA-256 prefix, never full digest or plaintext)

Token verification (delivery):
  plaintext from URL path → HMAC-SHA256 → compare digest
  match: serve
  no match: 404 (no existence leak, OUT-009)

Short code generation:
  code = CSPRNG → base62(8-12 chars)   // entropy >= 47 bits at 8 chars
  retry on UNIQUE conflict (OUT-013 atomic rejection)
  rate limit: per-IP probe throttle on GET /s/{code}
```

Constitution binding (§159-165): subscription tokens are CSPRNG-generated,
stored as HMAC-SHA256 digests, redacted in logs. ULIDs identify entities only
and must not be used as subscription tokens. The master key is shared with
session-token HMAC; XChaCha20-Poly1305 is not needed for tokens (digests are
non-reversible) but applies to any stored sensitive URL/cookie/header fields.

### Delivery pipeline

```text
GET /sub/{token}[/{profile}]
  → parse plaintext token from path
  → HMAC-SHA256 → lookup by digest
      no match / disabled / deleted → 404 (no existence leak)
  → resolve Subscription
  → check enabled, User expires_at, User traffic_quota,
        Subscription expires_at, Subscription traffic_limit
      expired / exceeded → action_on_exceed (default: 403 + clear error,
        no fake empty config, spec §1254)
  → resolve profile: explicit path profile or User-Agent auto-detect
  → resolve template version: pin or template.active_version
  → generation cache lookup (template_id, template_version, profile,
        selection_mode, selection_payload, pool_revision)
      hit:  content + cached ETag
      miss: generate on demand (M5 pipeline), store inactive, activate,
            serve (constraint #19: failure preserves last good)
  → compute ETag = hash(content) or cached ETag
  → conditional GET: If-None-Match matches → 304 Not Modified (OUT-008)
  → set response headers:
      ETag, Last-Modified, Content-Type (profile-specific),
      Content-Disposition: attachment; filename="...",
      Cache-Control: private, no-cache,
      subscription-userinfo: upload=...; download=...; total=...; expire=...
  → 200 OK + body
```

The delivery handler is a thin Delivery-layer adapter: it resolves the token,
delegates enforcement and generation to Application commands/queries, and
contains no business rules (constraint #4, #6). It does not cross multiple
repositories in a hand-stitched transaction.

### Traffic and expiry policy framework

```text
TrafficRecord {
  subscription_id, source_kind: TrafficSourceKind,
  upload: u64, download: u64,
  recorded_at: Timestamp, source_ref: String,
}

TrafficSourceKind:
  AirportHeader    // parsed from upstream source subscription-userinfo
  ManualCorrection // admin input
  Probe            // reserved for M7, not populated in M6

TrafficAggregate = sum of records per subscription, grouped by source_kind
  dashboard shows data_source (traceability, terminology §127)

Enforcement (at delivery time):
  user.expired OR subscription.expired → action_on_expire
  user.traffic_exceeded OR subscription.traffic_exceeded → action_on_exceed
  actions: Warn | StopDownload | ClearError
  default: StopDownload + ClearError (HTTP 403/429 + clear message,
    no fake empty config, spec §1254)
```

M6 does not infer real proxy traffic from download counts (terminology §116-121).
Download counts may be recorded as observability data but never feed quota
enforcement. The `subscription-userinfo` response header Deve Sub emits
reflects the aggregated traffic and expiry state for the requesting client.

### Token rotation grace period

```text
rotate_token(subscription_id, grace: Option<Duration>):
  generate new plaintext + digest
  previous_token_digest = current token_digest
  rotation_grace_until = grace.map(|d| now + d)   // None = permanent
  store: new digest active, old digest retained
  return new plaintext once

Delivery during grace:
  new token digest → serve (primary)
  old token digest + grace active (grace_until is None or > now) → serve
  old token digest + grace expired (grace_until <= now) → 404

Cleanup:
  background job (observable, cancellable, constraint #20) sweeps
  expired grace tokens, removing previous_token_digest rows
```

`Option<Duration>` with `None` meaning permanent grace is the config model.
In the REST/CLI config surface, `-1` seconds or `null` maps to `None`.

## Failure/recovery

- Bad token / disabled subscription / deleted subscription: return 404 with a
  generic error body. The response must not reveal whether the token, the
  subscription, or the owner exists (OUT-009). Token lookup is by digest; no
  timing side-channel beyond constant-time digest comparison.
- User expired or traffic exceeded: enforce per `action_on_expire` /
  `action_on_exceed`. Default is `StopDownload` + clear HTTP error (403 or
  429), never a fake empty config (spec §1254, OUT-010, OUT-011). The admin
  can configure `Warn` (serve with warning header) for soft limits.
- Generation failure on delivery cache miss: the previous active generation
  remains served (constraint #19, GEN-015). The delivery handler falls back
  to the last successful version. If no prior generation exists, return 503
  with a clear error (no empty/fake config).
- Short-code UNIQUE conflict: the generator retries with a new CSPRNG code
  (OUT-013). After a bounded retry budget, return 500. The conflict is
  atomic (UNIQUE constraint rejects the duplicate); no partial state.
- Token rotation race: rotation is a single transactional update of the
  `SubscriptionToken` row. Concurrent rotations serialize; the last writer
  wins, and the previous `previous_token_digest` is replaced.
- Concurrent delivery + generation: atomic publish (M5, GEN-014/015) ensures
  all clients see either the complete old version or the complete new
  version, never a partial one (OUT-014).
- Migration 0009 has a recovery test (constraint #13): apply migration,
  verify schema, rollback via backup-restore, verify rollback.

## Authority

- Subscription aggregate model: this blueprint §"Subscription aggregate"
- Token and short-code security: this blueprint §"Token and short-code
  security model" + `docs/plan/00-engineering-constitution.md` §"Data and
  security"
- Delivery pipeline: this blueprint §"Delivery pipeline" + ADR-0001 (Axum
  serves public subscription endpoints)
- DTO ownership: ADR-0004 (DTOs + `ToSchema` in `deve-sub-contract`)
- Module boundaries: `docs/contracts/module-boundaries.md` (amended in M6
  to separate REST admin from public-sub delivery)
- Traffic data model: `docs/plan/01-terminology.md` §"Traffic"
- New terminology (Subscription, Profile, Short Code, Temp Link, Token, ETag,
  Delivery): added to `docs/plan/01-terminology.md` in Slice 1
- Generation cache and atomic publish: M5 blueprint §"Generation cache"
- Acceptance: OUT-008 through OUT-014, AUTH-009, SEC-009 (regression),
  GEN-015/016 (carried from M5)

## Verification

- Subscription CRUD round-trip: create → list → get → update → delete.
  Acceptance: AUTH-009 (token reset, command surface).
- Token creation: plaintext returned once, digest stored, logs redacted.
  Acceptance: SEC-009 (regression).
- Profile-URL delivery: `GET /sub/{token}/mihomo` returns Mihomo YAML with
  correct headers. Acceptance: OUT-008 (ETag).
- Conditional GET: `If-None-Match` matching ETag → 304. Acceptance: OUT-008.
- Bad token: `GET /sub/{bad}/mihomo` → 404, no existence leak. Acceptance:
  OUT-009.
- User-Agent auto-detect: `GET /sub/{token}` without profile infers profile
  from User-Agent. Acceptance: OUT-008 (header subset).
- Short code: `GET /s/{code}` redirects to delivery. Conflict on insert is
  retried atomically. Acceptance: OUT-013.
- Temp link: `GET /sub/{temp_token}` serves until expiry, 404 after.
  Acceptance: OUT-008 (delivery subset).
- Token rotation: rotate → old token valid during grace, 404 after grace
  expiry; `None` grace keeps old token valid permanently. Acceptance:
  OUT-012, AUTH-009.
- User expiry: expired User → delivery rejected with clear error.
  Acceptance: OUT-010.
- Traffic exceed: Subscription or User over quota → delivery rejected with
  clear error, no fake config. Acceptance: OUT-011.
- Concurrent generation: parallel deliveries during regeneration all see a
  complete version. Acceptance: OUT-014 (via M5 atomic publish).
- Migration recovery test. Acceptance: DEPLOY-001 (migration subset).
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
- OpenAPI spec regenerated and up to date (admin surface; the public
  `/sub/{token}` surface is documented but not bound to OpenAPI security
  schemes since it uses path tokens, not cookie auth).
