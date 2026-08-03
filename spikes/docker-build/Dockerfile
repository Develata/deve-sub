# syntax=docker/dockerfile:1

# ── Stage 1: Rust builder ────────────────────────────────────────────
FROM rust:1.87-bookworm AS rust-builder

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# ── Stage 2: Dioxus Web builder ──────────────────────────────────────
# Builds the Dioxus Web frontend (spikes/dioxus-ui) to static assets.
# Build context must be the repository root for this COPY to work:
#   docker build -f spikes/docker-build/Dockerfile .
FROM rust:1.87-bookworm AS web-builder

RUN rustup target add wasm32-unknown-unknown
RUN cargo install dioxus-cli --version 0.7 --locked

COPY spikes/dioxus-ui/ /build/frontend/
WORKDIR /build/frontend

# Tailwind CSS standalone binary
RUN curl -sLO https://github.com/tailwindlabs/tailwindcss/releases/download/v4.3.3/tailwindcss-linux-x64 \
    && chmod +x tailwindcss-linux-x64 \
    && ./tailwindcss-linux-x64 -i src/styles/input.css -o src/styles/tailwind.css

RUN dx build --release

# ── Stage 3: Minimal runtime ─────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user (UID 1000)
RUN groupadd --gid 1000 deve \
    && useradd --uid 1000 --gid deve --shell /bin/sh --home-dir /home/deve --create-home deve

WORKDIR /app

# Copy binary from builder with correct ownership
COPY --from=rust-builder --chown=deve:deve /build/target/release/docker-build-spike /app/deve-sub

# Copy web assets from Dioxus builder
COPY --from=web-builder --chown=deve:deve /build/frontend/target/dx/dioxus-ui-spike/release/web/public /app/web

USER deve

EXPOSE 8080

ENTRYPOINT ["/app/deve-sub"]
