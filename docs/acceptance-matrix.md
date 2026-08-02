# Acceptance Matrix

## Scope

This is the human-readable acceptance matrix for Deve Sub. The machine-readable
source is [`tests/acceptance/matrix.yaml`](../tests/acceptance/matrix.yaml).
The compact registry is
[`docs/acceptance/matrix.tsv`](acceptance/matrix.tsv).

All cases are derived from the archived spec §21 (minimum mandatory acceptance
items). `planned` is not `pass`.

## Summary

| Metric | Count |
|---|---|
| Total cases | 132 |
| P0 (core) | 115 |
| P1 (infrastructure/performance) | 17 |

## Parameterized dimensions

Cases marked with dimensions are automatically expanded across the listed
values where applicable:

| Dimension | Values |
|---|---|
| address | IPv4, IPv6 literal, A-only domain, AAAA-only domain, dual-stack domain |
| input | URI, Base64, Mihomo YAML, sing-box JSON, Xray JSON, V2Ray JSON, Shadowrocket |
| protocol | VLESS Reality, Hysteria2, TUIC v5, Naive, VMess, Trojan, Shadowsocks |
| output | Mihomo, FlClash, sing-box, Xray, v2rayN, v2rayNG, Shadowrocket |

## Case categories

### UI — Base and UI (10 cases)

UI-001 through UI-010. Covers first-run init, i18n, theme, 10k node
performance, mobile, and keyboard navigation.

### AUTH — Authentication and Users (10 cases)

AUTH-001 through AUTH-010. Covers admin init, login, rate limiting, 2FA,
recovery codes, user disable, privilege enforcement, token reset, forced
logout.

### SRC — Subscription Sources (14 cases)

SRC-001 through SRC-014. Covers source CRUD, manual/auto refresh, ETag, failure
retention, zero-node guard, response limits, timeout, cancel, filtering, IPv6,
compression, concurrency, diff.

### PARSE — Parsing and Export (18 cases)

PARSE-001 through PARSE-018. Covers P0 protocol golden tests, input format
parsing, Base64 padding, URL encoding, IPv6, short-id regression,
allowInsecure three-state, round-trip property, fuzz.

### NODE — Node Management (18 cases)

NODE-001 through NODE-018. Covers batch import, dedup, batch operations, region
detection (manual/IPv4/IPv6/dual-stack), override persistence, upstream
deletion, latency probing, UDP reachability, real proxy testing, chain proxy,
cycle detection.

### GEN — V3 Template and Generator (16 cases)

GEN-001 through GEN-016. Covers template CRUD, versioning, rollback, dynamic/
snapshot selection, grouping, drag-and-drop, cycle detection, compatibility
report, strict mode, atomic publish, preview consistency.

### OUT — Subscription Output (14 cases)

OUT-001 through OUT-014. Covers P0 client compatibility, ETag, token error,
expiry, traffic quota, token rotation, short code conflict, concurrent
generation.

### PROBE — Probes (5 cases, P1)

PROBE-001 through PROBE-005. Covers Nezha, DStatus, Komari, data source
failure, multi-source aggregation.

### CLI — Headless and CLI (5 cases)

CLI-001 through CLI-005. Covers headless mode, stdin import, stdout export,
JSON output, doctor.

### DEPLOY — Deployment (4 cases, P1)

DEPLOY-001 through DEPLOY-004. Covers SQLite Compose, Linux install, amd64/arm64
images.

### UPDATE — Update Mechanism (2 cases, P1)

UPDATE-001 through UPDATE-002. Covers signed update and failure rollback.

### SEC — Security (10 cases)

SEC-001 through SEC-010. Covers SSRF, DNS rebinding, redirect, YAML bomb, path
traversal, IP spoofing, SPA routing, token logging, CSRF.

### PERF — Performance (6 cases, P1)

PERF-001 through PERF-006. Covers 10k parsing, 10k list, cached subscription,
uncached generation, concurrent download, long-running soak.

## Authority

- Machine-readable: `tests/acceptance/matrix.yaml`
- Compact registry: `docs/acceptance/matrix.tsv`
- Gate rules: `docs/acceptance/gates.md`
- Historical source: `docs/product-and-architecture-spec.md` §21 (archived)
