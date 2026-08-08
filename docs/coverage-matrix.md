# Documentation coverage matrix

This matrix keeps the live documentation layers aligned:

- `plan/` — engineering constitution, terminology, and per-milestone blueprints;
- `features/` — user-visible behavior;
- `contracts/` — exact public/module/machine boundaries;
- `acceptance/` — stable proof cases and gates;
- `tasks/` — milestone scheduling only; never behavior authority.

## Foundation and cross-system plans

| Blueprint | Feature projection | Contract / registry | Acceptance binding |
|---|---|---|---|
| `00-engineering-constitution` | — | repository `AGENTS.md`, `tasks/module-work-policy.md` | `acceptance/gates.md` |
| `01-terminology` | all feature vocabulary | `contracts/data-models.md` | matrix vocabulary |
| `02-product-positioning` | — | — | — |
| `03-architecture` | — | `contracts/module-boundaries.md` | — |
| `04-workspace-layout` | — | — | — |
| `05-protocol-engine` | — | `contracts/data-models.md` | `PARSE-*`, `NODE-*` |
| `06-output-profiles` | — | — | `OUT-*` |
| `13-storage` | — | — | `DEPLOY-*`, `PERF-*` |

## Milestone blueprints

| Milestone blueprint | Primary public boundary | Feature projection | Acceptance projection |
|---|---|---|---|
| `milestones/M0-tech-spike` | — | — | `UI-008`, `PERF-001`, `PERF-002` |
| `milestones/M1-infrastructure` | — | `contracts/module-boundaries.md` | — |
| `milestones/M2-auth-and-users` | `contracts/module-boundaries.md` | — | `AUTH-*`, `SEC-009`, `SEC-010` |
| `milestones/M3-protocol-engine` | `contracts/data-models.md` | — | `PARSE-*`, `NODE-*` |
| `milestones/M4-sources-and-node-pool` | `contracts/module-boundaries.md` | — | `SRC-*`, `NODE-*` |
| `milestones/M5-generator-and-v3-template` | `contracts/module-boundaries.md` | — | `GEN-001`–`GEN-016` |

## Test coverage notes

- Domain core types (Node, Authentication, ProtocolConfig variants, Transport,
  CongestionConfig, etc.) are covered by golden integration tests
  (`crates/deve-sub-domain/tests/golden_*.rs`) and the proptest scaffold, not
  by per-type unit tests. This is the intended coverage layer for Phase 1;
  dedicated unit tests may be added as complexity grows.

## Non-matrix documents

| Path | Role | Authority rule |
|---|---|---|
| `acceptance/gates.md` | PR-gate command set and evidence-state rules | defines pass/not-pass states; does not own behavior |
| `overview/architecture.md` | cross-layer/module navigation map | summarizes and links; does not own behavior |
| `tasks/module-work-policy.md` | milestone ownership, commits, review process | schedules work; defers to constitution/plans/contracts |
| `tasks/execution-roadmap.md` | milestone batches and assembly gates | schedules approved contracts; cannot override plans |
| `guides/contributing.md` | collaboration workflow | defers to root/docs `AGENTS.md` and plans |
| `guides/development.md` | local command runbook | commands must match current CI/contracts |
| `adr/` | decision history | explains why; current plan/contract owns behavior after amendment |
| `data-model/core-er.md` | conceptual entity model | migration SQL is the physical source of truth |
| `product-and-architecture-spec.md` | archived task specification | history only; does not override current plans/contracts |

## Rules

- Every product-visible behavior MUST map to an owning plan, typed contract and
  active acceptance row before implementation completion.
- Every cross-module call MUST appear in `contracts/module-boundaries.md` or a
  named more-specific contract.
- A single acceptance case MAY cover several chapters/modules, but its
  assertion and binding must remain exact.
- `planned`, skipped, unavailable and not-run are non-pass states.
- A task/report/overview MUST NOT introduce a new product identity, authority
  class or lifecycle transition.
- Existing code evidence does not promote an incomplete milestone.
- Add a new docs directory only when a real document has a distinct semantic
  role; do not create empty architecture theatre.
