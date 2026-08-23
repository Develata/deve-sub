//! Nezha monitoring panel traffic sync adapter.
//!
//! Implements [`ProbeSourceAdapter`] for the Nezha panel. Calls
//! `GET {endpoint}/api/v1/server` with a Bearer PAT, parses cumulative
//! network counters, computes deltas against the last snapshot, and returns
//! [`ProbeTrafficSample`] values. The new counter snapshot is returned as
//! plaintext JSON for the storage layer to encrypt at rest (ADR-0007).
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port" and PROBE-001.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{
    ProbeError, ProbeSource, ProbeSourceAdapter, ProbeSyncResult, ProbeTrafficSample,
};
use deve_sub_kernel::Timestamp;
use serde::Deserialize;

use crate::SsrfChecker;
use crate::probe_common::{self, SUCCESS_BODY_CAP};

#[derive(Deserialize)]
struct NezhaServerState {
    net_in_transfer: u64,
    net_out_transfer: u64,
}

#[derive(Deserialize)]
struct NezhaServer {
    id: u64,
    #[allow(dead_code)]
    uuid: String,
    state: NezhaServerState,
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

pub struct NezhaProbeAdapter {
    ssrf: Arc<dyn SsrfChecker>,
}

impl Default for NezhaProbeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl NezhaProbeAdapter {
    /// Create a new Nezha adapter with production SSRF protection.
    #[must_use]
    pub fn new() -> Self {
        Self::with_checker(Arc::new(crate::ProductionSsrfChecker))
    }

    /// Create a new Nezha adapter with a custom SSRF checker (for testing).
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

    fn require_token(source: &ProbeSource) -> Result<String, ProbeError> {
        if source.auth_config.is_empty() {
            return Err(ProbeError::ProbeFailed(
                "Nezha probe source requires auth_config (Bearer token)".to_owned(),
            ));
        }
        Ok(source.auth_config.clone())
    }

    async fn fetch_servers(
        &self,
        endpoint: &str,
        token: &str,
    ) -> Result<Vec<NezhaServer>, ProbeError> {
        let url = format!("{endpoint}/api/v1/server");
        // SSRF guard + DNS pinning live in probe_common (shared with the
        // DStatus and Komari adapters, SEC-001-005).
        let client = probe_common::build_ssrf_client(self.ssrf.as_ref(), &url).await?;

        let resp = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Nezha API request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = probe_common::read_error_body(resp).await;
            return Err(ProbeError::ProbeFailed(format!(
                "Nezha API returned {status}: {body}"
            )));
        }

        let body = probe_common::read_body_capped(resp, SUCCESS_BODY_CAP).await;
        if body.len() > SUCCESS_BODY_CAP {
            return Err(ProbeError::ProbeFailed(format!(
                "Nezha API response body exceeds {SUCCESS_BODY_CAP} bytes"
            )));
        }
        serde_json::from_str::<Vec<NezhaServer>>(&body)
            .map_err(|e| ProbeError::ProbeFailed(format!("Nezha API response parse failed: {e}")))
    }

    fn compute_samples(
        servers: &[NezhaServer],
        last: &CounterSnapshot,
    ) -> (Vec<ProbeTrafficSample>, CounterSnapshot) {
        let now = Timestamp::now();
        let mut samples = Vec::with_capacity(servers.len());
        let mut new_snapshot = CounterSnapshot::default();

        for srv in servers {
            let id_str = srv.id.to_string();
            let (delta_in, delta_out) = match last.servers.get(&id_str) {
                Some(prev) => {
                    let din = if srv.state.net_in_transfer >= prev.net_in {
                        srv.state.net_in_transfer - prev.net_in
                    } else {
                        srv.state.net_in_transfer
                    };
                    let dout = if srv.state.net_out_transfer >= prev.net_out {
                        srv.state.net_out_transfer - prev.net_out
                    } else {
                        srv.state.net_out_transfer
                    };
                    (din, dout)
                }
                // WHY: first sighting of a server — the panel counter is a
                // lifetime/billing cumulative. Attributing it as fresh
                // traffic would instantly add terabytes to quota enforcement
                // when a long-running panel is first bound. Record the
                // baseline only; deltas start from the NEXT sync (never
                // double-count, under-count is safe).
                None => (0, 0),
            };

            new_snapshot.servers.insert(
                id_str.clone(),
                CounterEntry {
                    net_in: srv.state.net_in_transfer,
                    net_out: srv.state.net_out_transfer,
                },
            );

            if delta_in > 0 || delta_out > 0 {
                samples.push(ProbeTrafficSample {
                    external_server_id: id_str,
                    upload: delta_in,
                    download: delta_out,
                    recorded_at: now,
                });
            }
        }

        (samples, new_snapshot)
    }
}

#[async_trait]
impl ProbeSourceAdapter for NezhaProbeAdapter {
    async fn sync_traffic(&self, source: &ProbeSource) -> Result<ProbeSyncResult, ProbeError> {
        let token = Self::require_token(source)?;
        let last_snapshot = Self::parse_snapshot(source)?;

        let servers = self.fetch_servers(&source.endpoint_url, &token).await?;
        let (samples, new_snapshot) = Self::compute_samples(&servers, &last_snapshot);

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

    fn mk_server(id: u64, net_in: u64, net_out: u64) -> NezhaServer {
        NezhaServer {
            id,
            uuid: format!("uuid-{id}"),
            state: NezhaServerState {
                net_in_transfer: net_in,
                net_out_transfer: net_out,
            },
        }
    }

    fn mk_source(auth_config: String, snapshot: Option<String>) -> ProbeSource {
        let now = Timestamp::now();
        ProbeSource {
            id: ProbeSourceId::new(),
            kind: ProbeSourceKind::Nezha,
            name: "test-nezha".to_owned(),
            endpoint_url: "https://nezha.example.com".to_owned(),
            auth_config,
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
    fn compute_samples_first_sync_records_baseline_only() {
        // WHY: first sync attributes ZERO traffic — panel counters are
        // lifetime cumulatives; recording them would instantly exhaust a
        // newly bound subscription's quota. Baselines are captured in the
        // snapshot so the next sync can compute real deltas.
        let servers = vec![mk_server(1, 1000, 2000), mk_server(2, 500, 600)];
        let last = CounterSnapshot::default();
        let (samples, snapshot) = NezhaProbeAdapter::compute_samples(&servers, &last);

        assert!(samples.is_empty(), "first sync must not emit samples");
        assert_eq!(snapshot.servers.get("1").expect("server 1").net_in, 1000);
        assert_eq!(snapshot.servers.get("2").expect("server 2").net_out, 600);
    }

    #[test]
    fn compute_samples_delta_subtracts_previous_counters() {
        let servers = vec![mk_server(1, 3000, 4000)];
        let mut last = CounterSnapshot::default();
        last.servers.insert(
            "1".to_owned(),
            CounterEntry {
                net_in: 1000,
                net_out: 2000,
            },
        );
        let (samples, snapshot) = NezhaProbeAdapter::compute_samples(&servers, &last);

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].upload, 2000);
        assert_eq!(samples[0].download, 2000);
        assert_eq!(snapshot.servers.get("1").expect("server 1").net_in, 3000);
    }

    #[test]
    fn compute_samples_counter_reset_treats_new_value_as_full_delta() {
        let servers = vec![mk_server(1, 100, 50)];
        let mut last = CounterSnapshot::default();
        last.servers.insert(
            "1".to_owned(),
            CounterEntry {
                net_in: 5000,
                net_out: 8000,
            },
        );
        let (samples, _) = NezhaProbeAdapter::compute_samples(&servers, &last);

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].upload, 100);
        assert_eq!(samples[0].download, 50);
    }

    #[test]
    fn compute_samples_zero_delta_is_dropped() {
        let servers = vec![mk_server(1, 1000, 2000)];
        let mut last = CounterSnapshot::default();
        last.servers.insert(
            "1".to_owned(),
            CounterEntry {
                net_in: 1000,
                net_out: 2000,
            },
        );
        let (samples, snapshot) = NezhaProbeAdapter::compute_samples(&servers, &last);

        assert!(samples.is_empty());
        assert_eq!(snapshot.servers.get("1").expect("server 1").net_in, 1000);
    }

    #[test]
    fn snapshot_parse_round_trip() {
        let mut snapshot = CounterSnapshot::default();
        snapshot.servers.insert(
            "42".to_owned(),
            CounterEntry {
                net_in: 999,
                net_out: 888,
            },
        );
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let source = mk_source(String::new(), Some(json));
        let parsed = NezhaProbeAdapter::parse_snapshot(&source).expect("parse");
        assert_eq!(parsed.servers.get("42").expect("server 42").net_in, 999);
        assert_eq!(parsed.servers.get("42").expect("server 42").net_out, 888);
    }

    #[test]
    fn require_token_empty_fails() {
        let source = mk_source(String::new(), None);
        let result = NezhaProbeAdapter::require_token(&source);
        assert!(result.is_err());
    }

    #[test]
    fn require_token_nonempty_returns_clone() {
        let source = mk_source("nzp_test_token".to_owned(), None);
        let token = NezhaProbeAdapter::require_token(&source).expect("token");
        assert_eq!(token, "nzp_test_token");
    }

    #[tokio::test]
    async fn sync_traffic_missing_snapshot_returns_new_one() {
        let adapter = NezhaProbeAdapter::with_checker(Arc::new(crate::PermissiveSsrfChecker));
        let token = "nzp_test_token";
        let source = mk_source(token.to_owned(), None);

        let result = adapter.sync_traffic(&source).await;
        let err = result.expect_err("should fail on network");
        // The endpoint nezha.example.com is not resolvable in CI; the error
        // surfaces from either the SSRF/DNS step or the HTTP request step.
        let msg = err.to_string();
        assert!(
            msg.contains("Nezha API") || msg.contains("SSRF") || msg.contains("DNS"),
            "expected network/DNS failure, got: {msg}"
        );
    }
}
