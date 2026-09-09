# Deploying and Updating Deve Sub

The release provides Linux amd64/arm64 binaries, matching Web assets and a
multi-platform Docker image. Pin the release version for repeatable deployment.
See the [M8 blueprint](../plan/milestones/M8-deployment-and-hardening.md) and
[artifact contract](../contracts/release-artifacts.md) for the authoritative
behavior and file formats.

The native installer installs both the binary and Web UI, configures systemd,
and waits for readiness. Rerun the installer to update the complete native
installation. Docker upgrades replace the complete versioned image.

`deve-sub update` replaces the native binary and checks a signed manifest by
default. `--force` does not disable signature verification. The explicit
`--allow-unsigned` option is for manually trusted development release sources;
it accepts checksum-only metadata and does not authenticate the publisher.
It never bypasses a malformed, partial or invalid signature. Native Web assets
are not replaced by this command.

Acceptance: DEPLOY-001 through DEPLOY-005 and UPDATE-001/002. Check the acceptance
matrix for executed evidence; implemented deployment files alone do not prove
that every deployment platform has been exercised.
