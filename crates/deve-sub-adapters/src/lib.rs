//! Adapter implementations for Deve Sub: HTTP fetcher, GeoIP, probe, files.
//!
//! Adapters implement Port traits defined in the domain and application
//! layers. They contain I/O code (HTTP, DNS, files) and no business rules.
//! See `docs/plan/04-workspace-layout.md` and
//! `docs/contracts/module-boundaries.md`.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod geoip;
pub mod http_fetcher;
pub mod nezha_probe;
pub mod probe_registry;
pub mod quic_probe;
pub mod real_proxy;
pub mod tcp_probe;

pub use geoip::MaxMindGeoIp;
pub use http_fetcher::{HttpFetcher, PermissiveSsrfChecker, ProductionSsrfChecker, SsrfChecker};
pub use nezha_probe::NezhaProbeAdapter;
pub use probe_registry::ProbeSourceAdapterRegistry;
pub use quic_probe::QuicHandshakeProbe;
pub use real_proxy::RealProxyProbe;
pub use tcp_probe::TcpConnectProbe;
