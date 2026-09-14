//! NODE-012: missing/deleted nodes and persistence errors must not erase valid measurements.

use super::*;
use async_trait::async_trait;
use deve_sub_domain::{ErrorClass, LatencyResult, Node};
use deve_sub_kernel::NodeId;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::timeout;

struct FixedProbe {
    gate: Option<(mpsc::UnboundedSender<NodeId>, Arc<Semaphore>)>,
}

#[async_trait]
impl LatencyProbe for FixedProbe {
    async fn probe(&self, node: &Node, _timeout: Duration) -> LatencyResult {
        if let Some((entered, release)) = &self.gate {
            entered.send(node.id).expect("probe entered");
            timeout(Duration::from_secs(5), release.acquire())
                .await
                .expect("release probe")
                .expect("permit")
                .forget();
        }
        LatencyResult {
            node_id: node.id,
            rtt_ms: Some(7),
            error_class: ErrorClass::Ok,
        }
    }
}

async fn fixture(probe: FixedProbe) -> (TestApp, axum::Router, String, Vec<String>) {
    let mut app = TestApp::new().await;
    app.state.tcp_probe = Arc::new(probe);
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    let mut nodes = Vec::new();
    for port in [443, 444] {
        nodes.push(
            import_node(
                &router,
                &cookie,
                &format!("trojan://fixture@node.example.com:{port}#fixture-{port}"),
            )
            .await,
        );
    }
    (app, router, cookie, nodes)
}

async fn start(router: &axum::Router, cookie: &str, nodes: &[String]) -> String {
    let response = router
        .clone()
        .oneshot(with_cookie(
            post_json(
                "/api/v1/probe-runs",
                &serde_json::json!({"probe_type":"tcp_connect", "node_ids":nodes}).to_string(),
            ),
            cookie,
        ))
        .await
        .expect("start");
    assert_eq!(response.status(), StatusCode::CREATED);
    body_to_json(response).await["run"]["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

async fn finished(
    app: &TestApp,
    router: &axum::Router,
    cookie: &str,
    id: &str,
) -> serde_json::Value {
    app.state
        .job_supervisor
        .shutdown(Duration::from_secs(5))
        .await;
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/probe-runs/{id}")),
            cookie,
        ))
        .await
        .expect("run");
    assert_eq!(response.status(), StatusCode::OK);
    body_to_json(response).await["run"].clone()
}

async fn history(router: &axum::Router, cookie: &str, id: &str) -> Vec<serde_json::Value> {
    let response = router
        .clone()
        .oneshot(with_cookie(
            get(&format!("/api/v1/nodes/{id}/latency")),
            cookie,
        ))
        .await
        .expect("latency history");
    assert_eq!(response.status(), StatusCode::OK);
    body_to_json(response).await["records"]
        .as_array()
        .expect("records")
        .clone()
}

async fn database(app: &TestApp) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        app._dir.path().join("test.db").display()
    ))
    .await
    .expect("fixture database")
}

#[tokio::test]
async fn node012_missing_node_is_skipped_without_losing_valid_history() {
    let (app, router, cookie, nodes) = fixture(FixedProbe { gate: None }).await;
    let missing = NodeId::new().to_string();
    let id = start(&router, &cookie, &[nodes[0].clone(), missing.clone()]).await;
    let run = finished(&app, &router, &cookie, &id).await;
    assert_eq!(run["status"], "completed");
    assert_eq!(
        history(&router, &cookie, &nodes[0]).await.len(),
        1,
        "a missing node must not roll back another node's latency"
    );
    let results = run["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    let skipped = results
        .iter()
        .find(|r| r["node_id"] == missing)
        .expect("missing result");
    assert_eq!(skipped["skipped"], true);
    assert_eq!(skipped["error_class"], "ok", "no DNS query was attempted");
    assert!(skipped["rtt_ms"].is_null());
}

#[tokio::test]
async fn node012_delete_during_measurement_preserves_other_history() {
    let (entered, mut events) = mpsc::unbounded_channel();
    let release = Arc::new(Semaphore::new(0));
    let (app, router, cookie, nodes) = fixture(FixedProbe {
        gate: Some((entered, release.clone())),
    })
    .await;
    let id = start(&router, &cookie, &nodes).await;
    for _ in 0..2 {
        timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("measurement started")
            .expect("event");
    }
    // Hold the real probe futures while a separate database connection deletes
    // one measured node; then let the runner's persistence transaction execute.
    let db = database(&app).await;
    sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(&nodes[0])
        .execute(&db)
        .await
        .expect("delete node");
    release.add_permits(2);
    let run = finished(&app, &router, &cookie, &id).await;
    assert_eq!(run["status"], "completed");
    assert_eq!(run["results"].as_array().expect("results").len(), 2);
    assert_eq!(history(&router, &cookie, &nodes[1]).await.len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM latency_records WHERE node_id = ?")
        .bind(&nodes[0])
        .fetch_one(&db)
        .await
        .expect("deleted history");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn node012_history_write_failure_marks_failed_and_retains_diagnostics() {
    let (app, router, cookie, nodes) = fixture(FixedProbe { gate: None }).await;
    let db = database(&app).await;
    sqlx::query("CREATE TRIGGER reject_second_latency BEFORE INSERT ON latency_records WHEN EXISTS (SELECT 1 FROM latency_records) BEGIN SELECT RAISE(ABORT, 'injected latency persistence failure'); END")
        .execute(&db).await.expect("inject storage failure");
    let id = start(&router, &cookie, &nodes).await;
    let run = finished(&app, &router, &cookie, &id).await;
    assert_eq!(
        run["status"], "failed",
        "history failure must not report Completed"
    );
    let results = run["results"].as_array().expect("diagnostics");
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|r| r["rtt_ms"] == 7 && r["skipped"] == false)
    );
    assert!(run["completed_at"].is_string());
    assert!(history(&router, &cookie, &nodes[0]).await.is_empty());
    assert!(
        history(&router, &cookie, &nodes[1]).await.is_empty(),
        "failed transaction is atomic"
    );
}
