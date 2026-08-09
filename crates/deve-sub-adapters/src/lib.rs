//! Adapter implementations for Deve Sub: HTTP fetcher, GeoIP, probe, files.
//!
//! Adapters implement Port traits defined in the domain and application
//! layers. They contain I/O code (HTTP, DNS, files) and no business rules.
//! See `docs/plan/04-workspace-layout.md` and
//! `docs/contracts/module-boundaries.md`.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod geoip;
pub mod http_fetcher;
pub mod quic_probe;
pub mod tcp_probe;

pub use geoip::MaxMindGeoIp;
pub use http_fetcher::{HttpFetcher, ProductionSsrfChecker, SsrfChecker};
pub use quic_probe::QuicHandshakeProbe;
pub use tcp_probe::TcpConnectProbe;
