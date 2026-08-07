//! Domain model: the canonical node model, protocol kinds, identity, and
//! aggregate invariants.
//!
//! This crate depends only on [`deve_sub_kernel`] and contains no I/O, no
//! framework types, and no database access. See ADR-0003 for the canonical
//! node model decision and `docs/plan/05-protocol-engine.md` for the full
//! protocol engine blueprint.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod endpoint;
pub mod error;
pub mod identity;
pub mod node;
pub mod protocol;
pub mod protocol_config;
pub mod source;
pub mod tls;
pub mod transport;

pub use endpoint::{DomainName, Endpoint, Host};
pub use error::DomainError;
pub use identity::{
    IdentityError, RecoveryCode, RecoveryCodeRepository, Role, Session, SessionRepository,
    TotpSecret, TotpSecretRepository, User, UserRepository,
};
pub use node::{Authentication, ChainTarget, Node, NodeSource, RegionAssignment, RegionMethod};
pub use protocol::{ProtocolConfig, ProtocolKind, UnsupportedNode};
pub use protocol_config::{
    Hysteria2Config, NaiveProxyConfig, ShadowsocksConfig, TrojanConfig, TuicV5Config, UdpRelayMode,
    VMessConfig, VlessRealityConfig,
};
pub use source::{
    ImportOutcome, ImportResult, ItemParseStatus, NodeFilter, NodePoolEntry, NodePoolRepository,
    ReconcileEntry, ReconcileInput, ReconcileResult, Source, SourceError, SourceItem,
    SourceRepository, SourceSnapshot, SourceSnapshotRepository, SourceType,
};
pub use tls::{CertificatePin, RealityConfig, TlsConfig};
pub use transport::{
    CongestionConfig, CongestionController, MultiplexConfig, Obfuscation, Transport, TransportKind,
    UdpCapability,
};
