//! Capability-scoped projections of composition state.
//!
//! Only wiring sees AppState; handlers receive their route family's Ports.
//! There is intentionally no conversion from a capability back to root state.
use crate::AppState;
use axum::extract::FromRef;
use deve_sub_application::{
    DbHealthPort, GeoIpPort, JobSupervisor, LoginRateLimiter, SubscriptionFetcher,
};
use deve_sub_domain::*;
use deve_sub_security::MasterKey;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Ports and configuration needed by the auth surface.
#[derive(Clone)]
pub struct AuthState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
    pub(crate) config: deve_sub_application::AppConfig,
    pub(crate) master_key: Arc<MasterKey>,
    pub(crate) rate_limiter: Arc<dyn LoginRateLimiter>,
    pub(crate) recovery_code_repo: Arc<dyn RecoveryCodeRepository>,
    pub(crate) session_repo: Arc<dyn SessionRepository>,
    pub(crate) totp_secret_repo: Arc<dyn TotpSecretRepository>,
    pub(crate) user_repo: Arc<dyn UserRepository>,
}

impl FromRef<AppState> for AuthState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
            config: state.config.clone(),
            master_key: state.master_key.clone(),
            rate_limiter: state.rate_limiter.clone(),
            recovery_code_repo: state.recovery_code_repo.clone(),
            session_repo: state.session_repo.clone(),
            totp_secret_repo: state.totp_secret_repo.clone(),
            user_repo: state.user_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the user surface.
#[derive(Clone)]
pub struct UserState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
    pub(crate) session_repo: Arc<dyn SessionRepository>,
    pub(crate) user_repo: Arc<dyn UserRepository>,
}

impl FromRef<AppState> for UserState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
            session_repo: state.session_repo.clone(),
            user_repo: state.user_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the source surface.
#[derive(Clone)]
pub struct SourceState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
    pub(crate) fetcher: Arc<dyn SubscriptionFetcher>,
    pub(crate) geoip: Arc<dyn GeoIpPort>,
    pub(crate) job_supervisor: Arc<JobSupervisor>,
    pub(crate) pool_repo: Arc<dyn NodePoolRepository>,
    pub(crate) refresh_cancel_flags:
        Arc<Mutex<HashMap<deve_sub_kernel::SourceRefreshJobId, Arc<AtomicBool>>>>,
    pub(crate) refresh_job_repo: Arc<dyn SourceRefreshJobRepository>,
    pub(crate) snapshot_repo: Arc<dyn SourceSnapshotRepository>,
    pub(crate) source_repo: Arc<dyn SourceRepository>,
}

impl FromRef<AppState> for SourceState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
            fetcher: state.fetcher.clone(),
            geoip: state.geoip.clone(),
            job_supervisor: state.job_supervisor.clone(),
            pool_repo: state.pool_repo.clone(),
            refresh_cancel_flags: state.refresh_cancel_flags.clone(),
            refresh_job_repo: state.refresh_job_repo.clone(),
            snapshot_repo: state.snapshot_repo.clone(),
            source_repo: state.source_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the node surface.
#[derive(Clone)]
pub struct NodeState {
    pub(crate) override_repo: Arc<dyn NodeOverrideRepository>,
    pub(crate) pool_repo: Arc<dyn NodePoolRepository>,
}

impl FromRef<AppState> for NodeState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            override_repo: state.override_repo.clone(),
            pool_repo: state.pool_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the template surface.
#[derive(Clone)]
pub struct TemplateState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
    pub(crate) cache_repo: Arc<dyn GenerationCacheRepository>,
    pub(crate) pool_meta_repo: Arc<dyn PoolMetaRepository>,
    pub(crate) pool_repo: Arc<dyn NodePoolRepository>,
    pub(crate) template_repo: Arc<dyn TemplateRepository>,
    pub(crate) version_repo: Arc<dyn TemplateVersionRepository>,
}

impl FromRef<AppState> for TemplateState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
            cache_repo: state.cache_repo.clone(),
            pool_meta_repo: state.pool_meta_repo.clone(),
            pool_repo: state.pool_repo.clone(),
            template_repo: state.template_repo.clone(),
            version_repo: state.version_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the subscription surface.
#[derive(Clone)]
pub struct SubscriptionState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
    pub(crate) master_key: Arc<MasterKey>,
    pub(crate) short_code_repo: Arc<dyn ShortCodeRepository>,
    pub(crate) subscription_repo: Arc<dyn SubscriptionRepository>,
    pub(crate) subscription_token_repo: Arc<dyn SubscriptionTokenRepository>,
    pub(crate) temp_link_repo: Arc<dyn TempLinkRepository>,
    pub(crate) traffic_repo: Arc<dyn TrafficRepository>,
}

impl FromRef<AppState> for SubscriptionState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
            master_key: state.master_key.clone(),
            short_code_repo: state.short_code_repo.clone(),
            subscription_repo: state.subscription_repo.clone(),
            subscription_token_repo: state.subscription_token_repo.clone(),
            temp_link_repo: state.temp_link_repo.clone(),
            traffic_repo: state.traffic_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the delivery surface.
#[derive(Clone)]
pub struct DeliveryState {
    pub(crate) cache_repo: Arc<dyn GenerationCacheRepository>,
    pub(crate) master_key: Arc<MasterKey>,
    pub(crate) pool_meta_repo: Arc<dyn PoolMetaRepository>,
    pub(crate) pool_repo: Arc<dyn NodePoolRepository>,
    pub(crate) short_code_repo: Arc<dyn ShortCodeRepository>,
    pub(crate) subscription_repo: Arc<dyn SubscriptionRepository>,
    pub(crate) subscription_token_repo: Arc<dyn SubscriptionTokenRepository>,
    pub(crate) temp_link_repo: Arc<dyn TempLinkRepository>,
    pub(crate) template_repo: Arc<dyn TemplateRepository>,
    pub(crate) traffic_repo: Arc<dyn TrafficRepository>,
    pub(crate) user_repo: Arc<dyn UserRepository>,
    pub(crate) version_repo: Arc<dyn TemplateVersionRepository>,
}

impl FromRef<AppState> for DeliveryState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            cache_repo: state.cache_repo.clone(),
            master_key: state.master_key.clone(),
            pool_meta_repo: state.pool_meta_repo.clone(),
            pool_repo: state.pool_repo.clone(),
            short_code_repo: state.short_code_repo.clone(),
            subscription_repo: state.subscription_repo.clone(),
            subscription_token_repo: state.subscription_token_repo.clone(),
            temp_link_repo: state.temp_link_repo.clone(),
            template_repo: state.template_repo.clone(),
            traffic_repo: state.traffic_repo.clone(),
            user_repo: state.user_repo.clone(),
            version_repo: state.version_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the probe surface.
#[derive(Clone)]
pub struct ProbeState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
    pub(crate) cancelled_flags: Arc<Mutex<HashMap<deve_sub_kernel::ProbeRunId, Arc<AtomicBool>>>>,
    pub(crate) job_supervisor: Arc<JobSupervisor>,
    pub(crate) latency_repo: Arc<dyn LatencyRecordRepository>,
    pub(crate) pool_repo: Arc<dyn NodePoolRepository>,
    pub(crate) probe_adapter: Arc<dyn ProbeSourceAdapter>,
    pub(crate) probe_run_repo: Arc<dyn ProbeRunRepository>,
    pub(crate) probe_source_repo: Arc<dyn ProbeSourceRepository>,
    pub(crate) quic_probe: Arc<dyn LatencyProbe>,
    pub(crate) real_proxy_probe: Arc<dyn LatencyProbe>,
    pub(crate) tcp_probe: Arc<dyn LatencyProbe>,
}

impl FromRef<AppState> for ProbeState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
            cancelled_flags: state.cancelled_flags.clone(),
            job_supervisor: state.job_supervisor.clone(),
            latency_repo: state.latency_repo.clone(),
            pool_repo: state.pool_repo.clone(),
            probe_adapter: state.probe_adapter.clone(),
            probe_run_repo: state.probe_run_repo.clone(),
            probe_source_repo: state.probe_source_repo.clone(),
            quic_probe: state.quic_probe.clone(),
            real_proxy_probe: state.real_proxy_probe.clone(),
            tcp_probe: state.tcp_probe.clone(),
        }
    }
}

/// Ports and configuration needed by the dashboard surface.
#[derive(Clone)]
pub struct DashboardState {
    pub(crate) latency_repo: Arc<dyn LatencyRecordRepository>,
    pub(crate) probe_source_repo: Arc<dyn ProbeSourceRepository>,
    pub(crate) traffic_daily_snapshot_repo: Arc<dyn TrafficDailySnapshotRepository>,
    pub(crate) traffic_repo: Arc<dyn TrafficRepository>,
}

impl FromRef<AppState> for DashboardState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            latency_repo: state.latency_repo.clone(),
            probe_source_repo: state.probe_source_repo.clone(),
            traffic_daily_snapshot_repo: state.traffic_daily_snapshot_repo.clone(),
            traffic_repo: state.traffic_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the audit surface.
#[derive(Clone)]
pub struct AuditState {
    pub(crate) audit_log_repo: Arc<dyn AuditLogRepository>,
}

impl FromRef<AppState> for AuditState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            audit_log_repo: state.audit_log_repo.clone(),
        }
    }
}

/// Ports and configuration needed by the health surface.
#[derive(Clone)]
pub struct HealthState {
    pub(crate) config: deve_sub_application::AppConfig,
    pub(crate) db_health: Arc<dyn DbHealthPort>,
}

impl FromRef<AppState> for HealthState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            config: state.config.clone(),
            db_health: state.db_health.clone(),
        }
    }
}
