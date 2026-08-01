# Documentation

Deve Sub uses a docs-as-code structure: layered authority for engineering
blueprint, typed contracts, product behavior, proof, and historical rationale.

## Start here

1. [`AGENTS.md`](../AGENTS.md) — repository governance, work loop, Git strategy
2. [`plan/00-engineering-constitution.md`](plan/00-engineering-constitution.md)
3. [`plan/01-terminology.md`](plan/01-terminology.md)
4. [`plan/04-workspace-layout.md`](plan/04-workspace-layout.md)
5. [`contracts/module-boundaries.md`](contracts/module-boundaries.md)
6. [`tasks/execution-roadmap.md`](tasks/execution-roadmap.md)
7. [`overview/architecture.md`](overview/architecture.md)
8. [`coverage-matrix.md`](coverage-matrix.md)
9. the matching milestone plan/feature/contract/acceptance documents

## Structure

| Path | Answers |
|---|---|
| [`plan/`](plan/) | How is the system engineered, and which milestones exist? |
| [`contracts/`](contracts/) | What exact schemas, CLI, interfaces, permissions and data models are exposed? |
| [`features/`](features/) | What does the user see and what is the honest journey/status? |
| [`acceptance/`](acceptance/) | What is active now, and which stable proof cases must be retained? |
| [`tasks/`](tasks/) | How are milestones split and committed? |
| [`overview/`](overview/) | How do the layers fit together? |
| [`guides/`](guides/) | How do contributors develop and review? |
| [`adr/`](adr/) | Why were major architecture decisions made? |
| [`data-model/`](data-model/) | What is the conceptual entity model? |
| [`openapi/`](openapi/) | What is the generated API spec? |
| [`product-and-architecture-spec.md`](product-and-architecture-spec.md) | What was the original task specification? (archived) |

[`coverage-matrix.md`](coverage-matrix.md) maps the live layers.
[`AGENTS.md`](AGENTS.md) defines documentation authority and editing discipline.

## Deliberate exclusions

This tree does not retain raw discovery workspaces, rejected proposal dumps,
personal infrastructure/backup procedures or empty speculative registry/report
directories. Useful accepted semantics are carried by the current plans,
features, contracts, acceptance rows and ADRs; Git history preserves prior
tracked revisions.
