# Release Artifacts

## Scope and authority

This contract defines the M8 native distribution and updater boundary. Product
behavior is owned by `plan/milestones/M8-deployment-and-hardening.md`; CI artifact
identity is owned by `ci-evidence.md`.

## Version and assets

A release tag is `v` followed by the workspace Cargo SemVer. One release contains:

- `deve-sub-linux-amd64` and `deve-sub-linux-arm64`: executable native binaries.
- `deve-sub-web.tar.gz`: the same invocation's verified frontend, with
  `index.html` and nonempty JavaScript, WASM and CSS under `assets/`. The archive
  contains only relative regular files/directories, without parent traversal.
- `checksums.txt`: SHA-256 entries for both binaries and the frontend archive.
- `deve-sub-manifest.json` and `.json.sig`: manifest bytes and their raw 64-byte
  Ed25519 signature. The manifest has `version`, `target` and `assets`; each asset
  has `name`, `sha256`, and `size`. Asset names identify the native architecture;
  the legacy top-level `target` describes the primary binary, not every asset.
- `deve-sub-sbom.json` and `deve-sub-web-sbom.json`: CycloneDX inventories
  for the native CLI and frontend-enabled WASM binary, respectively.

Manual `workflow_dispatch` runs full CI, both native build/smoke jobs and
artifact assembly, including signature and SBOM generation, without creating
a tag, Release or image publication. Tag pushes repeat those gates before
publication.

Docker publishes the same version as a lowercase GHCR reference containing
both `linux/amd64` and `linux/arm64`; it never publishes a `latest` alias.

## Installation and update

The installer resolves a release tag once, verifies both downloaded payloads,
installs the binary and frontend, and passes the absolute frontend directory to
systemd. Success requires readiness plus the expected running version. An
installation failure stops the new process, restores the previous binary,
frontend and unit, and restarts a previously active service. Failed recovery
retains the backups for operator repair. The installer
bootstraps trust through HTTPS; its checksum is integrity evidence only.

`deve-sub update` requires valid signed assets by default. When **both** signed
assets are absent, `--allow-unsigned` permits a manually trusted development
source's `checksums.txt`; a missing flag, partial signature, invalid signature,
version mismatch, hash mismatch or size mismatch aborts before swapping the
binary. `--force` permits a same-version reinstall; downgrades require the separate
`--allow-downgrade` operator override. Neither bypasses authentication.
The updater replaces the binary only. With `server.serve_web=true` (the default),
it refuses before fetching or writing unless `--binary-only` explicitly accepts
version skew. An unreadable/invalid explicit config is an error. Use the installer
to update native Web assets or update the complete Docker image. No atomic
binary-plus-Web self-update or database rollback is claimed.

## Failure and verification

The workflow checks the signing secret against the embedded production public
key before signing. No private seed appears in source, fixtures, arguments or
logs. Missing keys, bad artifacts or failed gates cannot publish a release.

UPDATE-001/002 cover updater behavior; DEPLOY-002 covers native installation.
An isolated installer smoke verifies staging, assets and rollback but does not
replace actual systemd deployment evidence. Historical `not-run` cases remain
non-pass until that path is executed.
