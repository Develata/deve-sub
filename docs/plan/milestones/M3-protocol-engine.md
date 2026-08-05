# Milestone 3 — Protocol Engine

## Scope

Input parsing, canonical node model projection, and URI emission for the seven
P0 protocols. M3 delivers the `deve-sub-protocol` and `deve-sub-emitter` crates:
share-URI parsers that produce canonical [`Node`] values, URI emitters that
serialize [`Node`] values back to share URIs, and round-trip golden tests that
prove semantic fidelity. Container-format parsers (Base64, Mihomo YAML,
sing-box JSON, Xray JSON, V2Ray JSON, Shadowrocket) land in later slices.

The canonical node model, `ProtocolKind`, `ProtocolConfig`, `Endpoint`,
`TlsConfig`, `Transport`, and authentication types already exist in
`deve-sub-domain` from Phase 1. M3 builds the parsing and emission layer on
top of them.

## Dependency

M1 (Infrastructure) must be complete. The domain model, kernel, and workspace
scaffolding are prerequisites. M2 (Auth and Users) is complete but not a hard
dependency — the protocol engine is a pure library with no auth surface.

## Vertical slice

```text
// Library API (deve-sub-protocol)
let node = deve_sub_protocol::parse_uri("vless://...")?;

// Library API (deve-sub-emitter)
let uri = deve_sub_emitter::emit_uri(&node);
assert_eq!(uri, "vless://...");

// Round-trip test
let parsed = parse_uri(&original_uri)?;
let emitted = emit_uri(&parsed);
assert_eq!(normalize(&emitted), normalize(&original_uri));
```

## Deliverables

- `deve-sub-protocol` crate: URI parsers for VLESS Reality, Hysteria2, TUIC v5,
  NaiveProxy, Shadowsocks, VMess, Trojan. Each parser maps a share URI to a
  canonical [`Node`].
- `deve-sub-emitter` crate: URI emitter for all P0 protocols. Maps a canonical
  [`Node`] to a share URI string.
- Round-trip golden tests: parse reserved test URI → Node → emit URI → verify
  semantic equality (constraint #3).
- Regression tests: short-id string preservation (PARSE-013), allowInsecure=0
  → Some(false) (PARSE-014), absent allowInsecure → None (PARSE-015), IPv6
  bracketing (PARSE-012), URL-encoded name/special chars (PARSE-011).
- Base64 padding handling (PARSE-010), one-URI-per-line emission (PARSE-016).
- Container format parsers: Base64 subscription, Mihomo YAML, sing-box JSON,
  Xray JSON, V2Ray JSON, Shadowrocket (PARSE-005 through PARSE-009).
- Fuzz tests for illegal input (PARSE-018).
- Property-based round-trip test (PARSE-017).

## Slicing

M3 is delivered in four slices:

1. **Crate scaffolding + VLESS Reality round-trip**: Create `deve-sub-protocol`
   and `deve-sub-emitter` crates. Implement VLESS Reality URI parser and URI
   emitter with a full round-trip golden test. Establishes the architecture
   pattern (parser trait, emitter trait, error types, test fixture format).
   Acceptance: PARSE-001, PARSE-013, PARSE-014, PARSE-015, PARSE-012.
2. **Remaining P0 URI parsers + emitters**: Hysteria2, TUIC v5, NaiveProxy,
   Shadowsocks, VMess, Trojan. Round-trip golden tests for each. Acceptance:
   PARSE-002, PARSE-003, PARSE-004, PARSE-010, PARSE-011, PARSE-016, PARSE-017.
3. **Container format parsers**: Base64 subscription decoder, Mihomo YAML,
   sing-box JSON, Xray JSON, V2Ray JSON, Shadowrocket. Acceptance: PARSE-005
   through PARSE-009.
4. **Fuzz + property tests**: Illegal-input fuzz (PARSE-018), comprehensive
   round-trip property test across all P0 protocols and input formats
   (PARSE-017 completion).

## Architecture

### Parser interface

```rust
/// Parse a share URI into a canonical Node.
pub fn parse_uri(uri: &str) -> Result<Node, ParseError>;
```

Each protocol has a dedicated parser module. The top-level `parse_uri`
dispatches on the URI scheme (`vless://`, `hysteria2://`, `hy2://`, `tuic://`,
`naive+https://`, `ss://`, `vmess://`, `trojan://`).

### Emitter interface

```rust
/// Emit a canonical Node as a share URI.
pub fn emit_uri(node: &Node) -> Result<String, EmitError>;
```

Each protocol has a dedicated emitter module. The top-level `emit_uri`
dispatches on `node.protocol`.

### Error discipline

`thiserror`-based errors in both crates. `ParseError` and `EmitError` are
structured enums with variants per failure mode (invalid URI, unknown scheme,
missing required field, invalid encoding). No `anyhow` in library APIs.

### Test fixtures

All fixtures use reserved test identifiers (see `docs/contracts/data-models.md`):
- UUID: `00000000-0000-4000-8000-000000000001`
- IPv6: `[2001:db8::1]`
- Public key: `TEST_PUBLIC_KEY`
- Short ID: `01020304`
- Password: `TEST_PASSWORD`

No real node credentials in fixtures (constraint #9).

## Authority

- Canonical node model: ADR-0003
- Security field three-state: ADR-0005
- Protocol engine blueprint: `docs/plan/05-protocol-engine.md`
- Typed contract: `docs/contracts/data-models.md`
- Workspace layout: `docs/plan/04-workspace-layout.md`

## Verification

- Round-trip test (input → model → same-format output, semantically identical)
  is required before claiming protocol support (constraint #3).
- Golden tests for each P0 protocol and input format. Acceptance: `PARSE-*`.
- Fuzz tests for illegal input. Acceptance: `PARSE-018`.
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all pass.
- `python3 scripts/check_docs.py` passes.
