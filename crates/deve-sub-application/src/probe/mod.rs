//! Probe application module: commands for probe source CRUD and probe run
//! lifecycle, plus the probe runner service.
//!
//! This module orchestrates domain services and port interfaces. It does not
//! execute SQL directly. See `docs/plan/03-architecture.md` §"Lightweight
//! CQRS" and `docs/plan/milestones/M7-probes-and-detection.md`.

pub mod commands;
pub mod error;
pub mod runner;

pub use commands::{
    CreateProbeSourceParams, StartProbeRunParams, SyncProbeTrafficResult, UpdateProbeSourceParams,
    cancel_probe_run, create_probe_source, delete_probe_source, get_probe_run, get_probe_source,
    list_probe_sources, mark_sync_failed, mark_sync_stale, recover_crashed_runs, start_probe_run,
    sync_probe_traffic, update_probe_source,
};
pub use error::ProbeAppError;
pub use runner::{
    DEFAULT_CONCURRENCY, DEFAULT_PROBE_TIMEOUT, RunnerConfig, RunnerDeps, execute_probe_run,
};
