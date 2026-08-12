# Data Models

## Scope

This contract defines the typed data model projections for the canonical node
model and related types. The domain implementation lives in `deve-sub-domain`;
this document is the typed contract projection.

## Canonical Node Model

See ADR-0003 for the decision and `docs/plan/05-protocol-engine.md` for the
full blueprint.

### ProtocolKind

An enum with fifteen typed variants plus `Unknown(String)`:

```text
Vless, VMess, Trojan, Shadowsocks, Hysteria2, TuicV5, NaiveProxy,
Socks5, Http, HysteriaV1, AnyTls, Snell, WireGuard, ShadowTls, Ssh,
Unknown(String)
```

### ProtocolConfig

Typed payloads for the seven P0 protocols (M3) and four additional protocols
(M9):

```text
// M3 (P0)
VlessRealityConfig, Hysteria2Config, TuicV5Config, NaiveProxyConfig,
ShadowsocksConfig, VMessConfig, TrojanConfig

// M9
WireGuardConfig, AnyTlsConfig, SnellConfig, ShadowTlsConfig
```

Non-P0 or unknown protocols not covered by M9 use `UnsupportedNode`.

### UnsupportedNode

Preserves raw data for non-P0 or unknown-protocol nodes. Excluded from
emitters. Not claimed as supported.

### TlsConfig (three-state)

See ADR-0005. `skip_cert_verify: Option<bool>`:

- `None` — not provided, no default fill.
- `Some(false)` — explicitly secure.
- `Some(true)` — explicitly insecure.

Never auto-set `Some(true)` for compatibility.

### Host

```text
Ipv4(Ipv4Addr) | Ipv6(Ipv6Addr) | Domain(DomainName)
```

IPv6 URI output must auto-add brackets. Database must not store IPv6 as
arbitrary strings for later concatenation.

### TransportKind

```text
Tcp | Kcp | Ws | H2 | Quic | Grpc | HttpUpgrade | Xtls | Xhttp (M9)
```

`Xhttp` (M9) is a transport mode for VLESS and VMess. Mihomo: `network: xhttp`;
Xray: `network: xhttp` (alias for `splithttp`); sing-box: not supported.

### ProfileKind

```text
Mihomo | SingBox | Xray | V2Ray | Shadowrocket | UriList | Json (M9)
```

`Json` (M9) serializes the canonical node model as a JSON array — not tied
to any specific client. See `docs/plan/06-output-profiles.md`.

## Serialization

- Protocol config, TLS, transport, multiplex, obfuscation, and congestion are
  serialized as JSON in the database.
- The canonical model is the authority; database JSON columns are projections.
- Round-trip (parse → model → emit) must preserve semantic identity.

## Reserved test identifiers

Fixtures must use reserved test identifiers, never real credentials:

- UUID: `00000000-0000-4000-8000-000000000001`
- IPv6: `[2001:db8::1]`
- Public key: `TEST_PUBLIC_KEY`
- Short ID: `01020304`

## Authority

- Canonical model: ADR-0003
- Three-state TLS: ADR-0005
- Protocol engine blueprint: `docs/plan/05-protocol-engine.md`
- Physical schema: `migrations/`

## Verification

- Round-trip tests: `PARSE-001` through `PARSE-027`.
- Fuzz: `PARSE-018`.
- Regression: `PARSE-013` (short-id string), `PARSE-014` (allowInsecure=0),
  `PARSE-015` (absent allowInsecure).
- JSON profile round-trip: `OUT-015` (M9).
