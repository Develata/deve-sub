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
pub mod node_chain;
pub mod node_override;
pub mod probe;
pub mod protocol;
pub mod protocol_config;
pub mod source;
pub mod subscription;
pub mod template;
pub mod tls;
pub mod transport;

pub use endpoint::{DomainName, Endpoint, Host};
pub use error::DomainError;
pub use identity::{
    IdentityError, RecoveryCode, RecoveryCodeRepository, Role, Session, SessionRepository,
    TotpSecret, TotpSecretRepository, User, UserRepository,
};
pub use node::{Authentication, Node, NodeChain, NodeSource, RegionAssignment, RegionMethod};
pub use node_chain::{NodeChainEntry, NodeChainError, NodeChainGraph, NodeCyclePath};
pub use node_override::{NodeOverride, NodeOverrideRepository, Tag};
pub use probe::{
    ErrorClass, LatencyProbe, LatencyRecord, LatencyRecordRepository, LatencyResult, ProbeError,
    ProbeRun, ProbeRunRepository, ProbeRunResult, ProbeRunStatus, ProbeSource, ProbeSourceAdapter,
    ProbeSourceKind, ProbeSourceRepository, ProbeSyncResult, ProbeTrafficSample, ProbeType,
    SyncStatus,
};
pub use protocol::{ProtocolConfig, ProtocolKind, UnsupportedNode};
pub use protocol_config::{
    AnyTlsConfig, Hysteria2Config, NaiveProxyConfig, ShadowTlsConfig, ShadowTlsVersion,
    ShadowsocksConfig, SnellConfig, SnellObfs, SnellObfsMode, SnellV6Mode, SnellVersion,
    TrojanConfig, TuicV5Config, UdpRelayMode, VMessConfig, VlessRealityConfig, WireGuardConfig,
    WireGuardPeer,
};
pub use source::{
    ImportOutcome, ImportResult, ItemParseStatus, NodeFilter, NodePoolEntry, NodePoolRepository,
    PoolMetaRepository, ReconcileEntry, ReconcileInput, ReconcileResult, Source, SourceError,
    SourceFilterRules, SourceItem, SourceRepository, SourceSnapshot, SourceSnapshotRepository,
    SourceType,
};
pub use subscription::{
    ShortCode, ShortCodeRepository, Subscription, SubscriptionError, SubscriptionRepository,
    SubscriptionToken, SubscriptionTokenRepository, TempLink, TempLinkRepository, TrafficRecord,
    TrafficRepository, TrafficSourceKind, TrafficSummary,
};
pub use template::{
    API_VERSION, CacheKeyParams, ChainEdge, ChainGraph, ChainVertex, CompatibilityReport,
    CyclePath, ExcludedNode, FilterField, GenerationCacheEntry, GenerationCacheRepository,
    GenerationError, GenerationMode, GenerationRequest, GenerationResult, GroupMember,
    GroupResolution, GroupType, KIND, MAX_ALIAS_DEPTH, MAX_SPEC_BYTES, MissingNodeRef,
    MissingReason, NodeFilterRule, NodeSelector, ProxyGroup, QuickGroupFilter, Rule, SelectionMode,
    SortOrder, SubscriptionTemplate, TemplateDocument, TemplateError, TemplateMetadata,
    TemplateRepository, TemplateResolution, TemplateSpec, TemplateVersion,
    TemplateVersionRepository,
};
pub use tls::{CertificatePin, RealityConfig, TlsConfig};
pub use transport::{
    CongestionConfig, CongestionController, MultiplexConfig, Obfuscation, Transport, TransportKind,
    UdpCapability,
};
