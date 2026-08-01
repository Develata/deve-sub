<!-- Parent: ../AGENTS.md -->

# Plan chapter governance

## Purpose

`docs/plan/` is the authoritative engineering blueprint. Chapters are numbered
for stable reference. ADRs reference plan anchors by filename.

## Chapter contract

Every cross-system plan chapter MUST state:

- **Scope**: what the chapter covers and what it does not.
- **Authority**: which layer owns the decisions (plan, contract, or code).
- **Failure/recovery**: what happens when things go wrong and how the system
  recovers.
- **Verification entrypoint**: how correctness is proven (acceptance case IDs,
  test types, or smoke paths).

Milestone blueprints under `milestones/` MUST additionally state:

- **Vertical slice**: what runnable surface the milestone delivers.
- **Dependency**: which prior milestones must be complete.

## Editing rules

- Plans define how the system works. ADRs explain why decisions were made.
- An amended plan is current behavior authority; an amended ADR is not.
- Do not create placeholder chapters. Add a chapter when real content exists.
- Keep plan prose dense and scannable; use code blocks for structured data.
- Plans and contracts MUST agree. If they disagree, stop and resolve the
  contradiction explicitly.
