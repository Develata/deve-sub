# ADR-0006: Docker Base Image and Internalized Healthcheck

- **Status**: Accepted
- **Date**: 2026-08-12

## Context

The M0 and M1 Docker deliverables shipped with `debian:bookworm-slim` as the
runtime base and `rust:1.97.1-bookworm` as the builder. The container
`HEALTHCHECK` issued `curl -sf http://127.0.0.1:8080/health/live`, which forced
`curl` into the runtime image purely to perform a single HTTP GET.

Debian 13 "trixie" became the current stable release on 2025-08-09 with
long-term support through 2030, superseding bookworm as the recommended stable
baseline. The runtime binary has no dynamic library dependencies beyond glibc:
SQLite is statically compiled into the binary, and TLS uses rustls rather than
OpenSSL. A base image bump is therefore low-risk from a dependency standpoint.

M1 is already marked "done". This ADR authorizes a plan amendment to M1's
Docker deliverable rather than introducing a new milestone, because the change
is a base-image refresh plus a self-contained healthcheck internalization.

## Decision

1. Switch the runtime base to **`debian:trixie-slim`** and the builder to
   **`rust:1.97.1-trixie`**. The Rust version is unchanged.
2. Internalize the healthcheck as a CLI subcommand:
   **`deve-sub health live`** and **`deve-sub health ready`**. Each subcommand
   performs an HTTP GET to the running server's own `/health/live` or
   `/health/ready` endpoint and exits 0 on a 2xx response, 1 on any non-2xx
   response or connection failure. The Docker `HEALTHCHECK` becomes
   `CMD ["/app/deve-sub", "health", "live"]`.
3. The health subcommand is an **HTTP loopback**, not an in-process database
   probe. A Docker `HEALTHCHECK` executes as a separate process that cannot
   read the server's in-process state, so the only way to ask the running
   server about itself is to call its HTTP health endpoints from outside.
4. Remove **`curl`** from the runtime image. The CLI binary now performs the
   health probe that curl used to perform.
5. Apply safe image cleanup: `rm -rf /usr/share/doc /usr/share/man` alongside
   the existing `apt-get clean && rm -rf /var/lib/apt/lists/*`.
6. Retain `apt`, `dpkg`, coreutils, and shell in the runtime image.
   Debuggability ranks above disk savings in this project's engineering
   priorities.
7. Bump `HEALTHCHECK --start-period` from `5s` to `30s` so a cold start on a
   resource-constrained container is not flagged unhealthy before the server
   has bound its listening port.

## Boundary justification

`docs/contracts/module-boundaries.md` states that the Delivery layer
"Dispatches typed requests to application commands/queries." The new `health`
subcommand is an exception: it performs HTTP loopback to the delivery layer
rather than dispatching to an application command or query.

The exception is justified on two grounds:

- **Health probes are infrastructure, not business logic.** Liveness and
  readiness describe the process and its ability to serve traffic; they do not
  mutate domain state or query application data. A loopback HTTP GET is the
  correct primitive because the probe runs in a separate process and can only
  observe the server through its published HTTP surface.
- **The `doctor` subcommand already sets the precedent.** `doctor`
  (`apps/cli/src/commands.rs:178-215`) performs local diagnostics, version
  reporting, and directory checks without dispatching to an application
  command. The `health` subcommand follows the same infrastructure-probe
  pattern, differing only in that it probes a running process over HTTP rather
  than inspecting the local filesystem.

The naming authority (the subcommand list in `AGENTS.md` and
`docs/plan/00-engineering-constitution.md`) must be amended to sanction
`health` as a recognized subcommand.

## Consequences

- Image size drops to roughly 55-65MB uncompressed, down from about 80MB, by
  removing `curl` and the doc/man trees.
- `curl` is no longer a runtime dependency; the only extra runtime package is
  `ca-certificates`, which the server needs for outbound HTTPS source fetching.
- `apt` and shell remain in the image, so operators can attach with
  `docker exec` and run diagnostics without rebuilding.
- The `HEALTHCHECK` uses the same binary that serves traffic, so health
  monitoring is decoupled from the base image choice. Future base image
  changes do not require re-adding a probe tool.
- The `health` subcommand must be added to the naming authority
  (`AGENTS.md` and `docs/plan/00-engineering-constitution.md`) for the
  constitution to remain accurate. That amendment is tracked as todo T6 of the
  docker-trixie-healthcheck plan.

## Alternatives considered

1. **Extend `doctor` instead of adding a new `health` verb** — rejected:
   `doctor` is a one-shot diagnostic report covering version, database, and
   directory state. `health live/ready` carries Kubernetes-style
   liveness/readiness semantics with a distinct exit-code contract (0 on
   healthy, 1 on degraded or unreachable) designed for Docker `HEALTHCHECK`.
   Folding the two into one verb would conflate a human-readable diagnostic
   report with a machine-readable probe contract.
2. **Alpine base image** — rejected: musl libc instead of glibc. Per
   Constraint #18, a base image switch that changes the libc requires a
   round-trip spike validating that every workspace crate and its transitive
   dependencies behave correctly under musl. The runtime already has zero
   non-glibc dynamic dependencies, so alpine offers no meaningful savings and
   introduces a spike cost.
3. **Distroless runtime** — rejected: no shell, no package manager. Violates
   the debuggability priority; operators cannot attach with `docker exec` to
   investigate a running container.
4. **Keep `curl` for the healthcheck** — rejected: adds roughly 8MB to the
   runtime image and introduces an external dependency for a single HTTP GET
   that the CLI binary can perform itself.

## References

- `AGENTS.md` Constraint #11 — no `latest` image as a production release
  dependency. Both `debian:trixie-slim` and `rust:1.97.1-trixie` are pinned by
  tag, not by `latest`.
- `AGENTS.md` Constraint #18 — compatibility conclusions require client
  validation or official format. Cited in the alpine rejection above.
- `docs/contracts/module-boundaries.md` — CLI boundary contract that the
  `health` subcommand excepts, justified above.
- `apps/cli/src/commands.rs:178-215` — existing `doctor` command, the
  precedent for an infrastructure-probe CLI subcommand.
- `apps/server/src/routes.rs:117-174` — `/health/live` and `/health/ready`
  endpoints that the `health` subcommand probes via HTTP loopback.
