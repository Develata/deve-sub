//! OUT-013: overlapping regenerations must replace the current credential atomically.

use super::*;
use async_trait::async_trait;
use deve_sub_domain::{ShortCode, SubscriptionError};
use deve_sub_kernel::{ShortCodeId, SubscriptionId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Barrier;

struct GatedCodes {
    inner: Arc<dyn ShortCodeRepository>,
    entered: AtomicUsize,
    gate: Barrier,
}

#[async_trait]
impl ShortCodeRepository for GatedCodes {
    async fn create(&self, code: &ShortCode) -> Result<(), SubscriptionError> {
        self.inner.create(code).await
    }
    async fn replace(&self, code: &ShortCode) -> Result<(), SubscriptionError> {
        if self.entered.fetch_add(1, Ordering::Relaxed) < 2 {
            // Both application commands have read the same subscription before
            // either storage transaction can replace its credential.
            tokio::time::timeout(Duration::from_secs(5), self.gate.wait())
                .await
                .expect("both regenerations entered");
        }
        self.inner.replace(code).await
    }
    async fn find_by_code(&self, code: &str) -> Result<Option<ShortCode>, SubscriptionError> {
        self.inner.find_by_code(code).await
    }
    async fn find_by_subscription(
        &self,
        id: SubscriptionId,
    ) -> Result<Option<ShortCode>, SubscriptionError> {
        self.inner.find_by_subscription(id).await
    }
    async fn delete(&self, id: ShortCodeId) -> Result<(), SubscriptionError> {
        self.inner.delete(id).await
    }
    async fn delete_for_subscription(&self, id: SubscriptionId) -> Result<(), SubscriptionError> {
        self.inner.delete_for_subscription(id).await
    }
}

#[tokio::test]
async fn out013_concurrent_regeneration_replaces_current_code_without_retry_exhaustion() {
    for existing_code in [false, true] {
        let mut app = TestApp::new().await;
        let router = app.router();
        let cookie = setup_and_login(&router).await;
        import_nodes(
            &router,
            &cookie,
            "trojan://fixture@node.example.com:443#fixture",
        )
        .await;
        let template = create_template(&router, &cookie).await;
        let sub = create_sub(&router, &cookie, &template, "concurrent-codes").await;
        let id = sub["subscription"]["id"].as_str().expect("subscription");
        let path = format!("/api/v1/subscriptions/{id}/regenerate-short-code");
        if existing_code {
            let response = router
                .clone()
                .oneshot(with_cookie(post_json(&path, ""), &cookie))
                .await
                .expect("initial code");
            assert_eq!(response.status(), StatusCode::OK);
        }
        app.state.short_code_repo = Arc::new(GatedCodes {
            inner: app.state.short_code_repo.clone(),
            entered: AtomicUsize::new(0),
            gate: Barrier::new(2),
        });
        let router = app.router();
        let (first, second) = tokio::join!(
            router
                .clone()
                .oneshot(with_cookie(post_json(&path, ""), &cookie)),
            router
                .clone()
                .oneshot(with_cookie(post_json(&path, ""), &cookie)),
        );
        let mut codes = Vec::new();
        for response in [first, second] {
            let response = response.expect("regenerate response");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "overlapping regeneration must not exhaust random-code retries"
            );
            codes.push(
                body_to_json(response).await["code"]
                    .as_str()
                    .expect("code")
                    .to_owned(),
            );
        }
        let current = app
            .state
            .short_code_repo
            .find_by_subscription(SubscriptionId::parse(id).expect("id"))
            .await
            .expect("lookup")
            .expect("current");
        for code in &codes {
            let response = router
                .clone()
                .oneshot(get(&format!("/s/{code}/mihomo")))
                .await
                .expect("deliver");
            assert_eq!(
                response.status(),
                if code == &current.code {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                }
            );
        }
        let response = router
            .clone()
            .oneshot(with_cookie(post_json(&path, ""), &cookie))
            .await
            .expect("subsequent regeneration");
        assert_eq!(response.status(), StatusCode::OK);
        for code in codes {
            let response = router
                .clone()
                .oneshot(get(&format!("/s/{code}/mihomo")))
                .await
                .expect("revoked code");
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }
}
