use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct Stalled(Arc<AtomicBool>);
struct Reset(Arc<AtomicBool>);
impl Drop for Reset {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}
#[async_trait]
impl ProbeSourceAdapter for Stalled {
    async fn sync_traffic(&self, _: &ProbeSource) -> Result<ProbeSyncResult, ProbeError> {
        self.0.store(true, Ordering::Relaxed);
        let _reset = Reset(self.0.clone());
        std::future::pending().await
    }
}

#[tokio::test]
async fn overall_sync_deadline_cancels_the_panel_batch() {
    let active = Arc::new(AtomicBool::new(false));
    let registry = ProbeSourceAdapterRegistry::new().with_komari(Arc::new(Stalled(active.clone())));
    let source = ProbeSource {
        id: deve_sub_kernel::ProbeSourceId::new(),
        revision: 0,
        kind: ProbeSourceKind::Komari,
        name: "test".into(),
        endpoint_url: "https://test.example".into(),
        auth_config: String::new(),
        subscription_id: None,
        enabled: true,
        last_sync_at: None,
        last_sync_status: None,
        last_counter_snapshot: None,
        created_at: deve_sub_kernel::Timestamp::now(),
        updated_at: deve_sub_kernel::Timestamp::now(),
    };
    let result = registry
        .sync_with_timeout(&source, Duration::from_millis(20))
        .await;
    assert!(
        matches!(result, Err(ProbeError::ProbeFailed(message)) if message == "probe source sync timed out")
    );
    assert!(
        !active.load(Ordering::Relaxed),
        "cancel must drop panel futures"
    );
}
