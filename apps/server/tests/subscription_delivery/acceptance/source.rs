use super::*;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

/// Only these two synthetic loopback URLs may bypass production SSRF checks.
struct LoopbackOnly(Vec<String>);
impl deve_sub_adapters::SsrfChecker for LoopbackOnly {
    fn check(
        &self,
        url: &str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<IpAddr>, deve_sub_security::SsrfError>>
                + Send,
        >,
    > {
        assert!(
            self.0.iter().any(|allowed| allowed == url),
            "acceptance must never fetch another origin"
        );
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]) })
    }
}

/// SRC-002 and OUT-001: actual loopback HTTP upstream fetch and public
/// subscription download. This transports configurations, not proxy traffic.
#[tokio::test]
async fn acceptance_source_refresh_to_loopback_http_subscription() {
    let cases = cases();
    let body = cases
        .iter()
        .filter(|c| c.source_type == "uri_list")
        .map(|c| c.input.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream listener");
    let origin = format!(
        "http://{}/fixture",
        listener.local_addr().expect("upstream address")
    );
    let shadow_url = origin.replace("/fixture", "/shadowtls");
    let shadow_body = cases[10].input.clone();
    let upstream = axum::Router::new().route(
        "/fixture",
        axum::routing::get(move || {
            let body = body.clone();
            async move {
                (
                    [("content-type", "text/plain"), ("etag", "\"fixture-v1\"")],
                    body,
                )
            }
        }),
    );
    let upstream = upstream.route(
        "/shadowtls",
        axum::routing::get(move || {
            let body = shadow_body.clone();
            async move { ([("content-type", "application/json")], body) }
        }),
    );
    let (upstream_stop, stop) = tokio::sync::oneshot::channel::<()>();
    let upstream_task = tokio::spawn(async move {
        axum::serve(listener, upstream)
            .with_graceful_shutdown(async {
                let _ = stop.await;
            })
            .await
    });
    let mut app = TestApp::new().await;
    app.state.fetcher = Arc::new(
        deve_sub_adapters::HttpFetcher::with_checker(LoopbackOnly(vec![
            origin.clone(),
            shadow_url.clone(),
        ]))
        .timeout(3)
        .max_redirects(0),
    );
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    for (kind, url, expected) in [("uri_list", origin, 10), ("auto", shadow_url, 1)] {
        let response = router.clone().oneshot(with_cookie(post_json("/api/v1/sources", &json!({"name":format!("acceptance-{kind}"),"source_type":kind,"url":url,"auto_update":false,"keep_on_fail":true,"update_interval_secs":3600}).to_string()), &cookie)).await.expect("source create");
        assert_eq!(response.status(), StatusCode::CREATED);
        let source = body_to_json(response).await;
        let source_id = source["source"]["id"].as_str().expect("source ID");
        let response = router
            .clone()
            .oneshot(with_cookie(
                post_json(&format!("/api/v1/sources/{source_id}/refresh"), ""),
                &cookie,
            ))
            .await
            .expect("refresh");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let job = body_to_json(response).await;
        let job_id = job["job_id"].as_str().expect("job ID");
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let response = router
                    .clone()
                    .oneshot(with_cookie(
                        get(&format!("/api/v1/sources/refresh-jobs/{job_id}")),
                        &cookie,
                    ))
                    .await
                    .expect("job");
                assert_eq!(response.status(), StatusCode::OK);
                let job = body_to_json(response).await;
                if job["status"] != "running" && job["status"] != "pending" {
                    break job;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("bounded refresh");
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(result["new_nodes"], expected);
    }
    let spec = routing::supported_routing_projection();
    let id = template(&router, &cookie, &spec).await;
    let token = subscription(&router, &cookie, &id, "mihomo", None).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("delivery listener");
    let url = format!(
        "http://{}/sub/{token}/mihomo",
        listener.local_addr().expect("delivery address")
    );
    let (delivery_stop, stop) = tokio::sync::oneshot::channel::<()>();
    let delivery_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stop.await;
            })
            .await
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client")
        .get(url)
        .send()
        .await
        .expect("HTTP subscription");
    assert_eq!(response.status(), StatusCode::OK);
    let content = response.text().await.expect("HTTP body");
    assert_output(
        &content,
        "mihomo",
        &cases
            .iter()
            .filter(|c| c.accepts("mihomo"))
            .collect::<Vec<_>>(),
    );
    routing::assert_routing(&content, &spec);
    upstream_stop.send(()).expect("stop upstream");
    delivery_stop.send(()).expect("stop delivery");
    tokio::time::timeout(Duration::from_secs(3), async {
        upstream_task
            .await
            .expect("upstream task")
            .expect("upstream server");
        delivery_task
            .await
            .expect("delivery task")
            .expect("delivery server");
    })
    .await
    .expect("bounded server cleanup");
}
