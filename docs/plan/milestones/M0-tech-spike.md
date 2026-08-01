# Milestone 0 — Tech Spike

## Scope

Validate the Dioxus Web UI technology choice before committing to full
frontend development. If the spike fails, only the UI adapter is replaced; the
backend, CLI, protocol engine, and database layers are unaffected.

## Dependency

None. M0 is the first milestone.

## Vertical slice

A standalone spike under `spikes/dioxus-ui/` (excluded from the workspace).
The spike is not part of the production build. See ADR-0001.

## UI spike gate

The spike must pass all of the following in a two-week validation:

- 10,000-node virtual list;
- multi-select, pagination, filtering;
- 500-item drag-and-drop sorting;
- Chinese/English i18n switching;
- light/dark/custom theme;
- SSE task progress;
- 30-day traffic chart;
- mobile basic operations;
- Playwright automated tests.

## SQLite concurrency spike

- WAL mode concurrent read/write behavior under expected load.
- Write transaction throughput with batched node imports.
- WAL size growth under sustained write pressure.

## Docker build spike

- Multi-stage Rust + Dioxus Web builder.
- Minimal runtime image, non-root user.
- amd64 and arm64 targets.

## Failure path

If the UI spike fails, the frontend switches to React. React lives in
`apps/web`; Node.js exists only in frontend build and CI, not in the production
runtime. The production server remains pure Rust. The spike report is
retained; experimental spike code is not required to be kept permanently.

## Authority

- Frontend mode: ADR-0001
- Workspace layout: `docs/plan/04-workspace-layout.md` §"Spike exclusion"

## Verification

- UI spike gate items map to acceptance: `UI-008`, `PERF-001`, `PERF-002`.
- Spike result is recorded as a report; `planned` is not pass.
