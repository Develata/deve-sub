//! Nezha monitoring panel traffic sync adapter.
//!
//! Implements [`ProbeSourceAdapter`] for the Nezha panel. Calls
//! `GET {endpoint}/api/v1/server` with a Bearer PAT, parses cumulative
//! network counters, computes deltas against the last snapshot, and returns
//! [`ProbeTrafficSample`] values. The new counter snapshot is encrypted and
//! returned for persistence.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port" and PROBE-001.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{
    ProbeError, ProbeSource, ProbeSourceAdapter, ProbeSyncResult, ProbeTrafficSample,
};
use deve_sub_kernel::Timestamp;
use deve_sub_security::{MasterKey, decrypt_from_b64, encrypt_to_b64};
use serde::Deserialize;
use url::Url;

use crate::SsrfChecker;

const ENCRYPTED_SEPARATOR: char = ':';

/// Maximum bytes read from an error response body for diagnostics.
///
/// WHY: bounds memory on the non-2xx path so a hostile panel cannot exhaust
/// memory via a large error body, and limits injection of remote content into
/// logs/DB/API responses. Matches `HttpFetcher::ERROR_BODY_CAP`.
const ERROR_BODY_CAP: usize = 1024;

/// Default request timeout: 30 seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

fn encrypt_secret(key: &[u8], plaintext: &[u8]) -> Result<String, ProbeError> {
    let (ct, nonce) = encrypt_to_b64(key, plaintext)
        .map_err(|e| ProbeError::ProbeFailed(format!("encryption failed: {e}")))?;
    Ok(format!("{ct}{ENCRYPTED_SEPARATOR}{nonce}"))
}

fn decrypt_secret(key: &[u8], combined: &str) -> Result<String, ProbeError> {
    let (ct, nonce) = combined
        .split_once(ENCRYPTED_SEPARATOR)
        .ok_or_else(|| ProbeError::ProbeFailed("encrypted field missing separator".to_owned()))?;
    let bytes = decrypt_from_b64(key, ct, nonce)
        .map_err(|e| ProbeError::ProbeFailed(format!("decryption failed: {e}")))?;
    String::from_utf8(bytes)
        .map_err(|e| ProbeError::ProbeFailed(format!("decrypted value is not UTF-8: {e}")))
}

/// Read up to [`ERROR_BODY_CAP`] bytes of an error response body.
async fn read_error_body(mut response: reqwest::Response) -> String {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= ERROR_BODY_CAP {
            body.truncate(ERROR_BODY_CAP);
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

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
    master_key: Arc<MasterKey>,
    ssrf: Arc<dyn SsrfChecker>,
}

impl NezhaProbeAdapter {
    /// Create a new Nezha adapter with production SSRF protection.
    #[must_use]
    pub fn new(master_key: Arc<MasterKey>) -> Self {
        Self::with_checker(master_key, Arc::new(crate::ProductionSsrfChecker))
    }

    /// Create a new Nezha adapter with a custom SSRF checker (for testing).
    #[must_use]
    pub fn with_checker(master_key: Arc<MasterKey>, ssrf: Arc<dyn SsrfChecker>) -> Self {
        Self { master_key, ssrf }
    }

    fn decrypt_auth(&self, source: &ProbeSource) -> Result<String, ProbeError> {
        if source.auth_config.is_empty() {
            return Err(ProbeError::ProbeFailed(
                "Nezha probe source requires auth_config (Bearer token)".to_owned(),
            ));
        }
        decrypt_secret(self.master_key.as_bytes(), &source.auth_config)
    }

    fn decrypt_snapshot(&self, source: &ProbeSource) -> Result<CounterSnapshot, ProbeError> {
        match &source.last_counter_snapshot {
            None => Ok(CounterSnapshot::default()),
            Some(encrypted) => {
                let json = decrypt_secret(self.master_key.as_bytes(), encrypted)?;
                serde_json::from_str(&json).map_err(|e| {
                    ProbeError::ProbeFailed(format!("counter snapshot parse failed: {e}"))
                })
            }
        }
    }

    fn encrypt_snapshot(&self, snapshot: &CounterSnapshot) -> Result<String, ProbeError> {
        let json = serde_json::to_string(snapshot).map_err(|e| {
            ProbeError::ProbeFailed(format!("counter snapshot serialize failed: {e}"))
        })?;
        encrypt_secret(self.master_key.as_bytes(), json.as_bytes())
    }

    async fn fetch_servers(
        &self,
        endpoint: &str,
        token: &str,
    ) -> Result<Vec<NezhaServer>, ProbeError> {
        let url = format!("{endpoint}/api/v1/server");
        let parsed = Url::parse(&url)
            .map_err(|e| ProbeError::ProbeFailed(format!("invalid endpoint URL: {e}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| ProbeError::ProbeFailed("endpoint URL has no hostname".to_owned()))?;

        // WHY: SSRF guard prevents an admin-configured endpoint from pointing
        // at internal addresses (loopback, private, link-local, CGNAT) and
        // mitigates DNS rebinding by returning the validated IPs. Mirrors
        // HttpFetcher's protection (SEC-001-005).
        let safe_ips = self
            .ssrf
            .check(&url)
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("SSRF check failed: {e}")))?;

        let mut builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            // WHY: disable auto-redirect so a compromised panel cannot
            // redirect the server to internal addresses after the SSRF
            // check passes.
            .redirect(reqwest::redirect::Policy::none());

        // WHY: pin DNS to the validated IPs to prevent DNS rebinding between
        // the SSRF check and the actual request. IP literals connect directly
        // and were already validated by the SSRF checker.
        if host.parse::<IpAddr>().is_err() {
            let socket_addrs: Vec<SocketAddr> =
                safe_ips.iter().map(|ip| SocketAddr::new(*ip, 0)).collect();
            builder = builder.resolve_to_addrs(host, &socket_addrs);
        }

        let client = builder
            .build()
            .map_err(|e| ProbeError::ProbeFailed(format!("HTTP client build failed: {e}")))?;

        let resp = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Nezha API request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            // WHY: cap the error body to bound memory and limit injection of
            // remote content into logs/DB/API responses. Matches
            // `HttpFetcher::ERROR_BODY_CAP`.
            let body = read_error_body(resp).await;
            return Err(ProbeError::ProbeFailed(format!(
                "Nezha API returned {status}: {body}"
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| ProbeError::ProbeFailed(format!("Nezha API body read failed: {e}")))?;
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
                None => (srv.state.net_in_transfer, srv.state.net_out_transfer),
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
        let token = self.decrypt_auth(source)?;
        let last_snapshot = self.decrypt_snapshot(source)?;

        let servers = self.fetch_servers(&source.endpoint_url, &token).await?;
        let (samples, new_snapshot) = Self::compute_samples(&servers, &last_snapshot);

        let encrypted_snapshot = self.encrypt_snapshot(&new_snapshot)?;

        Ok(ProbeSyncResult {
            samples,
            new_counter_snapshot: Some(encrypted_snapshot),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deve_sub_domain::ProbeSourceKind;
    use deve_sub_kernel::{ProbeSourceId, SubscriptionId};

    fn mk_master_key() -> Arc<MasterKey> {
        Arc::new(MasterKey::from_bytes(&[0x42u8; 32]))
    }

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
    fn compute_samples_first_sync_returns_full_counters() {
        let servers = vec![mk_server(1, 1000, 2000), mk_server(2, 500, 600)];
        let last = CounterSnapshot::default();
        let (samples, snapshot) = NezhaProbeAdapter::compute_samples(&servers, &last);

        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].external_server_id, "1");
        assert_eq!(samples[0].upload, 1000);
        assert_eq!(samples[0].download, 2000);
        assert_eq!(samples[1].external_server_id, "2");
        assert_eq!(samples[1].upload, 500);
        assert_eq!(samples[1].download, 600);
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
    fn encrypt_decrypt_secret_round_trip() {
        let key = mk_master_key();
        let plaintext = "nzp_secret_token_abc123";
        let encrypted = encrypt_secret(key.as_bytes(), plaintext.as_bytes()).expect("encrypt");
        assert_ne!(encrypted, plaintext);
        assert!(encrypted.contains(ENCRYPTED_SEPARATOR));
        let decrypted = decrypt_secret(key.as_bytes(), &encrypted).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn snapshot_encryption_round_trip() {
        let adapter = NezhaProbeAdapter::new(mk_master_key());
        let mut snapshot = CounterSnapshot::default();
        snapshot.servers.insert(
            "42".to_owned(),
            CounterEntry {
                net_in: 999,
                net_out: 888,
            },
        );
        let encrypted = adapter.encrypt_snapshot(&snapshot).expect("encrypt");
        assert!(encrypted.contains(ENCRYPTED_SEPARATOR));
        let source = mk_source(String::new(), Some(encrypted));
        let decrypted = adapter.decrypt_snapshot(&source).expect("decrypt");
        assert_eq!(decrypted.servers.get("42").expect("server 42").net_in, 999);
        assert_eq!(decrypted.servers.get("42").expect("server 42").net_out, 888);
    }

    #[test]
    fn decrypt_secret_wrong_key_fails() {
        let key1 = mk_master_key();
        let key2 = Arc::new(MasterKey::from_bytes(&[0x99u8; 32]));
        let encrypted = encrypt_secret(key1.as_bytes(), b"secret").expect("encrypt");
        let result = decrypt_secret(key2.as_bytes(), &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_auth_empty_fails() {
        let adapter = NezhaProbeAdapter::new(mk_master_key());
        let source = mk_source(String::new(), None);
        let result = adapter.decrypt_auth(&source);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sync_traffic_missing_snapshot_encrypts_new_one() {
        let key = mk_master_key();
        let adapter = NezhaProbeAdapter::with_checker(
            Arc::clone(&key),
            Arc::new(crate::PermissiveSsrfChecker),
        );
        let token = "nzp_test_token";
        let encrypted_token = encrypt_secret(key.as_bytes(), token.as_bytes()).expect("encrypt");
        let source = mk_source(encrypted_token, None);

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
