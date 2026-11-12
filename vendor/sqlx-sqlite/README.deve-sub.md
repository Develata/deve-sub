# Temporary SQLx SQLite dependency backport

This is the official `sqlx-sqlite` 0.8.6 crate, with its MIT and Apache-2.0
licenses retained. Upstream executable Rust code is unchanged.

- Archive: https://static.crates.io/crates/sqlx-sqlite/sqlx-sqlite-0.8.6.crate
- SHA-256: `c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`
- Upstream commit: `bab1b022bd56a64f9a08b46b36b97c5cff19d77e`
- Upstream path: `sqlx-sqlite`

The upstream dependency edits change the `libsqlite3-sys` requirement in
`Cargo.toml` and `Cargo.toml.orig`: `0.30.1` becomes `=0.39.1`.
Trailing Markdown whitespace on three lines in `src/connection/serialize.rs` is removed
so the complete patch passes `git diff --check`; no executable code changes.
The root `[patch.crates-io]` selects this directory, and `Cargo.lock` pins
the remaining registry dependencies. SQLx's public API, SQLite feature
selection, and the database migrations stay unchanged.

RUSTSEC-2026-0459 affects the old bundled SQLite. `libsqlite3-sys` 0.39.1
bundles SQLite 3.54.1, including the WAL page-size fix. The exact requirement
keeps this temporary compatibility backport bound to the evaluated FFI
release. No advisory ignore or dependency-policy weakening is added.
The SQLCipher feature is not enabled or claimed fixed by this patch.

Remove this directory and the root patch when a compatible official SQLx
release selects a fixed SQLite dependency. Also remove Dockerfile's
`COPY vendor/ vendor/` if no other vendored packages remain. Regenerate the lockfile and rerun
dependency policy, storage, server/CLI, and Docker gates. The storage test
`sqlite_security_version` also queries the actually linked SQLite version.

Advisory: https://rustsec.org/advisories/RUSTSEC-2026-0459.html
