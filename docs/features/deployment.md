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
