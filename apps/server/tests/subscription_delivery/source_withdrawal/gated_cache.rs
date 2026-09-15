//! Pause the first real generated snapshot immediately before SQLite store.
use super::*;
use async_trait::async_trait;
use deve_sub_domain::{GenerationCacheEntry, TemplateError};
use deve_sub_kernel::{GenerationCacheId, TemplateId};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

pub(super) struct GatedCache {
    inner: Arc<dyn GenerationCacheRepository>,
    first: AtomicBool,
    pub(super) entered: Notify,
    pub(super) release: Notify,
}

impl GatedCache {
    pub(super) fn new(inner: Arc<dyn GenerationCacheRepository>) -> Self {
        Self {
            inner,
            first: AtomicBool::new(true),
            entered: Notify::new(),
            release: Notify::new(),
        }
    }
}

#[async_trait]
impl GenerationCacheRepository for GatedCache {
    async fn find_by_key(&self, key: &str) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        self.inner.find_by_key(key).await
    }
    async fn find_active(
        &self,
        id: TemplateId,
        profile: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        self.inner.find_active(id, profile).await
    }
    async fn find_latest(
        &self,
        id: TemplateId,
        profile: &str,
        mode: &str,
        payload: &str,
        pin: Option<u64>,
        generation_mode: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError> {
        self.inner
            .find_latest(id, profile, mode, payload, pin, generation_mode)
            .await
    }
    async fn store(&self, entry: &GenerationCacheEntry) -> Result<(), TemplateError> {
        if self.first.swap(false, Ordering::SeqCst) {
            assert!(
                entry.content.contains("remote.example.com"),
                "pause after loading withdrawn credentials"
            );
            self.entered.notify_one();
            tokio::time::timeout(Duration::from_secs(10), self.release.notified())
                .await
                .expect("release old writer");
        }
        self.inner.store(entry).await
    }
    async fn activate(
        &self,
        id: TemplateId,
        profile: &str,
        new_id: GenerationCacheId,
    ) -> Result<(), TemplateError> {
        self.inner.activate(id, profile, new_id).await
    }
}
