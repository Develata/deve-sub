# ADR-0001: Frontend Mode — Dioxus Web CSR with Typed REST

- **Status**: Accepted
- **Date**: 2026-08-02

## Context

Deve Sub needs a web UI for managing subscription sources, nodes, templates,
subscriptions, and users. The UI must handle 10,000-node virtual lists,
drag-and-drop sorting, i18n, theming, SSE progress, and mobile access.

Rust frontend frameworks have matured: Dioxus 0.7.x and Leptos 0.8.x both
integrate with Axum. However, Rust frontends may carry higher cost for complex
data tables, drag-and-drop, and accessibility compared to the React ecosystem.

The system architecture must keep the backend, CLI, protocol engine, and
database layers independent of the frontend choice so that the UI adapter can
be replaced without rewriting the rest.

## Decision

Use **Dioxus 0.7.x Web (CSR)** with a **typed REST `/api/v1` client**. Axum
serves the API, public subscription endpoints, and static web assets.

- P0 core business **must not** depend on Dioxus Server Functions. The API
  boundary is the typed REST `/api/v1` surface.
- Dioxus Server Functions may be used for non-critical convenience only, never
  as the primary API contract.
- Before full frontend development, a **two-week UI spike** under
  `spikes/dioxus-ui/` (excluded from the workspace) must pass all gate criteria
  defined in `docs/plan/milestones/M0-tech-spike.md`.
- If the spike fails, the frontend switches to **React** in `apps/web`.
  Node.js exists only in frontend build and CI, not in the production runtime.
  The production server remains pure Rust.
- The production path (`apps/web`) does not expose the specific UI technology
  in its public interface; the swap is an internal adapter change.

## Consequences

- The API boundary is explicit and typed; the frontend is a thin shell.
- The UI adapter is replaceable: swapping Dioxus for React does not touch the
  backend, CLI, protocol engine, or database.
- Server Functions do not become an architectural coupling point.
- The spike gates frontend commitment; `planned` is not `pass`.

## Alternatives considered

1. **Dioxus Fullstack with Server Functions as primary API** — rejected:
   couples frontend to backend, makes the API boundary implicit, harder to
   replace the UI adapter.
2. **React from the start** — rejected: adds Node.js to the production toolchain
   prematurely; the spike validates whether pure Rust suffices first.
3. **Leptos** — rejected: Dioxus 0.7.x has a stronger Fullstack integration
   story and first-party component primitives; both are viable, but Dioxus was
   chosen for ecosystem fit.
