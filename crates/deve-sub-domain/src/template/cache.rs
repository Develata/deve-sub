//! Generation cache entity and storage port.
//!
//! A [`GenerationCacheEntry`] stores a generated subscription's content along
//! with the parameters that produced it, keyed by `cache_key`. At most one
//! entry per `(template_id, profile)` is `is_active` at a time — enforced by
//! the `idx_generation_cache_single_active` partial unique index (migration
//! 0008). Atomic publish deactivates the prior active entry and activates the
//! new one in a single transaction (GEN-015, constraint #19: preserve last
//! successful subscription version on failure).
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation
//! cache".

use async_trait::async_trait;
use deve_sub_kernel::{GenerationCacheId, Revision, TemplateId};

use super::error::TemplateError;
use super::generation::GenerationMode;

/// A cached generation entry: the emitted content plus the parameters that
/// produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationCacheEntry {
    pub id: GenerationCacheId,
    pub template_id: TemplateId,
    pub template_version: u64,
    pub profile: String,
    pub mode: String,
    pub selection_mode: String,
    pub selection_payload: String,
    pub pool_revision: u64,
    pub cache_key: String,
    pub content: String,
    pub is_active: bool,
}

/// Parameters for a cache lookup or store.
#[derive(Debug, Clone)]
pub struct CacheKeyParams<'a> {
    pub template_id: TemplateId,
    pub template_version: u64,
    pub profile: &'a str,
    pub mode: GenerationMode,
    pub selection_mode: &'a str,
    pub selection_payload: &'a str,
    pub pool_revision: Revision,
}

impl CacheKeyParams<'_> {
    /// Compute the deterministic cache key from the parameters. SHA-256 of
    /// the canonical pipe-delimited concatenation. WHY: the cache must
    /// invalidate when any input changes (template version, profile, mode,
    /// selection, or pool revision), and a hash gives a stable, collision-
    /// resistant key without leaking the full parameters into the index.
    #[must_use]
    pub fn compute_key(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.template_id.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.template_version.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.profile.as_bytes());
        hasher.update(b"|");
        hasher.update(self.mode.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(self.selection_mode.as_bytes());
        hasher.update(b"|");
        hasher.update(self.selection_payload.as_bytes());
        hasher.update(b"|");
        hasher.update(self.pool_revision.value().to_string().as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(64);
        for b in digest.iter() {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

/// Storage boundary for the generation cache.
#[async_trait]
pub trait GenerationCacheRepository: Send + Sync {
    /// Look up a cached entry by its cache key. Returns `None` on miss.
    async fn find_by_key(
        &self,
        cache_key: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError>;

    /// Find the active generation for a template + profile. Returns `None`
    /// if no active entry exists (first generation or after manual clear).
    async fn find_active(
        &self,
        template_id: TemplateId,
        profile: &str,
    ) -> Result<Option<GenerationCacheEntry>, TemplateError>;

    /// Store a new cache entry as inactive (content only; no publish). The
    /// caller activates via [`activate`].
    async fn store(&self, entry: &GenerationCacheEntry) -> Result<(), TemplateError>;

    /// Atomically activate `new_id` and deactivate the currently active
    /// entry for the same `(template_id, profile)` in a single transaction.
    /// If `new_id` is already active, this is a no-op. On failure, the
    /// previous active entry remains active (constraint #19, GEN-015).
    async fn activate(
        &self,
        template_id: TemplateId,
        profile: &str,
        new_id: GenerationCacheId,
    ) -> Result<(), TemplateError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params<'a>(
        template_id: &'a str,
        version: u64,
        profile: &'a str,
        mode: GenerationMode,
        payload: &'a str,
        revision: u64,
    ) -> CacheKeyParams<'a> {
        CacheKeyParams {
            template_id: TemplateId::parse(template_id).expect("ulid"),
            template_version: version,
            profile,
            mode,
            selection_mode: "dynamic",
            selection_payload: payload,
            pool_revision: Revision::new(revision),
        }
    }

    #[test]
    fn cache_key_is_deterministic() {
        let p = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let k1 = p.compute_key();
        let k2 = p.compute_key();
        assert_eq!(k1, k2, "same params → same key");
        assert_eq!(k1.len(), 64, "SHA-256 hex = 64 chars");
    }

    #[test]
    fn cache_key_differs_on_version() {
        let p1 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let p2 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            2,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        assert_ne!(p1.compute_key(), p2.compute_key());
    }

    #[test]
    fn cache_key_differs_on_profile() {
        let p1 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let p2 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "xray",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        assert_ne!(p1.compute_key(), p2.compute_key());
    }

    #[test]
    fn cache_key_differs_on_revision() {
        let p1 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let p2 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            5,
        );
        assert_ne!(p1.compute_key(), p2.compute_key());
    }

    #[test]
    fn cache_key_differs_on_selection_mode() {
        let p1 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let p2 = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let mut p2 = p2;
        p2.selection_mode = "fixed";
        assert_ne!(p1.compute_key(), p2.compute_key());
    }

    #[test]
    fn cache_key_differs_on_generation_mode() {
        let p_lenient = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Lenient,
            "{}",
            0,
        );
        let p_strict = make_params(
            "01KZAAAAAAAAAAAAAAAAAAAAAA",
            1,
            "mihomo",
            GenerationMode::Strict,
            "{}",
            0,
        );
        assert_ne!(
            p_lenient.compute_key(),
            p_strict.compute_key(),
            "lenient and strict must produce different cache keys (B-10)"
        );
    }
}
