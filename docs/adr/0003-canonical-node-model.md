# ADR-0003: Canonical Node Model

- **Status**: Accepted
- **Date**: 2026-08-02

## Context

Deve Sub supports multiple input formats (URI, Base64, Mihomo YAML, sing-box
JSON, Xray JSON, V2Ray JSON, Shadowrocket) and multiple output targets (Mihomo,
FlClash, sing-box, Xray, v2rayN, v2rayNG, Shadowrocket). Without a unified
representation, each input→output pair would need its own converter (N×M
explosion).

The spec lists 15 protocols. P0 requires full support for 7: VLESS Reality,
Hysteria2, TUIC v5, NaiveProxy, Shadowsocks, VMess, Trojan. The remaining 8 and
future protocols must not be silently lost, but must not be claimed as
supported either.

## Decision

Define a **Canonical Node Model** as the single normalized node representation.
All parsers produce it; all emitters consume it.

- **`ProtocolKind`** is an enum with 15 typed protocol variants plus
  `Unknown(String)` for protocols not yet typed.
- **`ProtocolConfig`** carries typed configuration payloads for the **seven P0
  protocols only**. Non-P0 protocols do not have typed config in Phase 1.
- **`UnsupportedNode`** preserves the raw data of non-P0 or unknown-protocol
  nodes. It is stored in the node pool but is **excluded from emitters** and is
  **not claimed as supported**.
- No protocol is claimed as supported without a passing **round-trip test**
  (input → model → same-format output, semantically identical). See constraint
  #3.

## Consequences

- The enum is forward-compatible: adding typed support for a non-P0 protocol
  later is a non-breaking addition of a `ProtocolConfig` variant.
- `UnsupportedNode` ensures non-P0 nodes are preserved (not silently dropped,
  per constraint #7) while preventing false support claims.
- Clear separation: `ProtocolKind` identifies; `ProtocolConfig` configures.
- The canonical model is the N=1 intermediate, making the converter count N+M
  instead of N×M.

## Alternatives considered

1. **Only P0 7 in the enum** — rejected: adding non-P0 protocols later cascades
   enum changes across all match arms.
2. **P0 7 + `Other(String)` catchall, no typed 15** — rejected: loses the
   15-variant static enumeration benefit; the 15 protocols are known and should
   be typed.
3. **Dynamic protocol registry** — rejected: overengineered for Phase 1; a
   static enum with `Unknown(String)` is simpler and sufficient.
