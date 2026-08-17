//! Komari monitoring panel traffic sync adapter.
//!
//! Implements [`ProbeSourceAdapter`] for the Komari panel. Calls
//! `GET {endpoint}/api/nodes` to list client UUIDs, then for each UUID calls
//! `GET {endpoint}/api/records/load?uuid={uuid}&load_type=network&hours=1`
//! (anonymous guest API), parses the latest cumulative `net_total_up` /
//! `net_total_down` counters, computes deltas against the last snapshot, and
//! returns [`ProbeTrafficSample`] values.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port" and PROBE-003.

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
struct KomariNodesResponse {
    data: Vec<KomariNode>,
}

#[derive(Deserialize)]
struct KomariNode {
    uuid: String,
    #[allow(dead_code)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct KomariRecordsResponse {
    data: KomariRecordsData,
}

#[derive(Deserialize)]
struct KomariRecordsData {
    records: Vec<KomariRecord>,
    #[allow(dead_code)]
    count: usize,
}

#[derive(Deserialize)]
struct KomariRecord {
    #[serde(default)]
    net_total_up: Option<u64>,
    #[serde(default)]
    net_total_down: Option<u64>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CounterSnapshot {
    servers: HashMap<String, CounterEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CounterEntry {
    net_in: u64,
    net_out: u64,
}

pub struct KomariProbeAdapter {
    ssrf: Arc<dyn SsrfChecker>,
}

impl Default for KomariProbeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl KomariProbeAdapter {
    /// Create a new Komari adapter with production SSRF protection.
    #[must_use]
    pub fn new() -> Self {
        Self::with_checker(Arc::new(crate::ProductionSsrfChecker))
    }

    /// Create a new Komari adapter with a custom SSRF checker (for testing).
    #[must_use]
    pub fn with_checker(ssrf: Arc<dyn SsrfChecker>) -> Self {
        Self { ssrf }
    }

    fn parse_snapshot(source: &ProbeSource) -> Result<CounterSnapshot, ProbeError> {
        match &source.last_counter_snapshot {
            None => Ok(CounterSnapshot::default()),
            Some(json) => serde_json::from_str(json).map_err(|e| {
                ProbeError::ProbeFailed(format!("counter snapshot parse failed: {e}"))
            }),
        }
    }

    async fn fetch_nodes(&self, endpoint: &str) -> Result<Vec<KomariNode>, ProbeError> {
        let url = format!("{endpoint}/api/nodes");
        let client = build_ssrf_client(self.ssrf.as_ref(), &url).await?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Komari API request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = read_error_body(resp).await;
            return Err(ProbeError::ProbeFailed(format!(
                "Komari API returned {status}: {body}"
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Komari API body read failed: {e}")))?;
        serde_json::from_str::<KomariNodesResponse>(&body)
            .map(|r| r.data)
            .map_err(|e| ProbeError::ProbeFailed(format!("Komari nodes parse failed: {e}")))
    }

    async fn fetch_latest_counters(
        &self,
        endpoint: &str,
        uuid: &str,
    ) -> Result<Option<(u64, u64)>, ProbeError> {
        let url = format!("{endpoint}/api/records/load?uuid={uuid}&load_type=network&hours=1");
        let client = build_ssrf_client(self.ssrf.as_ref(), &url).await?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Komari API request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = read_error_body(resp).await;
            return Err(ProbeError::ProbeFailed(format!(
                "Komari API returned {status}: {body}"
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Komari API body read failed: {e}")))?;
        let parsed: KomariRecordsResponse = serde_json::from_str(&body)
            .map_err(|e| ProbeError::ProbeFailed(format!("Komari records parse failed: {e}")))?;

        // WHY: records are sorted ascending by time (database query order);
        // the last element is the most recent. We only need the latest
        // cumulative counter to compute the delta.
        let latest = parsed.data.records.into_iter().last();
        Ok(latest.and_then(|r| {
            let up = r.net_total_up?;
            let down = r.net_total_down?;
            Some((up, down))
        }))
    }
}

#[async_trait]
impl ProbeSourceAdapter for KomariProbeAdapter {
    async fn sync_traffic(&self, source: &ProbeSource) -> Result<ProbeSyncResult, ProbeError> {
        let last_snapshot = Self::parse_snapshot(source)?;
        let nodes = self.fetch_nodes(&source.endpoint_url).await?;

        let now = Timestamp::now();
        let mut samples = Vec::new();
        let mut new_snapshot = CounterSnapshot::default();

        for node in &nodes {
            let uuid = &node.uuid;
            let latest = self
                .fetch_latest_counters(&source.endpoint_url, uuid)
                .await?;

            let (net_in, net_out) = match latest {
                Some(v) => v,
                None => continue,
            };

            let (prev_in, prev_out) = match last_snapshot.servers.get(uuid) {
                Some(prev) => (prev.net_in, prev.net_out),
                None => (0, 0),
            };

            let delta_in = if net_in >= prev_in {
                net_in - prev_in
            } else {
                net_in
            };
            let delta_out = if net_out >= prev_out {
                net_out - prev_out
            } else {
                net_out
            };

            new_snapshot
                .servers
                .insert(uuid.clone(), CounterEntry { net_in, net_out });

            if delta_in > 0 || delta_out > 0 {
                samples.push(ProbeTrafficSample {
                    external_server_id: uuid.clone(),
                    upload: delta_in,
                    download: delta_out,
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
            kind: ProbeSourceKind::Komari,
            name: "test-komari".to_owned(),
            endpoint_url: "https://komari.example.com".to_owned(),
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
        let mut snapshot = CounterSnapshot::default();
        snapshot.servers.insert(
            "uuid-abc".to_owned(),
            CounterEntry {
                net_in: 99_999,
                net_out: 88_888,
            },
        );
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let source = mk_source(Some(json));
        let parsed = KomariProbeAdapter::parse_snapshot(&source).expect("parse");
        assert_eq!(
            parsed.servers.get("uuid-abc").expect("uuid-abc").net_in,
            99_999
        );
        assert_eq!(
            parsed.servers.get("uuid-abc").expect("uuid-abc").net_out,
            88_888
        );
    }

    #[test]
    fn parse_snapshot_none_returns_default() {
        let source = mk_source(None);
        let snapshot = KomariProbeAdapter::parse_snapshot(&source).expect("parse");
        assert!(snapshot.servers.is_empty());
    }
}
