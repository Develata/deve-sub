# Docker Build Spike Report

## Summary

**Result: DESIGN VALIDATED — build not executed (no Docker daemon access)**

The Dockerfile is structurally complete with all three stages active:
Rust builder, Dioxus Web builder, and minimal non-root runtime. The Rust
spike binary compiles and runs locally. The Docker image build was not
executed due to Docker daemon permission restrictions in this environment.

Per M0 rule "`planned` is not pass", this spike is **design-validated, not
build-verified**. The image build must be executed in CI or a
Docker-accessible environment to fully close the gate.

## Dockerfile structure

### Stage 1: Rust builder (`rust:1.87-bookworm`)

- Compiles release binary with `opt-level="s"`, LTO, strip, `panic="abort"`.
- Result: dynamically linked Rust binary (~300 KB).

### Stage 2: Dioxus Web builder (`rust:1.87-bookworm`)

- Installs `wasm32-unknown-unknown` target and `dx` CLI (v0.7, locked).
- Downloads Tailwind CSS v4.3.3 standalone binary and compiles CSS.
- Runs `dx build --release` to produce static web assets.
- Output: `target/dx/dioxus-ui-spike/release/web/public/`
- **Build context must be repository root** for `COPY spikes/dioxus-ui/`:
  ```bash
  docker build -f spikes/docker-build/Dockerfile .
  ```

### Stage 3: Runtime (`debian:bookworm-slim`)

- Installs only `ca-certificates` (no other runtime deps).
- Creates non-root user `deve` (UID 1000, GID 1000, home `/home/deve`).
- Copies binary and web assets with `--chown=deve:deve`.
- Sets `USER deve` — process runs as non-root.
- Exposes port 8080.

## Multi-arch build

```bash
# Create buildx builder (one-time)
docker buildx create --name multiarch --use

# Build for amd64 and arm64 (requires registry for --push)
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f spikes/docker-build/Dockerfile \
  -t ghcr.io/OWNER/deve-sub:spike \
  --push \
  .
```

**Note**: `--push` requires a registry reference (e.g. `ghcr.io/OWNER/deve-sub`).
A bare tag like `deve-sub:spike` without a registry prefix will fail.

For local single-arch testing:
```bash
docker build -f spikes/docker-build/Dockerfile -t deve-sub:spike .
docker run --rm -p 8080:8080 deve-sub:spike
curl http://localhost:8080/
# {"app":"deve-sub","spike":"docker-build","version":"0.0.0"}
```

## Expected image characteristics

| Property          | Value                          |
|-------------------|--------------------------------|
| Base image        | debian:bookworm-slim           |
| Runtime user      | deve (UID 1000)                |
| Binary size       | ~300 KB (stripped, LTO, opt-s) |
| Image size        | ~80 MB (bookworm-slim + certs) |
| Exposed port      | 8080                           |
| Entry point       | /app/deve-sub                  |

## Gate item coverage

| Gate item (M0) | Status | Evidence |
|----------------|--------|----------|
| Multi-stage Rust + Dioxus Web builder | **DESIGN VALIDATED** | Dockerfile has 3 active stages: rust-builder, web-builder (dx CLI + Tailwind), runtime. Build not executed. |
| Minimal runtime image, non-root user | **DESIGN VALIDATED** | `debian:bookworm-slim` + `ca-certificates` only; `USER deve` (UID 1000); `--chown` on COPY. Build not executed. |
| amd64 and arm64 targets | **NOT VERIFIED** | `buildx` command documented but never executed. Requires Docker daemon access. |

## Environment limitation

Docker daemon is present but not accessible (permission denied on
`/var/run/docker.sock`). The Dockerfile is validated by:
- Rust source compiles and runs locally (`cargo check` + `cargo run` pass).
- Dockerfile syntax follows Docker best practices.
- Multi-stage pattern is well-established for Rust + WASM builds.
- Non-root user creation is standard Debian pattern.

Actual image build must be run in CI or a Docker-accessible environment to
fully close the gate.
