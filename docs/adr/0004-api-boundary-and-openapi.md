# ADR-0004: API Boundary and OpenAPI Toolchain

- **Status**: Accepted
- **Date**: 2026-08-02

## Context

Deve Sub exposes a typed REST `/api/v1` surface as the API boundary (see
ADR-0001). An OpenAPI specification is needed for client generation,
documentation, and contract verification. Hand-maintaining the spec creates
drift between code and documentation.

## Decision

Use **utoipa + utoipa-axum + utoipa-scalar**.

- **DTO definitions and `ToSchema` derives** live in the `deve-sub-contract`
  crate.
- **Path, method, and status definitions** live in the API crate
  (`deve-sub-server`).
- **`OpenApiRouter`** registers routes and OpenAPI documentation simultaneously,
  ensuring the spec is derived from the actual handler signatures.
- The OpenAPI spec is **exported to `docs/openapi/openapi.json`** by CI.
- **Hand-maintaining `docs/openapi/openapi.json` is forbidden.**
- **Scalar UI** serves interactive API documentation at `/docs`.

## Consequences

- Single source of truth: Rust types define both the API and the spec.
- Low drift: the spec is generated, not written.
- CI verifies that the exported spec matches the code.
- The contract crate carries schema definitions; the API crate carries route
  definitions. This separation keeps the contract crate free of Axum
  dependencies while allowing `ToSchema` derives on DTOs.

## Alternatives considered

1. **Hand-written `openapi.json` in the contract crate** — rejected: high
   maintenance cost, inevitable drift from code, violates the "generated, not
   hand-maintained" principle.
2. **aide (Axum-native OpenAPI)** — rejected: utoipa has broader adoption,
   stronger `ToSchema` derive support, and a path-agnostic schema model that
   fits the contract/API split better.
3. **Server Functions as the API** — rejected: see ADR-0001; the API boundary
   must be the typed REST surface.
