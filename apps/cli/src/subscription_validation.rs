//! Composition of read-only generation ports for subscription saves.

use std::sync::Arc;

use deve_sub_application::subscription::SubscriptionValidationDeps;
use deve_sub_security::MasterKey;
use deve_sub_storage_sqlite::{
    SqliteGenerationCacheRepository, SqliteNodePoolRepository, SqlitePoolMetaRepository,
    SqliteTemplateRepository, SqliteTemplateVersionRepository,
};

pub(crate) struct ValidationRepositories {
    templates: SqliteTemplateRepository,
    versions: SqliteTemplateVersionRepository,
    nodes: SqliteNodePoolRepository,
    cache: SqliteGenerationCacheRepository,
    pool_meta: SqlitePoolMetaRepository,
}

impl ValidationRepositories {
    pub(crate) fn new(pool: sqlx::SqlitePool, key: Arc<MasterKey>) -> Self {
        Self {
            templates: SqliteTemplateRepository::new(pool.clone()),
            versions: SqliteTemplateVersionRepository::new(pool.clone()),
            nodes: SqliteNodePoolRepository::new_with_key(pool.clone(), Arc::clone(&key)),
            cache: SqliteGenerationCacheRepository::new(pool.clone(), key),
            pool_meta: SqlitePoolMetaRepository::new(pool),
        }
    }

    pub(crate) fn deps(&self) -> SubscriptionValidationDeps<'_> {
        SubscriptionValidationDeps {
            template_repo: &self.templates,
            version_repo: &self.versions,
            pool_repo: &self.nodes,
            cache_repo: &self.cache,
            pool_meta_repo: &self.pool_meta,
        }
    }
}
