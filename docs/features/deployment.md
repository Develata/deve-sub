# Deploying and Updating Deve Sub

The release provides Linux amd64/arm64 binaries, matching Web assets and a
multi-platform Docker image. Pin the release version for repeatable deployment.
See the [M8 blueprint](../plan/milestones/M8-deployment-and-hardening.md) and
[artifact contract](../contracts/release-artifacts.md) for the authoritative
behavior and file formats.

The native installer installs both the binary and Web UI, configures systemd,
and waits for readiness. Rerun the installer to update the complete native
installation. Existing databases are not automatically migrated during native
upgrade: back up first and explicitly migrate when a new release requires a
schema change. Failed or interrupted installation retains a recovery checkpoint
at `/var/tmp/deve-sub-install.pending`; inspect its private backup directory and
restore the previous unit/binary/Web before clearing it. An old process must
pass readiness and version checks before automatic rollback removes backups.
Docker upgrades replace the complete versioned image.

Runtime rotation and audit retention/manual cleanup are documented in
[日志管理](logging.md). Compose bounds its own runtime logs; database audit history
defaults to 90 days in builds containing the M10 lifecycle extension (0 disables).

Subscription-source refresh and traffic-panel sync connect directly to the
addresses checked by their SSRF guard. They do not inherit `HTTP_PROXY`,
`HTTPS_PROXY` or `ALL_PROXY`; proxy-side DNS resolution would invalidate the
checked destination. Deployments must provide direct reachability to these
sources/panels. This does not change client proxy configurations generated for
subscribers, or explicitly configured real-proxy node tests.

`deve-sub update` replaces the native binary and checks a signed manifest by
default. With Web serving enabled (including default configuration), it refuses
before downloading unless `--binary-only` explicitly accepts frontend version
skew. `--force` allows same-version reinstall; downgrade requires
`--allow-downgrade`. None of these flags disables signature verification. The explicit
`--allow-unsigned` option is for manually trusted development release sources;
it accepts checksum-only metadata and does not authenticate the publisher.
It never bypasses a malformed, partial or invalid signature. Native Web assets
are not replaced by this command.

Acceptance: DEPLOY-001 through DEPLOY-005 and UPDATE-001/002. Check the acceptance
matrix for executed evidence; implemented deployment files alone do not prove
that every deployment platform has been exercised.

## Docker Compose

Optional administrator bootstrap: set `DEVE_SUB_ADMIN_USERNAME` and
`DEVE_SUB_ADMIN_PASSWORD` in the Compose directory's `.env`; the Compose file
passes them to the container. Use single quotes around passwords containing
`$` to prevent Compose interpolation. The entrypoint creates the first admin
before serving HTTP and removes both variables from the server's environment.
Use the configured credentials to log in directly. Keep `.env` private and
remove the bootstrap settings after creation if no longer needed.

Both variables unset/empty preserve Web setup. Partial/invalid credentials
prevent startup on an empty database. Any existing user makes initialization a
no-op, including changed credentials on container recreation; this cannot
reset passwords or re-enable disabled accounts. The shared application command
hashes the password with Argon2id and atomically permits only the first user.
The [artifact contract](../contracts/release-artifacts.md#container-administrator-bootstrap)
owns exact CLI and environment semantics. Existing `v0.1.0` images do not
implement this feature; use a source build until a containing release ships.

The [repository Compose file](../../docker-compose.yml) pulls
`ghcr.io/develata/deve-sub:v0.1.0`, including the binary and Web UI. Save it in a
`deve-sub` directory and run `docker compose pull` followed by
`docker compose up -d` there. No source checkout or local build is required.
Compose selects `linux/amd64` or `linux/arm64` for the host. The image entrypoint
initializes the key on first boot, runs migrations and starts the server.

The named volume at `/app/data` contains both the database and master key.
Back up both before upgrading, edit the explicit image version, then run
`docker compose pull` and `docker compose up -d` in the same directory/project.
Keep the project name and volume mapping unchanged when switching an existing
source-built deployment to the published image. `docker compose down` preserves
the volume; `down --volumes` deletes it. An older image cannot reverse database
migrations. Versioned image tags include the leading `v`.

To follow stable releases, set `DEVE_SUB_IMAGE_TAG=latest` in the Compose
directory's `.env`, then run `docker compose pull` and `docker compose up -d`.
The persisted setting applies to both commands; changing only the environment
of `pull` would leave a later `up` using the default version. Use a specific
tag in `.env` to pin again. `docker pull ghcr.io/develata/deve-sub:latest` also
works independently of Compose but does not replace running containers.
The alias advances only after the current stable release's versioned image
has been published; prereleases and old-release reruns do not advance it.
Initial rollout: the existing `v0.1.0` image has not yet received this alias.
It becomes available after the first stable tag release using the new workflow
or an authorized registry backfill. A source push, merge or manual preflight
alone does not publish it; keep the default fixed version until then.

For a source build, check out the desired release tag, or a branch/commit that
contains the administrator bootstrap changes while they are unreleased
(`fix/compose-published-image` currently contains them; `v0.1.0` does not).
Replace the Compose service's `image: ...` line with `build: .`
(older tags may already use `build: .`), then run
`docker compose up -d --build`. This compiles Rust and Web assets locally and
requires the full checkout. Release-tag Compose files are historical snapshots;
the standalone image example in [README](../../README.md#quick-start) also works
when an older tag's Compose file still defaults to a build.
