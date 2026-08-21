//! Application layer for Deve Sub: commands, queries, and use cases.
//!
//! This crate orchestrates domain services and port interfaces. It does not
//! execute SQL directly or contain framework types. See
//! `docs/plan/03-architecture.md` for the application layer's position in the
//! hexagonal architecture and the lightweight CQRS pattern.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod audit;
pub mod auth;
pub mod config;
pub mod health;
pub mod job_supervisor;
pub mod probe;
pub mod source;
pub mod subscription;
pub mod template;

pub use auth::{AuthError, LoginRateLimiter};
pub use config::{AppConfig, IssueSeverity, ValidationIssue};
pub use health::{DbHealthPort, HealthError, HealthStatus, HealthView};
pub use job_supervisor::JobSupervisor;
pub use probe::{
    CreateProbeSourceParams, ProbeAppError, RunnerConfig, StartProbeRunParams,
    UpdateProbeSourceParams, cancel_probe_run, create_probe_source, delete_probe_source,
    execute_probe_run, get_probe_run, get_probe_source, list_probe_sources, recover_crashed_runs,
    start_probe_run, update_probe_source,
};
pub use source::{
    FetchError, FetchResult, GeoIpPort, ImportParseResult, ParseContentError, RefreshScheduler,
    SourceAppError, SubscriptionFetcher, parse_content, parse_for_import,
    recover_crashed_refresh_jobs, recover_stale_refresh_jobs,
};
pub use subscription::{
    CreateSubscriptionParams, CreateSubscriptionResult, GraceTokenCleanupScheduler,
    ManualCorrectionParams, RecordTrafficParams, RotateTokenResult, SubscriptionAppError,
    TrafficDailySnapshotScheduler, TrafficHistoryPoint, UpdateSubscriptionParams,
    aggregate_daily_traffic, apply_manual_correction, create_subscription, delete_subscription,
    get_subscription, get_traffic_summary, list_subscriptions,
    list_traffic_history_for_subscription, list_traffic_history_global, record_traffic,
    rotate_token, update_subscription,
};
pub use template::{
    CreateTemplateParams, CreateTemplateResult, TemplateAppError, UpdateTemplateParams,
    UpdateTemplateResult, create_template, delete_template, generate, get_active_generation,
    get_active_version, get_template, get_template_by_name, list_templates, list_versions, preview,
    rollback_template, update_template,
};
