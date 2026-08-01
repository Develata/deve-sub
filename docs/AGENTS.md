<!-- Parent: ../AGENTS.md -->

# Documentation governance

## Purpose

`docs/` is split by semantic role. The repository copies the layering
discipline from sibling projects without creating empty registries or
speculative chapters.

- `plan/`: current engineering blueprint — constitution, terminology, and
  per-milestone module blueprints.
- `contracts/`: typed module boundaries, CLI, schema, permission, interface
  and data contracts.
- `features/`: user-visible behavior and honest journeys.
- `acceptance/`: active proof bindings plus the retained long-horizon case
  catalog.
- `tasks/`: milestone work policy and execution roadmap; tasks do not override
  plans.
- `overview/`: cross-layer maps; never a second source of authority.
- `guides/`: contributor, local-development and publication handoffs.
- `adr/`: time-ordered architecture decisions; ADRs explain why, while current
  plans define how.
- `data-model/`: conceptual entity model and ER diagrams. The physical schema
  source of truth is the `migrations/` directory.
- `openapi/`: generated OpenAPI spec exported from code. Hand-maintaining this
  is forbidden.
- `product-and-architecture-spec.md`: archived task specification retained as
  product and architecture history. Where it conflicts with `plan/` or
  `contracts/`, the plan and contracts prevail.

Raw discovery notes, rejected drafts, personal infrastructure, private backup
procedures and copied chat/workspace archives do not belong in repository
documentation.

## Reading order

Before changing product or runtime behavior:

1. read `plan/00-engineering-constitution.md`;
2. read `plan/01-terminology.md`;
3. read the relevant `plan/` milestone blueprint and cross-system chapter;
4. read `contracts/module-boundaries.md` and matching specific contracts
   through `coverage-matrix.md`;
5. read the matching feature, acceptance cases, `tasks/` roadmap and current
   milestone batch.

## Authority rules

- Current plan and typed contracts MUST agree. If they do not, stop and resolve
  the contradiction explicitly.
- Features describe what users observe; they MUST NOT invent authority or
  runtime semantics.
- Acceptance rows prove claims; `planned` is not pass.
- Tasks schedule work but MUST NOT redefine product topology or invariants.
- Overview and guides summarize; they MUST link to, rather than duplicate,
  owning contracts.
- ADRs are decision history. An amended ADR is not current behavior authority.
- Code is a projection of approved plans/contracts, not an excuse to weaken
  them.
- The archived spec is history. It does not override current plans or
  contracts.

## Editing discipline

- Keep chapters cohesive and proportionate; do not create empty placeholder
  trees.
- Every new public behavior needs a feature projection and an acceptance
  binding.
- Every cross-system plan MUST state scope, authority, failure/recovery and
  verification entrypoints.
- Move dated evidence to a future `report/` directory only when real evidence
  exists; do not create the directory pre-emptively.
- Run `git diff --check` after docs-only changes.
