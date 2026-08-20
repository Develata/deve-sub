# syntax=docker/dockerfile:1

# ── Stage 1: Rust + WASM builder ─────────────────────────────────────
# WHY: Builder always runs on $BUILDPLATFORM (amd64 in CI) for fast native
# compilation. Arm64 binaries are produced via cross-compilation with
# aarch64-unknown-linux-gnu, avoiding QEMU emulation which is 10-50x slower
# for Rust + C deps (ring, libsqlite3-sys). See ADR-0006.
# Constraint #11: no "latest" tag as a production release dependency.
FROM --platform=$BUILDPLATFORM rust:1.97.1-trixie AS builder

ARG TARGETARCH

# Install cross-compilation toolchain for arm64 targets.
# WHY: ring (crypto) and libsqlite3-sys (SQLite) compile C/assembly code via
# the cc crate, which needs a native C cross-compiler for the target arch.
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      apt-get update && \
      apt-get install -y --no-install-recommends \
        gcc-aarch64-linux-gnu g++-aarch64-linux-gnu && \
      rm -rf /var/lib/apt/lists/* && \
      rustup target add aarch64-unknown-linux-gnu; \
    fi

WORKDIR /build

# Copy all workspace files needed for the build.
# NOTE: No separate dependency-prebuild layer; the workspace is small enough
# that a full rebuild on each source change is acceptable for M1. Use
# cargo-chem or dummy-src technique if compile time becomes a bottleneck.
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY apps/ apps/
COPY migrations/ migrations/
COPY scripts/ scripts/

# Build the host release binary.
# For arm64 targets, cross-compile with aarch64-unknown-linux-gnu to avoid
# QEMU emulation. The binary is copied to target/release/deve-sub in both
# cases so the runtime stage COPY path is stable.
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc; \
      export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++; \
      export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar; \
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc; \
      cargo build --locked --release --bin deve-sub --target aarch64-unknown-linux-gnu && \
      mkdir -p target/release && \
      cp target/aarch64-unknown-linux-gnu/release/deve-sub target/release/deve-sub; \
    else \
      cargo build --locked --release --bin deve-sub; \
    fi

# Build the WASM frontend (DS-AUD-004): architecture-independent, always
# built natively on $BUILDPLATFORM. The build script normalizes Dioxus
# output to apps/web/dist/ and verifies the index.html + WASM + JS contract
# so the runtime image always has the real admin UI, not the placeholder
# (DS-AUD-007/053).
RUN rustup target add wasm32-unknown-unknown
RUN cargo install dioxus-cli --locked --version 0.7.10
RUN bash scripts/build-web-release.sh

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

# Copy the compiled web frontend dist (DS-AUD-004).
COPY --from=builder --chown=deve:deve /build/apps/web/dist /app/web/dist

# Copy the entrypoint script (DS-AUD-007: migrate before serve).
COPY --chmod=0755 --chown=deve:deve docker-entrypoint.sh /app/entrypoint.sh

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

# WHY: entrypoint runs migrate (idempotent) then execs serve with the
# correct web_dist_dir so `docker run` works standalone (DS-AUD-007).
ENTRYPOINT ["/app/entrypoint.sh"]
