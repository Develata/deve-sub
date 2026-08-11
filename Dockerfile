# syntax=docker/dockerfile:1

# ── Stage 1: Rust builder ────────────────────────────────────────────
# WHY: Pinned Rust version matches CI (dtolnay/rust-toolchain@1.97.1). Trixie
# builder per ADR-0006.
# Constraint #11: no "latest" tag as a production release dependency.
FROM rust:1.97.1-trixie AS builder

WORKDIR /build

# Copy all workspace files needed for the build.
# NOTE: No separate dependency-prebuild layer; the workspace is small enough
# that a full rebuild on each source change is acceptable for M1. Use
# cargo-chem or dummy-src technique if compile time becomes a bottleneck.
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY apps/ apps/
COPY migrations/ migrations/

RUN cargo build --locked --release --bin deve-sub

# ── Stage 2: Minimal runtime ─────────────────────────────────────────
# WHY: trixie-slim is the current Debian stable (per ADR-0006), not "latest".
FROM debian:trixie-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* /usr/share/doc /usr/share/man

# Create non-root user (UID 1000, GID 1000).
RUN groupadd --gid 1000 deve \
    && useradd --uid 1000 --gid deve --shell /bin/sh --home-dir /home/deve --create-home deve

WORKDIR /app

# Copy the release binary from the builder stage.
COPY --from=builder --chown=deve:deve /build/target/release/deve-sub /app/deve-sub

# Create data directory with correct ownership for SQLite + WAL.
RUN mkdir -p /app/data && chown deve:deve /app/data

# Declare the data directory as a volume to prompt operators to mount
# persistent storage and prevent silent data loss on container removal.
VOLUME /app/data

USER deve

EXPOSE 8080

# Healthcheck: probe liveness via the internalized CLI health command.
# WHY: 127.0.0.1 instead of localhost to avoid IPv6 ::1 resolution ambiguity
# in minimal containers where the server binds 0.0.0.0 (IPv4 only).
# See ADR-0006 for the internalized-healthcheck rationale.
HEALTHCHECK --interval=30s --timeout=3s --start-period=30s --retries=3 \
    CMD ["/app/deve-sub", "health", "live"]

ENTRYPOINT ["/app/deve-sub"]
CMD ["serve", "--db-path", "/app/data/deve-sub.db"]
