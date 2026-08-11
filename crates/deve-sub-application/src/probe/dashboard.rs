//! Dashboard application queries: aggregated latency and traffic views for the
//! admin dashboard.
//!
//! These queries compose probe source state with traffic attribution and
//! recent latency records. They are read-only. See
//! `docs/plan/milestones/M7-probes-and-detection.md` §"Traffic aggregation".

use deve_sub_domain::{
    LatencyRecord, LatencyRecordRepository, ProbeSource, ProbeSourceKind, ProbeSourceRepository,
    SyncStatus, TrafficRepository, TrafficSummary,
};
use deve_sub_kernel::{SubscriptionId, Timestamp};

use super::error::ProbeAppError;

/// Aggregated traffic summary for the dashboard, with per-source-kind and
/// per-probe-source breakdown.
#[derive(Debug, Clone)]
pub struct DashboardTrafficAggregate {
    /// Global totals across all subscriptions.
    pub summary: TrafficSummary,
    /// Per-probe-source breakdown: (source_id, subscription_id, kind, name,
    /// enabled, upload, download). Attribution is via `source_ref` prefix
    /// matching against the probe source's `kind` kebab name.
    pub by_probe_source: Vec<ProbeSourceTrafficContribution>,
}

/// One probe source's traffic contribution to a subscription.
#[derive(Debug, Clone)]
pub struct ProbeSourceTrafficContribution {
    pub source_id: deve_sub_kernel::ProbeSourceId,
    pub subscription_id: SubscriptionId,
    pub kind: ProbeSourceKind,
    pub name: String,
    pub enabled: bool,
    pub upload: u64,
    pub download: u64,
    pub last_sync_at: Option<Timestamp>,
    pub last_sync_status: Option<SyncStatus>,
}

/// Build the dashboard traffic aggregate: global totals, per-source-kind
/// breakdown, and per-probe-source attribution (PROBE-005).
///
/// # Errors
/// - [`ProbeAppError::Domain`] — traffic or probe source repository failure.
pub async fn build_dashboard_traffic(
    traffic_repo: &dyn TrafficRepository,
    probe_source_repo: &dyn ProbeSourceRepository,
) -> Result<DashboardTrafficAggregate, ProbeAppError> {
    let summary = traffic_repo
        .get_global_summary()
        .await
        .map_err(|e| ProbeAppError::Traffic(e.to_string()))?;

    let attributions = traffic_repo
        .get_probe_traffic_attributions()
        .await
        .map_err(|e| ProbeAppError::Traffic(e.to_string()))?;

    let mut sources: Vec<ProbeSource> = Vec::new();
    let mut cursor: Option<deve_sub_kernel::ProbeSourceId> = None;
    loop {
        let page = probe_source_repo
            .list(cursor, 200, None)
            .await
            .map_err(ProbeAppError::Domain)?;
        if page.is_empty() {
            break;
        }
        let next_cursor = page.last().map(|s| s.id);
        sources.extend(page);
        cursor = next_cursor;
    }

    let mut contributions = Vec::new();
    for source in &sources {
        let Some(sub_id) = source.subscription_id else {
            continue;
        };
        let prefix = source.kind.as_kebab().to_owned();
        let (upload, download) = attributions
            .iter()
            .filter(|(s, p, _, _)| *s == sub_id && *p == prefix)
            .fold((0u64, 0u64), |(u, d), (_, _, up, down)| {
                (u.saturating_add(*up), d.saturating_add(*down))
            });
        if upload == 0 && download == 0 {
            continue;
        }
        contributions.push(ProbeSourceTrafficContribution {
            source_id: source.id,
            subscription_id: sub_id,
            kind: source.kind,
            name: source.name.clone(),
            enabled: source.enabled,
            upload,
            download,
            last_sync_at: source.last_sync_at,
            last_sync_status: source.last_sync_status.clone(),
        });
    }

    Ok(DashboardTrafficAggregate {
        summary,
        by_probe_source: contributions,
    })
}

/// List recent latency records across all nodes for the dashboard.
///
/// # Errors
/// - [`ProbeAppError::Domain`] — latency record repository failure.
pub async fn list_recent_latency(
    latency_repo: &dyn LatencyRecordRepository,
    limit: u32,
) -> Result<Vec<LatencyRecord>, ProbeAppError> {
    latency_repo
        .list_recent(limit)
        .await
        .map_err(ProbeAppError::Domain)
}
