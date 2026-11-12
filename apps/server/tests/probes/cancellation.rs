use super::*;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use deve_sub_application::probe::DEFAULT_CONCURRENCY;
use deve_sub_domain::{ErrorClass, LatencyResult, Node};

struct BlockedProbe {
    started: tokio::sync::mpsc::UnboundedSender<()>,
    dropped: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    complete_first: AtomicBool,
}

struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl LatencyProbe for BlockedProbe {
    async fn probe(&self, node: &Node, timeout: Duration) -> LatencyResult {
        if self.complete_first.swap(false, Ordering::SeqCst) {
            return LatencyResult {
                node_id: node.id,
                rtt_ms: Some(7),
                error_class: ErrorClass::Ok,
            };
        }
        let _in_flight = InFlight(Arc::clone(&self.dropped));
        self.started
            .send(())
            .expect("test observes probe admission");
        // WHY: reserved remote addresses can refuse immediately. An owned,
        // deadline-bounded future gives cancellation a deterministic workload.
        let _ = tokio::time::timeout(timeout, std::future::pending::<()>()).await;
        self.completed.fetch_add(1, Ordering::SeqCst);
        LatencyResult {
            node_id: node.id,
            rtt_ms: None,
            error_class: ErrorClass::Timeout,
        }
    }
}

/// NODE-016: cancel actual in-flight work and skip queued nodes through the API.
#[tokio::test]
async fn batch_cancel_probe_run_aborts_inflight_and_skips_pending() {
    assert_cancellation(false).await;
}

/// NODE-016: cancelling remaining work must preserve completed measurements.
#[tokio::test]
async fn batch_cancel_probe_run_preserves_completed_measurements() {
    assert_cancellation(true).await;
}

async fn assert_cancellation(complete_first: bool) {
    let mut app = TestApp::new().await;
    let (started, mut admissions) = tokio::sync::mpsc::unbounded_channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    app.state.tcp_probe = Arc::new(BlockedProbe {
        started,
        dropped: Arc::clone(&dropped),
        completed: Arc::clone(&completed),
        complete_first: AtomicBool::new(complete_first),
    });
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let successful_count = usize::from(complete_first);
    let node_count = DEFAULT_CONCURRENCY + 3 + successful_count;
    let mut node_ids = Vec::new();
    for i in 0..node_count {
        let uri = format!("trojan://TEST_PASSWORD@127.0.0.1:{}#cancel-{i}", 10000 + i);
        node_ids.push(import_node(&router, &cookie, &uri).await);
    }
    let body = serde_json::json!({"probe_type": "tcp_connect", "node_ids": node_ids});
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json("/api/v1/probe-runs", &body.to_string()),
            &cookie,
        ))
        .await
        .expect("start run");
    assert_eq!(response.status(), StatusCode::CREATED);
    let run_id = body_to_json(response).await["run"]["id"]
        .as_str()
        .expect("run id")
        .to_owned();

    // Wait for the concurrency window to fill before cancelling, so both
    // in-flight cancellation and pending-node skipping are exercised.
    tokio::time::timeout(Duration::from_secs(3), async {
        for _ in 0..DEFAULT_CONCURRENCY {
            admissions.recv().await.expect("probe started");
        }
    })
    .await
    .expect("runner admits the bounded probe window");

    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(&format!("/api/v1/probe-runs/{run_id}/cancel"), "{}"),
            &cookie,
        ))
        .await
        .expect("cancel run");
    assert_eq!(response.status(), StatusCode::OK);

    let final_json = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let response = router
                .clone()
                .oneshot(with_cookie(
                    get(&format!("/api/v1/probe-runs/{run_id}")),
                    &cookie,
                ))
                .await
                .expect("poll run");
            let json = body_to_json(response).await;
            if matches!(
                json["run"]["status"].as_str(),
                Some("cancelled" | "completed" | "failed")
            ) {
                break json;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled run reaches terminal state");

    assert_eq!(final_json["run"]["status"], "cancelled");
    let results = final_json["run"]["results"].as_array().expect("results");
    assert_eq!(results.len(), node_count);
    assert_eq!(
        results.iter().filter(|r| r["skipped"] == true).count(),
        DEFAULT_CONCURRENCY + 3
    );
    assert!(
        results
            .iter()
            .filter(|r| r["skipped"] == true)
            .all(|r| r["rtt_ms"].is_null())
    );
    assert_eq!(
        results.iter().filter(|r| r["skipped"] == false).count(),
        successful_count
    );
    assert!(
        results
            .iter()
            .filter(|r| r["skipped"] == false)
            .all(|r| r["error_class"] == "ok" && r["rtt_ms"] == 7)
    );
    assert_eq!(dropped.load(Ordering::SeqCst), DEFAULT_CONCURRENCY);
    assert_eq!(
        completed.load(Ordering::SeqCst),
        0,
        "in-flight probes must be dropped, not allowed to time out"
    );
    assert!(
        admissions.try_recv().is_err(),
        "queued probes must never start"
    );

    for result in results {
        let response = router
            .clone()
            .oneshot(with_cookie(
                get(&format!(
                    "/api/v1/nodes/{}/latency",
                    result["node_id"].as_str().expect("node id")
                )),
                &cookie,
            ))
            .await
            .expect("latency history");
        let history = body_to_json(response).await;
        let expected = usize::from(result["skipped"] == false);
        assert_eq!(
            history["records"].as_array().expect("history").len(),
            expected
        );
        if expected == 1 {
            assert_eq!(history["records"][0]["rtt_ms"], 7);
            assert_eq!(history["records"][0]["error_class"], "ok");
        }
    }
    app.state
        .job_supervisor
        .shutdown(Duration::from_secs(1))
        .await;
    assert!(app.state.cancelled_flags.lock().expect("flags").is_empty());
}
