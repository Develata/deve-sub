# 05 — Protocol Engine

## Scope

This chapter defines the canonical node model, the separation of input formats
from protocols, P0 protocol requirements, security field semantics, the
override model, and node export. See ADR-0003 for the canonical node model
decision and ADR-0005 for security field three-state semantics.

## Input formats vs protocols

Input formats (container formats of subscription responses):

```text
分享 URI 列表
Base64 通用订阅
Mihomo / Clash YAML
sing-box JSON
Xray JSON
V2Ray JSON
Shadowrocket 分享列表或配置
```

Protocols (wire-level proxy protocols):

```text
VLESS
VMess
Trojan
Shadowsocks
Hysteria2
TUIC v5
NaiveProxy
SOCKS5
HTTP
Hysteria v1
AnyTLS
Snell
WireGuard
ShadowTLS
SSH
```

P0 protocols (full support required in M3):

```text
VLESS Reality
Hysteria2
TUIC v5
NaiveProxy
Shadowsocks
VMess
Trojan
```

## Canonical Node Model

```rust
pub struct Node {
    pub id: NodeId,
    pub display_name: String,
    pub protocol: ProtocolKind,
    pub config: ProtocolConfig,
    pub endpoint: Endpoint,
    pub authentication: Authentication,
    pub transport: Option<Transport>,
    pub tls: Option<TlsConfig>,
    pub udp: UdpCapability,
    pub multiplex: Option<MultiplexConfig>,
    pub obfuscation: Option<Obfuscation>,
    pub congestion: Option<CongestionConfig>,
    pub chain: Option<NodeChain>,
    pub source: NodeSource,
    pub tags: Vec<TagId>,
    pub region: RegionAssignment,
    pub extras: BTreeMap<String, serde_json::Value>,
}
```

> **Implementation drift (CC3):** `protocol` and `config` are independent
> public fields, so inconsistent pairings (e.g. `ProtocolKind::Trojan` +
> `ProtocolConfig::VMess`) are representable. The kind↔config invariant is
> upheld by parsers and emitters (M3), not by the type system. A sum type
> (e.g. `NodeProtocol::VlessReality(VlessRealityConfig) | ...`) would make
> illegal states unrepresentable but is deferred to M3 when the parser/
> emitter context is available. See the WHY comment on `Node` in
> `crates/deve-sub-domain/src/node.rs`.

### Host

```rust
pub enum Host {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Domain(DomainName),
}
```

Output IPv6 URI must auto-add brackets: `vless://uuid@[2001:db8::1]:443?...`.
The database must not store IPv6 addresses as arbitrary strings for later
concatenation.

### ProtocolKind and ProtocolConfig

`ProtocolKind` is an enum with fifteen typed variants plus `Unknown(String)`.
`ProtocolConfig` carries typed payloads for the seven P0 protocols only. Non-P0
or unknown protocols use `UnsupportedNode`, which preserves raw data but does
not enter emitters. See ADR-0003.

## Security fields (three-state)

```rust
pub struct TlsConfig {
    pub enabled: bool,
    pub server_name: Option<String>,
    pub skip_cert_verify: Option<bool>,
    pub alpn: Vec<String>,
    pub client_fingerprint: Option<String>,
    pub certificate_pins: Vec<CertificatePin>,
    pub reality: Option<RealityConfig>,
}
```

Mapping:

```text
allowInsecure=0 → Some(false)
allowInsecure=1 → Some(true)
参数不存在       → None
```

The system must never auto-set `skip_cert_verify=true` for compatibility. See
ADR-0005.

## VLESS Reality

Supported fields:

```text
uuid, server, port, encryption, flow, network, security, sni, fp, pbk, sid,
spx, allowInsecure, packetEncoding, udp, xudp
```

Constraints:

- `short-id` is always a string. YAML must not coerce pure-digit short IDs to
  integers.
- `pbk` must be validated as Base64URL.
- `security=reality` explicitly denotes Reality.
- `xtls-rprx-vision` is modeled as-is. Output profiles without Vision support
  must exclude and report.
- Never auto-set `skip-cert-verify=true` for compatibility.

## Hysteria2

Supported fields:

```text
hysteria2://, hy2://, password/auth, sni, alpn, skip-cert-verify, pinSHA256,
obfs, obfs-password, up, down, ports, port hopping, hop interval, fast-open,
lazy
```

## TUIC v5

Supported fields:

```text
uuid, password, token, sni, alpn, skip-cert-verify, congestion-controller,
udp-relay-mode, zero-rtt-handshake, heartbeat, disable-sni
```

Internally stored as `Duration`. Output converts per target format. Never mix
seconds and milliseconds.

## NaiveProxy

Supported fields:

```text
username, password, server, port, sni, alpn, quic, http2, http3,
skip-cert-verify, certificate pin
```

Naive must not be downgraded to a plain HTTP node. Unsupported target clients
exclude by default, generate a compatibility report, and may be set to strict
fail. Silent corruption is forbidden.

## Override model

Remote subscription updates must not overwrite manual edits. The effective
node is upstream raw + human override.

Override fields:

- name, region, tags, enabled, SNI, cert verify, fingerprint, chain, sort
  order, notes.

When an upstream node is deleted:

- mark as `missing_from_source`;
- nodes used by subscriptions are not physically deleted immediately;
- admin can restore, replace, or clean up;
- source snapshot is retained for comparison.

> **Implementation drift (C8):** The `missing_from_source` flag and override
> snapshot fields are not yet present in the current `Node` / `NodeSource`
> types in `crates/deve-sub-domain/`. They land with the Source aggregate
> and override machinery in a future milestone (M3+). The current
> `NodeSource` carries `source_label`, `raw_uri`, and `imported_at` only.

## Node export

Export scopes: current filter result, selected nodes, all nodes, by source, by
tag, by protocol.

Text format: one standard share URI per line, UTF-8, LF line endings.

Test fixtures must use reserved test addresses, never real node credentials:

```text
vless://00000000-0000-4000-8000-000000000001@[2001:db8::1]:443?security=reality&type=tcp&allowInsecure=0&sni=example.com&fp=chrome&flow=xtls-rprx-vision&sid=01020304&pbk=TEST_PUBLIC_KEY&encryption=none#IPv6-Test
```

## Authority

- Canonical node model decision: ADR-0003
- Security field three-state: ADR-0005
- Typed contract: `docs/contracts/data-models.md`

## Verification

- Round-trip test (input → model → same-format output, semantically identical)
  is required before claiming protocol support. See constraint #3.
- Golden tests for each P0 protocol and input format. Acceptance: `PARSE-*`.
- Fuzz tests for illegal input. Acceptance: `PARSE-018`.
