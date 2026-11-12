//! Read-only save validation shared by API and CLI subscription commands.

use deve_sub_domain::{
    GenerationCacheRepository, GenerationMode, GenerationRequest, NodePoolRepository, NodeSelector,
    PoolMetaRepository, TemplateError, TemplateRepository, TemplateVersionRepository,
};
use deve_sub_kernel::TemplateId;

use super::SubscriptionAppError;
use crate::template::{self, TemplateAppError};

/// Ports needed to verify that a subscription can currently produce output.
pub struct SubscriptionValidationDeps<'a> {
    pub template_repo: &'a dyn TemplateRepository,
    pub version_repo: &'a dyn TemplateVersionRepository,
    pub pool_repo: &'a dyn NodePoolRepository,
    pub cache_repo: &'a dyn GenerationCacheRepository,
    pub pool_meta_repo: &'a dyn PoolMetaRepository,
}

impl SubscriptionValidationDeps<'_> {
    pub(super) async fn validate(
        &self,
        template_id: TemplateId,
        template_version_pin: Option<u64>,
        profile: &str,
        selector: &NodeSelector,
    ) -> Result<(), SubscriptionAppError> {
        // WHY: use delivery's exact selection/profile pipeline without publishing
        // output or issuing a credential. Pool changes remain delivery-time policy.
        template::preview(
            self.template_repo,
            self.version_repo,
            self.pool_repo,
            self.cache_repo,
            self.pool_meta_repo,
            GenerationRequest {
                template_id,
                template_version_pin,
                profile: profile.to_owned(),
                mode: GenerationMode::Lenient,
                node_selection: Some(selector.clone()),
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| match error {
            TemplateAppError::Storage(message)
            | TemplateAppError::Template(TemplateError::Storage(message)) => {
                SubscriptionAppError::Storage(message)
            }
            other => SubscriptionAppError::InvalidGeneration(other.to_string()),
        })
    }
}
