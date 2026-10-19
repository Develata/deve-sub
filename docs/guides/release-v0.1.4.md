# Deve Sub v0.1.4 release notes

Changes since v0.1.3, with compatibility and upgrade guidance.

## Changes

- Retain the dependency fixes merged since v0.1.3: `quinn-proto` 0.11.18,
  `rustls` 0.23.46, and removal of `serde_yaml` / `unsafe-libyaml`. The shared
  bounded YAML adapter preserves ordered template mappings and applies alias,
  depth and expansion limits before materializing values.
- Commit 2FA enrollment state and recovery codes atomically. Stale concurrent
  enable, disable or recovery-code operations cannot overwrite newer enrollment
  state.
- Detect sing-box and Xray/V2Ray JSON imports by document structure, retain
  input order across parser and storage outcomes, and preserve import drafts
  after errors. Keep document validation and the 10,000-entry input bound.
- Accept native Mihomo routing templates with `mode: rule`, an absent/null/empty
  `proxies` insertion slot, and structural `__PROXY_NODES__` group members.
  Preserve DNS policy order, rule order and authored template history. External
  proxy providers remain unsupported; provider placeholders expand to no
  members with an explicit warning. Empty groups use REJECT.
- Emit current sing-box WireGuard endpoints and supported Snell settings,
  preserve Xray Reality with implicit TCP, and reject output combinations that
  cannot preserve their protocol or security fields. See the
  [protocol/target matrix](https://github.com/Develata/deve-sub/blob/v0.1.4/docs/acceptance/subscription-protocols.md).
- Preserve the configured product title in release Web builds, distinguish
  dashboard loading/error/empty states, clarify subscription actions and focus,
  and select the currently filtered nodes after an immediate filter change.
- Retain the reviewed single-parse JSON path, owned generation/cache data and
  memoized node projections. Measurements were workload-specific and included
  resident-memory regressions; this release makes no claim of universally
  faster responses or lower memory use. The
  [bounded performance evidence](https://github.com/Develata/deve-sub/blob/v0.1.4/docs/acceptance/gates.md#2026-10-14--bounded-regression-convergence)
  records the tradeoffs.

## Compatibility and upgrade notes

Back up the database and master key before upgrading. There are **no new or
modified database migrations since v0.1.3**. This does not make a binary
rollback a database rollback.

The YAML parser migration has deliberate compatibility boundaries for source
YAML and templates. Existing stored templates may require editing before they
can generate again:

- Nonstandard URI tags and local custom tags are rejected, including tags that
  the previous loader ignored. Standard YAML namespace tags remain supported.
- Compact unquoted `?foo` in flow collections is rejected. Use `? foo` for an
  explicit entry or quote `'?foo'` for literal text. A bare `?` at end of input
  and the Tab-only `{?\t}` separator are also rejected; use an explicit value
  or a space separator. Multiline quoted scalars need indented continuations.
- Literal NEL (U+0085), LS (U+2028) and PS (U+2029) are rejected even in comments.
  Use LF for syntax and double-quoted Unicode escapes for string contents;
  edit and resave historical templates containing these literal characters.
- Omitted keys use YAML 1.2 empty-key semantics, and an alias after an anchor
  redefinition uses the latest definition. Template duplicate keys remain
  errors. Source-container projection keeps its distinct last-key-wins and
  ordinary `<<` behavior; it does not acquire template merge semantics.

See the [YAML contract](https://github.com/Develata/deve-sub/blob/v0.1.4/docs/plan/milestones/M5-generator-and-v3-template.md#clash-routing-input-and-version-recovery)
for bounds and remediation. Ordinary question marks and Tabs inside strings
or comments are unaffected by these compatibility checks.

**Generation cache v5 invalidates earlier output semantics.** Prior entries
cannot be used as cache hits, active output or last-good fallback; they remain
subject to existing retention. Generation rebuilds on demand. A subscription
that cannot regenerate under the new rules can fail instead of serving an old
cached result. Review affected templates and target formats before upgrading;
last-good fallback requires an eligible result under the current semantics.

Some formerly lossy outputs are now explicit errors. V2Ray rejects Reality;
complete ShadowTLS subscription URIs are rejected because they cannot retain
the inner configuration and authentication. Snell userkey is supported by
sing-box and retained in Canonical JSON; other output formats reject it.
Target-incompatible security fields, version-specific settings and v6 URI modes
are also rejected. sing-box WireGuard imports with distinct peer
endpoints or unsupported interface/dialer settings remain raw Unsupported input
instead of being collapsed. Strict generation
rejects incompatible nodes; lenient generation reports exclusions, and an
unsupported-only result cannot become a successful empty subscription.
Canonical JSON remains the full-fidelity typed-node representation.

For a native Web installation, update the **binary and Web assets from the
same release** with the installer. `deve-sub update` replaces only the binary;
with Web serving enabled it requires explicit `--binary-only` acceptance of
version skew. It is not a complete native Web upgrade. The installer checks
downloaded checksums over HTTPS; the binary updater verifies signed manifests
by default. See the [artifact and update contract](https://github.com/Develata/deve-sub/blob/v0.1.4/docs/contracts/release-artifacts.md).

Docker deployments can pin
`ghcr.io/develata/deve-sub:v0.1.4` for Linux amd64 or arm64. The optional `latest`
alias moves only after the versioned image is published and v0.1.4 is confirmed
as the current stable GitHub release. Pulling an image does not replace a
running container; recreate it with the existing persistent volume.

## Verification boundaries

Candidate validation belongs to the release PR and the manual release preflight
on its exact commit. The preflight runs full CI, both native architecture builds
and smoke checks, then assembles the paired Web archive, checksums, Ed25519
manifest/signature and native/Web SBOMs. A final tag repeats those gates before
publishing; a manual preflight publishes neither a release nor GHCR images.

Pinned-core configuration checks cover Mihomo 1.19.30, sing-box 1.14.3 and
Xray 26.3.27. They do not establish live proxy connectivity, production DNS
answers or GUI-client behavior. Historical UPDATE-001/002 systemd-VM and
PERF-001/002 budget cases remain `not-run`; process-isolated updater checks and
short soak runs do not promote those cases to pass.

[Full comparison](https://github.com/Develata/deve-sub/compare/v0.1.3...v0.1.4)
