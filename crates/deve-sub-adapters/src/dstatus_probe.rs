//! DStatus monitoring panel traffic sync adapter.
//!
//! Implements [`ProbeSourceAdapter`] for the DStatus panel. Calls
//! `GET {endpoint}/api/allnode_status` (anonymous public API), parses
//! per-node billing-cycle `traffic_stats.used` counters, computes deltas
//! against the last snapshot, and returns [`ProbeTrafficSample`] values.
//!
//! DStatus reports a single cumulative `used` value per node (total bytes
//! consumed in the current billing cycle), not split upload/download. The
//! adapter normalizes this to `upload = 0, download = delta`.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port" and PROBE-002.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{
    ProbeError, ProbeSource, ProbeSourceAdapter, ProbeSyncResult, ProbeTrafficSample,
};
use deve_sub_kernel::Timestamp;
use serde::Deserialize;

use crate::SsrfChecker;
use crate::probe_common::{build_ssrf_client, read_error_body};

#[derive(Deserialize)]
struct DStatusResponse {
    #[allow(dead_code)]
    success: bool,
    data: HashMap<String, DStatusNodeData>,
}

#[derive(Deserialize)]
struct DStatusNodeData {
    #[allow(dead_code)]
    name: String,
    traffic_stats: DStatusTrafficStats,
}

#[derive(Deserialize)]
struct DStatusTrafficStats {
    used: u64,
    #[allow(dead_code)]
    limit: u64,
    #[allow(dead_code)]
    unlimited: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct DStatusSnapshot {
    nodes: HashMap<String, DStatusEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DStatusEntry {
    used: u64,
}

pub struct DStatusProbeAdapter {
    ssrf: Arc<dyn SsrfChecker>,
}

impl Default for DStatusProbeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DStatusProbeAdapter {
    /// Create a new DStatus adapter with production SSRF protection.
    #[must_use]
    pub fn new() -> Self {
        Self::with_checker(Arc::new(crate::ProductionSsrfChecker))
    }

    /// Create a new DStatus adapter with a custom SSRF checker (for testing).
    #[must_use]
    pub fn with_checker(ssrf: Arc<dyn SsrfChecker>) -> Self {
        Self { ssrf }
    }

    fn parse_snapshot(source: &ProbeSource) -> Result<DStatusSnapshot, ProbeError> {
        match &source.last_counter_snapshot {
            None => Ok(DStatusSnapshot::default()),
            Some(json) => serde_json::from_str(json).map_err(|e| {
                ProbeError::ProbeFailed(format!("counter snapshot parse failed: {e}"))
            }),
        }
    }

    async fn fetch_allnode_status(&self, endpoint: &str) -> Result<DStatusResponse, ProbeError> {
        let url = format!("{endpoint}/api/allnode_status");
        let client = build_ssrf_client(self.ssrf.as_ref(), &url).await?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("DStatus API request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = read_error_body(resp).await;
            return Err(ProbeError::ProbeFailed(format!(
                "DStatus API returned {status}: {body}"
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("DStatus API body read failed: {e}")))?;
        serde_json::from_str::<DStatusResponse>(&body)
            .map_err(|e| ProbeError::ProbeFailed(format!("DStatus API response parse failed: {e}")))
    }
}

#[async_trait]
impl ProbeSourceAdapter for DStatusProbeAdapter {
    async fn sync_traffic(&self, source: &ProbeSource) -> Result<ProbeSyncResult, ProbeError> {
        let last_snapshot = Self::parse_snapshot(source)?;
        let response = self.fetch_allnode_status(&source.endpoint_url).await?;

        let now = Timestamp::now();
        let mut samples = Vec::new();
        let mut new_snapshot = DStatusSnapshot::default();

        for (node_id, node_data) in &response.data {
            let current_used = node_data.traffic_stats.used;
            let prev_used = last_snapshot
                .nodes
                .get(node_id)
                .map(|e| e.used)
                .unwrap_or(0);

            // WHY: if current < previous, the billing cycle reset. Treat the
            // new value as the full delta (no negative traffic).
            let delta = if current_used >= prev_used {
                current_used - prev_used
            } else {
                current_used
            };

            new_snapshot
                .nodes
                .insert(node_id.clone(), DStatusEntry { used: current_used });

            if delta > 0 {
                samples.push(ProbeTrafficSample {
                    external_server_id: node_id.clone(),
                    upload: 0,
                    download: delta,
                    recorded_at: now,
                });
            }
        }

        let snapshot_json = serde_json::to_string(&new_snapshot).map_err(|e| {
            ProbeError::ProbeFailed(format!("counter snapshot serialize failed: {e}"))
        })?;
        Ok(ProbeSyncResult {
            samples,
            new_counter_snapshot: Some(snapshot_json),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deve_sub_domain::ProbeSourceKind;
    use deve_sub_kernel::{ProbeSourceId, SubscriptionId};

    fn mk_source(snapshot: Option<String>) -> ProbeSource {
        let now = Timestamp::now();
        ProbeSource {
            id: ProbeSourceId::new(),
            kind: ProbeSourceKind::DStatus,
            name: "test-dstatus".to_owned(),
            endpoint_url: "https://dstatus.example.com".to_owned(),
            auth_config: String::new(),
            subscription_id: Some(SubscriptionId::new()),
            enabled: true,
            last_sync_at: None,
            last_sync_status: None,
            last_counter_snapshot: snapshot,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn snapshot_parse_round_trip() {
        let mut snapshot = DStatusSnapshot::default();
        snapshot.nodes.insert(
            "node-a".to_owned(),
            DStatusEntry {
                used: 12_300_000_000,
            },
        );
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let source = mk_source(Some(json));
        let parsed = DStatusProbeAdapter::parse_snapshot(&source).expect("parse");
        assert_eq!(
            parsed.nodes.get("node-a").expect("node-a").used,
            12_300_000_000
        );
    }

    #[test]
    fn parse_snapshot_none_returns_default() {
        let source = mk_source(None);
        let snapshot = DStatusProbeAdapter::parse_snapshot(&source).expect("parse");
        assert!(snapshot.nodes.is_empty());
    }
}
