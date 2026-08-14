//! Deve Sub — Dioxus Web frontend entry point.
//!
//! On WASM targets (`target_family = "wasm"`), the full Dioxus frontend is
//! compiled from `main_wasm.rs` via `include!`.
//!
//! On native targets, this bin target exits with a clear error. The frontend
//! must be built separately via `dx build --release --package deve-sub-web`
//! for `wasm32-unknown-unknown` and served from `apps/web/dist/`. The host
//! `cargo`/`clippy`/`test` invocation under `--all-features` still compiles
//! this stub so the workspace gate stays green, but no real UI runs on host.
//!
//! See ADR-0001 for the frontend mode decision.

#![allow(clippy::expect_used)]

#[cfg(target_family = "wasm")]
include!("main_wasm.rs");

#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!(
        "deve-sub-web is a WebAssembly binary and must be built with \
         --target wasm32-unknown-unknown."
    );
    eprintln!(
        "Use `dx build --release --package deve-sub-web` to build the \
         frontend, or run the server with web_dist_dir pointing at the \
         compiled dist."
    );
    std::process::exit(1);
}
