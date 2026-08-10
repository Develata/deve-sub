//! Probe domain module: probe source, latency record, probe run, and port
//! traits.
//!
//! A `ProbeSource` is an external monitoring panel (Nezha, DStatus, Komari)
//! configured as a traffic data source. A `LatencyRecord` captures one node
//! latency measurement. A `ProbeRun` is a batch latency probing job executed
//! by the `ProbeRunner` (application layer). See
//! `docs/plan/milestones/M7-probes-and-detection.md`.

pub mod entity;
pub mod error;
pub mod ports;

pub use entity::{
    ErrorClass, LatencyRecord, LatencyResult, ProbeRun, ProbeRunResult, ProbeRunStatus,
    ProbeSource, ProbeSourceKind, ProbeSyncResult, ProbeTrafficSample, ProbeType, SyncStatus,
};
pub use error::ProbeError;
pub use ports::{
    LatencyProbe, LatencyRecordRepository, ProbeRunRepository, ProbeSourceAdapter,
    ProbeSourceRepository,
};
