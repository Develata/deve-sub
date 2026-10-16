# Subscription protocol acceptance

## Input provenance and native template behavior

Work starts from main `aa6ecea37cb47e0ccecc204122aede45013a30e8` on
`test/subscription-protocol-acceptance`; no merge or deployment is part of this
work. The source is the user's Library attachment
`libfile_b094e7ccb8bc819180a21d868d61c381`, `Pasted text(2).txt` (metadata
15,451 bytes; 350 lines). The Library text API returned the complete text;
materialized download was unavailable. The private original is not committed.

[uploaded-mihomo.yaml](../../tests/fixtures/subscription-acceptance/uploaded-mihomo.yaml)
retains the complete DNS block and four Unicode groups. Following the approved
follow-up, its rule list is shortened to **23 representative rules in original
relative order**, including process, domain, port, IPv4/IPv6 CIDR, `no-resolve`
and final MATCH. Personal/campus domains, resolver endpoints and server IP
rules use reserved `.example`, documentation IPv4 and `2001:db8::` equivalents.
No test contacts those resolvers, node endpoints or original production services.

The native template is accepted without removing its structural placeholders:
`mode: rule` is retained, and absent/null/empty `proxies` is the pool insertion
slot. Nonempty embedded proxies, other modes, actual external `proxy-providers`
and group `use` remain explicit errors. `__PROXY_NODES__` expands at its list
position to eligible selected nodes, sorted by final name and subject to group
filters. Repeated tokens and include flags deduplicate in first-occurrence order.
`__PROXY_PROVIDERS__` and `include-all-providers: true` expand to an empty set
with a warning on generation and cache hits. Empty groups safely become REJECT;
an empty eligible pool cannot publish a successful empty subscription.

DNS values and policy insertion order, rule order and group references are
asserted independently. Tokens are structural group members, never global text
substitutions; literal token text elsewhere and raw template history survive.
Token-named nodes receive qualified aliases only for native templates. Native
routing remains Mihomo-only; other targets use V3 proxy-only templates.

## Explicit protocol/target matrix

`G` means the fixture passes real import, encrypted SQLite storage, generation,
HTTP delivery and independent field assertions. `X` means strict rejection,
reported mixed lenient exclusion and failed unsupported-only delivery.
The matrix is explicit fixture data compared with the implementation registry;
the VLESS family registry is distinguished from its Reality-specific fixture.

| Fixture | Mihomo | sing-box | Xray | V2Ray | Shadowrocket | URI list | Canonical JSON |
|---|---|---|---|---|---|---|---|
| VLESS Reality | G | G | G | X | G | G | G |
| VMess WebSocket | G | G | G | G | G | G | G |
| Trojan | G | G | G | G | G | G | G |
| Shadowsocks | G | G | G | G | G | G | G |
| Hysteria2 salamander | G | G | X | X | G | G | G |
| TUIC v5 | G | G | X | X | G | G | G |
| NaiveProxy | X | X | X | X | X | G | G |
| WireGuard | G | G | G | X | X | G | G |
| AnyTLS | G | G | X | X | X | G | G |
| Snell v4 | G | G | X | X | X | G | G |
| ShadowTLS v3 + SS | G | G | X | X | X | X | G |

**77 cells: 55 generated, 22 rejected.** Fields come from independently authored
[fixtures](../../tests/fixtures/subscription-acceptance/protocols.json), not from
the emitter under test. Public deterministic test keys and fake credentials are
synthetic. G alone is not a GUI-client or live-connectivity claim.

V2Ray rejects Reality without downgrading security; Xray retains it even when
TCP transport is implicit. The complete ShadowTLS subscription URI is rejected
because its format cannot preserve inner configuration/authentication. The
low-level wrapper URI utility retains absent/false/true verification distinctly
and rejects additional settings it cannot encode. Canonical JSON remains the
full-fidelity typed-node representation.

sing-box WireGuard uses root `endpoints`, local `address` and peer
`address`/`port`, with an explicit logical default route. Imports accept current
and legacy schemas, validate keys/CIDRs/numeric bounds and count raw outbounds
plus endpoints before merging wrappers. Distinct peer endpoints and unsupported
interface/dialer options remain raw Unsupported input instead of being collapsed.
Snell uses versions 1–5 on Mihomo and 4/6 on sing-box. TLS camouflage uses
`obfs_mode`/`obfs_host`; nonrepresentable security, userkey, v6 URI mode and
wrong-version options are rejected. Generation cache v5 prevents old output
from bypassing these corrections through hits, active lookup or stale fallback.

Additional regressions cover xhttp exclusions, Snell v3/Restls filtering,
Snell TLS and v6, current IPv6 WireGuard import, default routing, implicit TCP
security, wrapper tag collisions, unsupported inner TLS, malformed/empty/partial
input, unknown protocols, exact exclusion reasons, duplicate imports, cache
notices, ETags and conditional GET. The source-provider test fetches two exact
loopback-only URLs (URI list and ShadowTLS JSON), reconciles snapshots and
storage, then downloads the subscription over actual loopback HTTP. Ephemeral
servers have bounded waits and joined shutdown. This proves subscription
transport, not proxy traffic. External proxy-provider YAML is distinct from
this source fetch path and from existing rule-provider validation.

## Pinned-core evidence

The explicit installer verifies publisher SHA-256 digests and installs
[Mihomo 1.19.30](https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.30),
[sing-box 1.14.3](https://github.com/SagerNet/sing-box/releases/tag/v1.14.3), and
[Xray 26.3.27](https://github.com/XTLS/Xray-core/releases/tag/v26.3.27).
The updated baselines support AnyTLS and Snell; the former known-rejection tests
have been replaced by positive checks of actual delivered bytes.

| Core | Baseline fixtures | Additional delivered cases |
|---|---|---|
| Mihomo 1.19.30 | 10 | Snell TLS; mixed set; uploaded DNS template with all 10 compatible nodes |
| sing-box 1.14.3 | 10 | Snell TLS, Snell v6, IPv6 WireGuard endpoint, implicit-TCP Reality; mixed set |
| Xray 26.3.27 | 5 | Implicit-TCP Reality; mixed set |

That is **31 individual configurations, three mixed sets and one full routing
configuration**, plus invalid-protocol controls. Validators assert exact versions,
use private temporary directories and enforce 10-second deadlines plus a
2-second kill grace. A tiny self-authored PRIVATE/CN GeoSite protobuf allows
DNS-policy validation without downloads; no external geodata is required.
Maintained schema references:
[Mihomo Snell](https://github.com/MetaCubeX/mihomo/blob/v1.19.30/adapter/outbound/snell.go),
[sing-box Snell](https://github.com/SagerNet/sing-box/blob/v1.14.3/option/snell.go),
[WireGuard endpoint](https://github.com/SagerNet/sing-box/blob/v1.14.3/option/wireguard.go),
[default routing](https://github.com/SagerNet/sing-box/blob/v1.14.3/adapter/outbound/manager.go),
[V2Fly transport security](https://www.v2fly.org/config/transport.html).

```bash
cargo test --locked -p deve-sub-server --test subscription_delivery acceptance -- --test-threads=2
python3 scripts/install-validators.py /tmp/deve-sub-validators-current
PATH=/tmp/deve-sub-validators-current:$PATH cargo test --locked -p deve-sub-server --test subscription_delivery acceptance_validators -- --ignored --test-threads=1
```

Ordinary deterministic tests neither install nor execute external cores. The
compatibility CI job explicitly installs pinned tools and runs the ignored
server and existing emitter validator suites. Configuration acceptance does not
establish DNS answers, live proxy connectivity, GUI behavior or performance.
