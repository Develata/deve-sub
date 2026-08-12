# Milestone 9 — Protocol and Output Expansion

## Scope

Typed configuration, URI parsing, and container-format emission for four
additional proxy protocols (WireGuard, AnyTLS, Snell, ShadowTLS), one new
transport mode (xhttp), and one new output profile (JSON). M9 upgrades these
protocols from `ProtocolConfig::Unsupported` to fully typed variants with
round-trip-tested parsers and emitters, and adds the xhttp transport to
`TransportKind` and the JSON profile to `ProfileKind`.

M9 delivers: typed config structs in `deve-sub-domain`, URI and container
parsers in `deve-sub-protocol`, emitters in `deve-sub-emitter`, compatibility
matrix updates in `deve-sub-compatibility`, and acceptance golden/round-trip
tests for each new protocol, transport, and profile.

## Dependency

M3 (Protocol Engine) must be complete. The canonical node model,
`ProtocolKind`, `ProtocolConfig`, `TransportKind`, parser/emitter architecture,
and round-trip test infrastructure are prerequisites. M9 extends the M3
foundation with additional typed variants using the established patterns.

M4 (Sources and Node Pool) must be complete. The unified node pool stores
`ProtocolConfig` as JSON; M9 adds new variants that flow through the existing
serialization path without schema changes.

M5 (Generator and V3 Template) must be complete. The compatibility matrix
(`deve-sub-compatibility`) determines which protocols/transports each output
profile supports. M9 extends the matrix with the new protocols and transport.

M6 (Subscription Distribution) must be complete. The delivery pipeline serves
generated output by `ProfileKind`; M9 adds `ProfileKind::Json` as a new
delivery target.

## Vertical slice

```text
// Parse a WireGuard share URI
let node = deve_sub_protocol::parse_uri("wireguard://...")?;
assert_eq!(node.protocol, ProtocolKind::WireGuard);
assert!(matches!(node.config, ProtocolConfig::WireGuard(_)));

// Emit to mihomo YAML
let yaml = deve_sub_emitter::emit_mihomo(&[node])?;
assert!(yaml.contains("type: wireguard"));

// Emit to JSON profile
let json = deve_sub_emitter::emit_json(&[node])?;
let parsed: Vec<serde_json::Value> = serde_json::from_str(&json)?;
```

## Deliverables

- Domain typed configs: `WireGuardConfig`, `AnyTlsConfig`, `SnellConfig`,
  `ShadowTlsConfig` structs in `crates/deve-sub-domain/src/protocol_config.rs`.
  Four new `ProtocolConfig` variants: `WireGuard(WireGuardConfig)`,
  `AnyTls(AnyTlsConfig)`, `Snell(SnellConfig)`, `ShadowTls(ShadowTlsConfig)`.
- Domain transport: `TransportKind::Xhttp` variant added to
  `crates/deve-sub-domain/src/transport.rs`; `XhttpConfig` struct carrying
  path, host, mode, and xmux settings; `Transport` struct extended to carry
  xhttp-specific configuration.
- Protocol parsers: URI parsers for `wireguard://`, `anytls://`, `snell://`,
  `shadow-tls://` schemes in `crates/deve-sub-protocol/src/uri.rs` (each
  dispatching to a dedicated module). Container-format parsers extended in
  `crates/deve-sub-protocol/src/container/` to parse the four new protocols
  from mihomo YAML and sing-box JSON inputs.
- Emitters: mihomo, sing-box, Xray, V2Ray, Shadowrocket, and URI emitters
  extended in `crates/deve-sub-emitter/src/` to emit the four new protocols
  where the target client supports them. New `emit_json` function for the
  JSON output profile.
- Compatibility matrix: `crates/deve-sub-compatibility/src/lib.rs` extended
  with `ProfileKind::Json` and updated capability tables for the four new
  protocols and xhttp transport.
- Migration: none. `ProtocolConfig` and `Transport` are serialized as JSON in
  the existing `nodes.config_json` and `nodes.transport_json` columns. New
  variants are backward-compatible — old rows with `Unsupported` configs
  remain valid; a re-parse of the source upgrades them to typed configs.
- Round-trip golden tests: parse → model → same-format emit → semantic
  equality, for each new protocol in each supported format (URI, mihomo YAML,
  sing-box JSON, Xray JSON where supported). Acceptance: PARSE-019 through
  PARSE-026.
- JSON profile round-trip test: emit → parse → verify. Acceptance: OUT-015.
- xhttp transport round-trip test: VLESS+xhttp URI → model → mihomo/Xray emit
  → verify. Acceptance: PARSE-027.

## Slicing

M9 is delivered in five slices:

1. **WireGuard typed config + URI + emitters**: `WireGuardConfig` domain
   struct, `wireguard://` URI parser, mihomo YAML / sing-box JSON / Xray JSON
   container parsers and emitters. Golden round-trip tests. Acceptance:
   PARSE-019 (WireGuard URI), PARSE-020 (WireGuard mihomo YAML),
   PARSE-021 (WireGuard sing-box JSON).
2. **AnyTLS typed config + URI + emitters**: `AnyTlsConfig` domain struct,
   `anytls://` URI parser, mihomo YAML / sing-box JSON container parsers and
   emitters (Xray does not support AnyTLS — excluded with report). Golden
   round-trip tests. Acceptance: PARSE-022 (AnyTLS URI), PARSE-023 (AnyTLS
   mihomo/sing-box).
3. **Snell typed config + URI + emitters**: `SnellConfig` domain struct,
   `snell://` URI parser (sublinkPro de-facto format), mihomo YAML / sing-box
   JSON container parsers and emitters (Xray does not support Snell — excluded
   with report). Version range handling: mihomo v1–5, sing-box v4/v6.
   Golden round-trip tests. Acceptance: PARSE-024 (Snell URI), PARSE-025
   (Snell mihomo/sing-box).
4. **ShadowTLS typed config + URI + emitters**: `ShadowTlsConfig` domain
   struct, `shadow-tls://` URI parser, sing-box JSON container parser and
   emitter (standalone `type: shadowtls`). Mihomo emitter projects ShadowTLS
   as an obfuscation layer under the inner protocol (`shadow-tls-opts` for
   vless/trojan/vmess/anytls; `plugin: shadow-tls` for ss; `obfs-opts.mode:
   shadow-tls` for snell). Xray does not support ShadowTLS — excluded with
   report. Golden round-trip tests. Acceptance: PARSE-026 (ShadowTLS
   sing-box/mihomo projection).
5. **xhttp transport + JSON output profile**: `TransportKind::Xhttp` +
   `XhttpConfig` domain struct, URI `type=xhttp` query param parsing,
   mihomo `network: xhttp` + `xhttp-opts` emitter, Xray `network: xhttp`
   (splitHTTP alias) emitter. sing-box does not support xhttp — excluded with
   report. `ProfileKind::Json` + `emit_json` function (canonical node model
   serialized as a JSON array). Acceptance: PARSE-027 (xhttp transport
   round-trip), OUT-015 (JSON profile round-trip).

## Architecture

### WireGuard

```text
WireGuardConfig {
  private_key: String,           // base64
  address: Vec<IpCidr>,          // local tunnel addresses
  peers: Vec<WireGuardPeer>,     // usually one
  mtu: Option<u32>,              // default 1408 (sing-box) / 1420 (mihomo)
  workers: Option<u32>,          // mihomo only
  dns: Vec<String>,              // mihomo remote-dns-resolve
}

WireGuardPeer {
  public_key: String,            // base64
  pre_shared_key: Option<String>,// base64
  endpoint: Endpoint,            // server:port
  allowed_ips: Vec<IpCidr>,
  reserved: Option<[u8; 3]>,     // mihomo/sing-box specific
  persistent_keepalive: Option<Duration>,
}
```

WireGuard has **no TLS layer** — it uses Noise IK handshake with X25519 +
ChaCha20-Poly1305. No SNI, no cert-verify, no fingerprint fields. The
`tls` field on `Node` must be `None` for WireGuard nodes.

URI scheme: `wireguard://<private-key>@<server>:<port>?publickey=...&address=...&presharedkey=...&reserved=...&mtu=...#name`

### AnyTLS

```text
AnyTlsConfig {
  password: String,              // required
  // TLS is required (always TLS)
  // TLS fields live on Node.tls: sni, alpn, skip_cert_verify, fingerprint
  idle_session_check_interval: Option<Duration>,  // sing-box only
  idle_session_timeout: Option<Duration>,          // sing-box only
  min_idle_session: Option<u32>,                   // sing-box only
  // Nested obfs (mihomo): shadow-tls-opts, restls-opts, jls-opts
  // projected via Node.obfuscation or nested in extras
}
```

URI scheme: `anytls://<password>@<host>:<port>?sni=...&insecure=0|1#name`
(default port 443; `insecure=1` → `tls.skip_cert_verify = Some(true)`)

Compatibility: mihomo ✅, sing-box ✅, Xray ❌ (excluded with report).

### Snell

```text
SnellConfig {
  psk: String,                   // pre-shared key, required
  version: SnellVersion,         // V1 | V2 | V3 | V4 | V5 | V6
  reuse: Option<bool>,           // mihomo v4/v5 only
  obfs: Option<SnellObfs>,       // mihomo obfs-opts
}

SnellVersion: V1 | V2 | V3 | V4 | V5 | V6
// mihomo supports V1–V5; sing-box supports V4 and V6 only.
// V6 has different mode semantics (default | unshaped | unsafe-raw).

SnellObfs {
  mode: SnellObfsMode,           // Tls | Http | ShadowTls | Restls | Jls
  host: Option<String>,
  password: Option<String>,
  version: Option<u32>,
  alpn: Vec<String>,
}
```

Snell has **no TLS by default**. TLS only if `obfs.mode = Tls` (simple TLS
obfs). The `tls` field on `Node` is `None` unless obfs-mode is TLS-shaped.

URI scheme (de-facto, sublinkPro): `snell://<psk>@<server>:<port>?psk=...&version=4&udp=1#name`
No official URI standard exists; Deve Sub parses and emits this de-facto format.

Compatibility: mihomo ✅ (v1–5), sing-box ✅ (v4/v6 only), Xray ❌. When
emitting to sing-box, versions 1/2/3/5 are excluded with report (constraint
#7). When emitting to Xray, all Snell nodes are excluded with report.

### ShadowTLS

ShadowTLS is a TLS-camouflage wrapper. Its canonical model carries the
ShadowTLS handshake parameters plus a reference to the inner protocol.

```text
ShadowTlsConfig {
  version: ShadowTlsVersion,     // V1 | V2 | V3
  password: Option<String>,      // required for V2/V3, None for V1
  // TLS fields live on Node.tls (the camouflage TLS handshake target)
  inner_protocol: ProtocolKind,  // the protocol wrapped inside ShadowTLS
  inner_config: Box<ProtocolConfig>, // typed config of the inner protocol
}
```

**Mihomo projection**: ShadowTLS is not a standalone `type` in mihomo. The
emitter projects ShadowTLS as an obfuscation layer under the inner protocol:
- inner = Shadowsocks → `type: ss` + `plugin: shadow-tls` + `plugin-opts`
- inner = Snell → `type: snell` + `obfs-opts: { mode: shadow-tls }`
- inner = VLESS/Trojan/VMess/AnyTLS → `type: <inner>` + `shadow-tls-opts`

**sing-box projection**: ShadowTLS is a standalone `type: shadowtls` outbound.
The inner protocol runs as a separate outbound that chains through the
ShadowTLS outbound (detour). The emitter generates the ShadowTLS outbound and
the inner protocol outbound with `detour: <shadowtls-tag>`.

**Xray**: not supported. Excluded with report (constraint #7).

URI scheme (de-facto): `shadow-tls://<password>@<server>:<port>?version=2&sni=...#name`

### xhttp transport

xhttp is a **transport mode** (like ws, grpc, h2), not a standalone protocol.
It applies to VLESS and VMess only.

```text
TransportKind::Xhttp

XhttpConfig {
  path: Option<String>,
  host: Option<String>,
  mode: XhttpMode,               // Auto | StreamOne | StreamUp | PacketUp
  // Padding, session, xmux settings (xray-extended fields)
  // Stored in Transport extras or a typed XhttpConfig on Transport
}
```

URI: `vless://...?type=xhttp&path=/abc&host=example.com&mode=stream-one&...`

Mihomo: `network: xhttp` + `xhttp-opts: { ... }` block.
Xray: `network: xhttp` (alias → `splithttp`) + `xhttpSettings: { ... }`.
sing-box: **not supported** — excluded with report.

The xhttp config has many Xray-specific advanced fields (padding, xmux,
session management). The canonical model carries the core fields (path, host,
mode) as typed and the advanced fields in `extras: BTreeMap<String,
serde_json::Value>`, preserving them through round-trip without modeling every
field. This follows the existing `extras` pattern for forward compatibility.

### JSON output profile

```text
ProfileKind::Json

emit_json(nodes: &[Node]) -> Result<String, EmitError>
```

The JSON profile serializes the canonical node model as a JSON array:

```json
[
  {
    "id": "01J...",
    "display_name": "test-node",
    "protocol": "Vless",
    "config": { ... },
    "endpoint": { ... },
    "transport": { ... },
    "tls": { ... },
    ...
  }
]
```

This is the canonical `Node` serialized via `serde_json::to_string_pretty`. It
is not tied to any specific client — it is useful for programmatic access,
API responses, debugging, and inter-system exchange. The schema is the
`Node` type's serde representation, stable across versions via the
`#[non_exhaustive]` and `#[serde(rename_all = "snake_case")]` discipline.

### Compatibility matrix

The updated matrix in `deve-sub-compatibility`:

| Protocol | Mihomo | SingBox | Xray | V2Ray | Shadowrocket | UriList | Json |
|---|---|---|---|---|---|---|---|
| WireGuard | ✅ | ✅ | ✅ | ❌ | ❌ | ✅ | ✅ |
| AnyTLS | ✅ | ✅ | ❌ | ❌ | ❌ | ✅ | ✅ |
| Snell | ✅ | ✅ (v4/v6) | ❌ | ❌ | ❌ | ✅ | ✅ |
| ShadowTLS | ✅ (obfs) | ✅ | ❌ | ❌ | ❌ | ✅ | ✅ |

| Transport | Mihomo | SingBox | Xray | V2Ray | Shadowrocket |
|---|---|---|---|---|---|
| xhttp | ✅ | ❌ | ✅ | ❌ | ❌ |

Incompatible nodes are excluded with a per-node reason in the generation
report (constraint #7). Snell v1/2/3/5 → sing-box: excluded. AnyTLS/Snell/
ShadowTLS → Xray/V2Ray/Shadowrocket: excluded. xhttp → sing-box/
Shadowrocket: excluded.

## Failure/recovery

- Unsupported target client: the emitter excludes the node and records the
  reason in the generation report (constraint #7). No silent corruption.
  The compatibility matrix is the authority for support decisions.
- Snell version mismatch: if a node has `SnellVersion::V3` and the target is
  sing-box (which supports v4/v6 only), the node is excluded with reason
  `UNSUPPORTED_PROTOCOL_VERSION`. The report includes the version and the
  target's supported range.
- ShadowTLS mihomo projection: if the inner protocol is not one of
  Shadowsocks/Snell/VLESS/Trojan/VMess/AnyTLS, the mihomo emitter excludes
  the node with reason `SHADOWTLS_INNER_UNSUPPORTED`. sing-box projection
  always succeeds (standalone type, chains via detour).
- xhttp advanced fields: fields not modeled in `XhttpConfig` are preserved in
  `extras` and passed through. If a target client does not support a field,
  it is silently dropped from that target's output (advanced fields are
  optional, not security-relevant). Core fields (path, host, mode) are
  always emitted.
- sing-box WireGuard deprecation: sing-box deprecated the `wireguard` outbound
  in favor of the `endpoint` type (1.13.0+). The emitter emits the legacy
  `type: wireguard` outbound for now (widely compatible). A future slice may
  add `type: endpoint` emission for sing-box 1.13.0+.

## Authority

- Canonical node model: ADR-0003
- Protocol engine blueprint: `docs/plan/05-protocol-engine.md` (amended in M9
  to add the four new protocol sections and xhttp transport)
- Output profiles blueprint: `docs/plan/06-output-profiles.md` (amended in M9
  to add the JSON profile)
- Typed contract: `docs/contracts/data-models.md` (amended in M9 to add the
  new ProtocolConfig variants, TransportKind::Xhttp, ProfileKind::Json)
- Compatibility matrix: `crates/deve-sub-compatibility/src/lib.rs`
- DTO ownership: ADR-0004
- Acceptance: PARSE-019 through PARSE-027, OUT-015

## Verification

- WireGuard URI round-trip: parse reserved test `wireguard://` URI → Node →
  emit URI → verify semantic equality. Acceptance: PARSE-019.
- WireGuard mihomo YAML round-trip: parse mihomo YAML → Node → emit mihomo
  YAML → verify. Acceptance: PARSE-020.
- WireGuard sing-box JSON round-trip: parse sing-box JSON → Node → emit
  sing-box JSON → verify. Acceptance: PARSE-021.
- AnyTLS URI round-trip. Acceptance: PARSE-022.
- AnyTLS mihomo/sing-box round-trip. Acceptance: PARSE-023.
- Snell URI round-trip. Acceptance: PARSE-024.
- Snell mihomo/sing-box round-trip (version range handling: v4 emits to both,
  v3 excluded from sing-box with report). Acceptance: PARSE-025.
- ShadowTLS sing-box standalone round-trip + mihomo obfs projection
  round-trip. Acceptance: PARSE-026.
- xhttp transport: VLESS+xhttp URI → mihomo/Xray emit → verify. Acceptance:
  PARSE-027.
- JSON profile round-trip: emit_json → parse JSON → verify node equality.
  Acceptance: OUT-015.
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
- OpenAPI spec regenerated (no API surface change in M9; spec unchanged if
  no new routes).
