# 06 — Output Profiles

## Scope

This chapter defines client output profiles, the generation result format, and
the explicit-profile-vs-User-Agent policy.

## P0 output targets

```text
Mihomo
FlClash
sing-box
Xray
v2rayN
v2rayNG
Shadowrocket
```

FlClash is a distinct profile from Mihomo, even though it typically consumes
Mihomo configuration. This separation preserves version differences and UI
feature limits.

## Additional output profiles (M9)

```text
JSON
```

The JSON profile (M9) serializes the canonical node model as a JSON array.
It is not tied to any specific client — useful for programmatic access, API
responses, debugging, and inter-system exchange. See
`docs/plan/milestones/M9-protocol-output-expansion.md`.

## Profile fields

Each profile stores:

```text
Profile name
Target client
Minimum tested version
Supported protocols
Supported transports
Supported TLS fields
Supported proxy chains
Supported proxy group types
Output format
Incompatibility policy
Test fixture version
```

## Generation result

```json
{
  "profile": "flclash",
  "included": 84,
  "excluded": 3,
  "warnings": 2,
  "excluded_nodes": [
    {
      "node_id": "...",
      "name": "naive-test",
      "reason_code": "UNSUPPORTED_PROTOCOL"
    }
  ]
}
```

Incompatible nodes are never silently dropped. The report includes count,
warnings, and per-node exclusion reasons. See constraint #7.

## Explicit profile vs User-Agent

Explicit profile path takes priority over User-Agent auto-detection:

```text
/sub/{token}/mihomo
/sub/{token}/flclash
/sub/{token}/sing-box
/sub/{token}/xray
/sub/{token}/v2rayn
/sub/{token}/v2rayng
/sub/{token}/shadowrocket
/sub/{token}/json
```

## Incompatibility policy

Default: exclude incompatible nodes and generate a report.
Optional: strict mode — generation fails when any incompatible node exists.
Forbidden: silent corruption.

## Authority

- Compatibility conclusions require client validation or official format. See
  constraint #18.
- Acceptance: `OUT-001` through `OUT-014`, `OUT-015` (JSON profile, M9).

## Verification

- Each P0 profile has a compatibility test against the target client or an
  official format fixture.
- Round-trip: canonical model → emitter → target format → parse back.
- JSON profile round-trip: `emit_json` → parse JSON → verify node equality.
  Acceptance: `OUT-015` (M9).
