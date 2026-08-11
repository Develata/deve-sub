//! Registry that dispatches [`ProbeSourceAdapter`] calls to the concrete
//! adapter matching a probe source's kind.
//!
//! The registry itself implements [`ProbeSourceAdapter`] so the application
//! command holds a single `&dyn ProbeSourceAdapter` regardless of how many
//! panel kinds are configured. Adapters for kinds not yet implemented return
//! [`ProbeError::ProbeFailed`] with a clear message.
//!
//! See `docs/plan/milestones/M7-probes-and-detection.md` §"Probe source
//! adapter Port".

use std::sync::Arc;

use async_trait::async_trait;
use deve_sub_domain::{
    ProbeError, ProbeSource, ProbeSourceAdapter, ProbeSourceKind, ProbeSyncResult,
};

/// Registry of panel-specific probe adapters, keyed by [`ProbeSourceKind`].
#[derive(Clone)]
pub struct ProbeSourceAdapterRegistry {
    nezha: Option<Arc<dyn ProbeSourceAdapter>>,
    dstatus: Option<Arc<dyn ProbeSourceAdapter>>,
    komari: Option<Arc<dyn ProbeSourceAdapter>>,
}

impl ProbeSourceAdapterRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            nezha: None,
            dstatus: None,
            komari: None,
        }
    }

    /// Attach a Nezha panel adapter.
    #[must_use]
    pub fn with_nezha(mut self, adapter: Arc<dyn ProbeSourceAdapter>) -> Self {
        self.nezha = Some(adapter);
        self
    }

    /// Attach a DStatus panel adapter.
    #[must_use]
    pub fn with_dstatus(mut self, adapter: Arc<dyn ProbeSourceAdapter>) -> Self {
        self.dstatus = Some(adapter);
        self
    }

    /// Attach a Komari panel adapter.
    #[must_use]
    pub fn with_komari(mut self, adapter: Arc<dyn ProbeSourceAdapter>) -> Self {
        self.komari = Some(adapter);
        self
    }

    fn resolve(&self, kind: ProbeSourceKind) -> Result<&Arc<dyn ProbeSourceAdapter>, ProbeError> {
        match kind {
            ProbeSourceKind::Nezha => self.nezha.as_ref().ok_or_else(|| {
                ProbeError::ProbeFailed("no Nezha probe adapter configured".to_owned())
            }),
            ProbeSourceKind::DStatus => self.dstatus.as_ref().ok_or_else(|| {
                ProbeError::ProbeFailed("no DStatus probe adapter configured".to_owned())
            }),
            ProbeSourceKind::Komari => self.komari.as_ref().ok_or_else(|| {
                ProbeError::ProbeFailed("no Komari probe adapter configured".to_owned())
            }),
        }
    }
}

impl Default for ProbeSourceAdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProbeSourceAdapter for ProbeSourceAdapterRegistry {
    async fn sync_traffic(&self, source: &ProbeSource) -> Result<ProbeSyncResult, ProbeError> {
        let adapter = self.resolve(source.kind)?;
        adapter.sync_traffic(source).await
    }
}
