//! Generation pipeline: resolve → compat → strict check → emit → validate,
//! with cache lookup and atomic publish.
//!
//! This is the core generation command (M5 Slice 5a + 5b + 5c). It
//! orchestrates the existing building blocks (`resolve_template`,
//! `check_compatibility`, container emitters) into a single pipeline. Cache
//! lookup skips regeneration when inputs are unchanged; on miss, `generate`
//! stores and atomically publishes the result while `preview` returns it
//! without side effects. On any error, no store or activate occurs — the
//! previous active generation remains served (constraint #19, GEN-015).
//!
//! Preview consistency (GEN-016): `preview` and `generate` share the same
//! pipeline and cache lookup, so preview output equals the published output.
//!
//! See `docs/plan/milestones/M5-generator-and-v3-template.md` §"Generation
//! pipeline" and §"Generation cache".

use std::collections::HashSet;

use deve_sub_compatibility::ProfileKind;
use deve_sub_domain::source::NodePoolRepository;
use deve_sub_domain::template::{TemplateRepository, TemplateVersionRepository};
use deve_sub_domain::{
    CacheKeyParams, GenerationCacheEntry, GenerationCacheRepository, GenerationError,
    GenerationMode, GenerationRequest, GenerationResult, Node, PoolMetaRepository, SelectionMode,
    TemplateVersion,
};
use deve_sub_emitter::{
    emit_json, emit_mihomo, emit_shadowrocket, emit_singbox, emit_uri_list, emit_v2ray, emit_xray,
};
use deve_sub_kernel::{GenerationCacheId, NodeId, Revision, TemplateId};

use super::compatibility::check_compatibility;
use super::error::TemplateAppError;
use super::selection::resolve_template;
use super::validation::parse_template_document;

/// Resolved inputs shared by [`generate`] and [`preview`].
struct GenerationContext {
    version: TemplateVersion,
    profile: ProfileKind,
    pool_revision: Revision,
    cache_key: String,
    selection_mode: &'static str,
    selection_payload: String,
}

/// Run the generation pipeline for a template + profile + mode.
///
/// Cache lookup: if the cache key (template_id, template_version, profile,
/// selection_mode, selection_payload, pool_revision) matches an existing
/// entry, the cached content is returned without re-running the pipeline.
/// On cache miss, the pipeline runs; on success, the result is stored as
/// inactive and then atomically activated (deactivating the prior active
/// entry for the same template + profile). On any error, no store or
/// activate occurs — the previous active generation remains served
/// (constraint #19, GEN-015).
///
/// In strict mode, returns [`GenerationError::IncompatibleNodes`] if any node
/// is excluded (GEN-014). This error occurs before any cache mutation.
pub async fn generate(
    template_repo: &dyn TemplateRepository,
    version_repo: &dyn TemplateVersionRepository,
    pool_repo: &dyn NodePoolRepository,
    cache_repo: &dyn GenerationCacheRepository,
    pool_meta_repo: &dyn PoolMetaRepository,
    request: GenerationRequest,
) -> Result<GenerationResult, TemplateAppError> {
    let ctx = resolve_context(template_repo, version_repo, pool_meta_repo, &request).await?;

    if let Some(cached) = cache_repo.find_by_key(&ctx.cache_key).await? {
        return Ok(cached_result(&cached));
    }

    let result = run_pipeline(
        &ctx.version,
        pool_repo,
        ctx.profile,
        request.mode,
        request.node_selection.as_ref(),
    )
    .await?;

    let entry = GenerationCacheEntry {
        id: GenerationCacheId::new(),
        template_id: request.template_id,
        template_version: ctx.version.version,
        profile: request.profile.clone(),
        selection_mode: ctx.selection_mode.to_owned(),
        selection_payload: ctx.selection_payload,
        pool_revision: ctx.pool_revision.value(),
        cache_key: ctx.cache_key,
        content: result.content.clone(),
        is_active: false,
    };

    cache_repo.store(&entry).await?;
    cache_repo
        .activate(entry.template_id, &entry.profile, entry.id)
        .await?;

    Ok(result)
}

/// Generate for subscription delivery: store the result as inactive for cache
/// reuse but do NOT activate it.
///
/// WHY: a Subscription may have its own `node_selection` override, producing a
/// different `cache_key` than the admin's template-level generation. The
/// `activate` step deactivates the currently active entry for the same
/// `(template_id, profile)` — so activating a subscription's generation would
/// silently replace the admin's active generation. Delivery stores as
/// inactive only: the cache_key lookup on subsequent deliveries hits the
/// stored entry, and the admin's active generation is untouched.
///
/// On cache hit (same `cache_key`), returns the cached content without
/// re-running the pipeline. On any pipeline error, no store occurs — the
/// previous cached entry (if any) remains available for the next delivery
/// (constraint #19).
pub async fn generate_for_delivery(
    template_repo: &dyn TemplateRepository,
    version_repo: &dyn TemplateVersionRepository,
    pool_repo: &dyn NodePoolRepository,
    cache_repo: &dyn GenerationCacheRepository,
    pool_meta_repo: &dyn PoolMetaRepository,
    request: GenerationRequest,
) -> Result<GenerationResult, TemplateAppError> {
    let ctx = resolve_context(template_repo, version_repo, pool_meta_repo, &request).await?;

    if let Some(cached) = cache_repo.find_by_key(&ctx.cache_key).await? {
        return Ok(cached_result(&cached));
    }

    let result = run_pipeline(
        &ctx.version,
        pool_repo,
        ctx.profile,
        request.mode,
        request.node_selection.as_ref(),
    )
    .await?;

    let entry = GenerationCacheEntry {
        id: GenerationCacheId::new(),
        template_id: request.template_id,
        template_version: ctx.version.version,
        profile: request.profile.clone(),
        selection_mode: ctx.selection_mode.to_owned(),
        selection_payload: ctx.selection_payload,
        pool_revision: ctx.pool_revision.value(),
        cache_key: ctx.cache_key,
        content: result.content.clone(),
        is_active: false,
    };

    // WHY: concurrent deliveries with the same cache_key may race to store.
    // The UNIQUE constraint on cache_key rejects the duplicate; on that
    // specific failure, re-read the cache to retrieve the winning entry
    // rather than returning a 503 (OUT-014: all concurrent clients see a
    // complete version). Any non-UNIQUE store error propagates.
    if let Err(store_err) = cache_repo.store(&entry).await {
        if let Some(cached) = cache_repo.find_by_key(&entry.cache_key).await? {
            return Ok(cached_result(&cached));
        }
        return Err(store_err.into());
    }

    Ok(result)
}
///
/// Shares the same cache lookup and pipeline as [`generate`]. On cache hit,
/// returns the cached (active) content. On cache miss, runs the full
/// pipeline but does NOT store or activate — the active generation is
/// unchanged. The returned content is identical to what [`generate`] would
/// produce for the same inputs, ensuring preview consistency (GEN-016).
///
/// In strict mode, returns [`GenerationError::IncompatibleNodes`] if any node
/// is excluded (GEN-014).
pub async fn preview(
    template_repo: &dyn TemplateRepository,
    version_repo: &dyn TemplateVersionRepository,
    pool_repo: &dyn NodePoolRepository,
    cache_repo: &dyn GenerationCacheRepository,
    pool_meta_repo: &dyn PoolMetaRepository,
    request: GenerationRequest,
) -> Result<GenerationResult, TemplateAppError> {
    let ctx = resolve_context(template_repo, version_repo, pool_meta_repo, &request).await?;

    if let Some(cached) = cache_repo.find_by_key(&ctx.cache_key).await? {
        return Ok(cached_result(&cached));
    }

    run_pipeline(
        &ctx.version,
        pool_repo,
        ctx.profile,
        request.mode,
        request.node_selection.as_ref(),
    )
    .await
}

/// Get the currently active generation for a template + profile.
///
/// Returns `None` if no active generation exists (first generation or after
/// manual clear). The active entry is the last successfully generated and
/// published content (GEN-015, constraint #19).
pub async fn get_active_generation(
    cache_repo: &dyn GenerationCacheRepository,
    template_id: TemplateId,
    profile: &str,
) -> Result<Option<GenerationCacheEntry>, TemplateAppError> {
    Ok(cache_repo.find_active(template_id, profile).await?)
}

async fn resolve_context(
    template_repo: &dyn TemplateRepository,
    version_repo: &dyn TemplateVersionRepository,
    pool_meta_repo: &dyn PoolMetaRepository,
    request: &GenerationRequest,
) -> Result<GenerationContext, TemplateAppError> {
    if super::get_template(template_repo, request.template_id)
        .await?
        .is_none()
    {
        return Err(TemplateAppError::TemplateNotFound);
    }

    let version = match request.template_version_pin {
        Some(n) => version_repo
            .find_by_version_number(request.template_id, n)
            .await?
            .ok_or(TemplateAppError::VersionNotFound)?,
        None => super::get_active_version(version_repo, request.template_id)
            .await?
            .ok_or(TemplateAppError::VersionNotFound)?,
    };

    let doc = parse_template_document(&version.spec_yaml)?;

    let profile = ProfileKind::from_kebab(&request.profile)
        .ok_or_else(|| TemplateAppError::UnknownProfile(request.profile.clone()))?;

    let pool_revision = pool_meta_repo
        .get_revision()
        .await
        .map_err(|e| TemplateAppError::Storage(e.to_string()))?;

    let selector = request
        .node_selection
        .as_ref()
        .unwrap_or(&doc.spec.node_selector);
    let selection_mode = match selector.mode {
        SelectionMode::Dynamic => "dynamic",
        SelectionMode::Fixed => "fixed",
    };
    let selection_payload =
        serde_json::to_string(selector).map_err(|e| TemplateAppError::Storage(e.to_string()))?;

    let cache_key = CacheKeyParams {
        template_id: request.template_id,
        template_version: version.version,
        profile: &request.profile,
        selection_mode,
        selection_payload: &selection_payload,
        pool_revision,
    }
    .compute_key();

    Ok(GenerationContext {
        version,
        profile,
        pool_revision,
        cache_key,
        selection_mode,
        selection_payload,
    })
}

async fn run_pipeline(
    version: &TemplateVersion,
    pool_repo: &dyn NodePoolRepository,
    profile: ProfileKind,
    mode: GenerationMode,
    selection_override: Option<&deve_sub_domain::NodeSelector>,
) -> Result<GenerationResult, TemplateAppError> {
    let mut doc = parse_template_document(&version.spec_yaml)?;

    if let Some(sel) = selection_override {
        doc.spec.node_selector = sel.clone();
    }

    let resolution = resolve_template(&doc, pool_repo).await?;

    let mut all_ids = resolution.selected_node_ids;
    for g in &resolution.groups {
        all_ids.extend(g.explicit_node_ids.iter().copied());
        all_ids.extend(g.quick_group_node_ids.iter().copied());
    }
    all_ids.sort_unstable();
    all_ids.dedup();

    let report = check_compatibility(&all_ids, profile, pool_repo).await?;

    if mode == GenerationMode::Strict && !report.excluded.is_empty() {
        return Err(TemplateAppError::Generation(
            GenerationError::IncompatibleNodes(report),
        ));
    }

    let mut warnings = Vec::new();
    for m in &resolution.selection_missing {
        warnings.push(format!("node {} is {}", m.node_id, m.reason));
    }
    for g in &resolution.groups {
        for m in &g.missing {
            warnings.push(format!(
                "group '{}': node {} is {}",
                g.group_name, m.node_id, m.reason
            ));
        }
    }

    let mut nodes: Vec<(Node, i64)> = Vec::with_capacity(report.included_node_ids.len());
    for id in &report.included_node_ids {
        match pool_repo.get_node(*id).await {
            Ok(Some(entry)) => {
                if entry.is_active && !entry.missing_from_source {
                    let sort_order = entry.override_info.as_ref().map_or(0, |o| o.sort_order);
                    nodes.push((entry.node, sort_order));
                } else {
                    warnings.push(format!("node {} became unavailable during generation", id));
                }
            }
            Ok(None) => {
                warnings.push(format!("node {} not found in pool during fetch", id));
            }
            Err(e) => return Err(TemplateAppError::Storage(e.to_string())),
        }
    }

    sort_and_dedup(&mut nodes);
    let node_refs: Vec<Node> = nodes.into_iter().map(|(n, _)| n).collect();

    // WHY: returning an error here — before `emit` and before any cache
    // mutation — ensures the prior active generation remains served when the
    // pool is temporarily empty (constraint #19). Emitting an empty-but-
    // structurally-valid document would silently replace the last good
    // subscription with a blank one.
    if node_refs.is_empty() {
        return Err(TemplateAppError::NoCompatibleNodes);
    }

    let content = emit(profile, &node_refs)?;

    validate_output(&content, profile)?;

    Ok(GenerationResult {
        content,
        profile: profile.as_kebab().to_owned(),
        included_node_ids: report.included_node_ids,
        excluded: report.excluded,
        warnings,
    })
}

fn cached_result(cached: &GenerationCacheEntry) -> GenerationResult {
    GenerationResult {
        content: cached.content.clone(),
        profile: cached.profile.clone(),
        included_node_ids: Vec::new(),
        excluded: Vec::new(),
        warnings: vec!["served from cache".to_owned()],
    }
}

fn sort_and_dedup(nodes: &mut Vec<(Node, i64)>) {
    nodes.sort_by(|(a, sa), (b, sb)| {
        sa.cmp(sb).then_with(|| {
            a.endpoint
                .host
                .uri_host()
                .cmp(&b.endpoint.host.uri_host())
                .then(a.endpoint.port.cmp(&b.endpoint.port))
                .then(a.protocol.to_string().cmp(&b.protocol.to_string()))
        })
    });
    let mut seen: HashSet<NodeId> = HashSet::new();
    nodes.retain(|(n, _)| seen.insert(n.id));
}

fn emit(profile: ProfileKind, nodes: &[Node]) -> Result<String, TemplateAppError> {
    match profile {
        ProfileKind::Mihomo => emit_mihomo(nodes),
        ProfileKind::SingBox => emit_singbox(nodes),
        ProfileKind::Xray => emit_xray(nodes),
        ProfileKind::V2Ray => emit_v2ray(nodes),
        ProfileKind::Shadowrocket => emit_shadowrocket(nodes),
        ProfileKind::UriList => emit_uri_list(nodes),
        ProfileKind::Json => emit_json(nodes),
    }
    .map_err(|e| TemplateAppError::Emit(e.to_string()))
}

fn validate_output(content: &str, profile: ProfileKind) -> Result<(), TemplateAppError> {
    if content.trim().is_empty() {
        return Err(TemplateAppError::EmptyOutput);
    }
    match profile {
        ProfileKind::Mihomo => {
            serde_yaml::from_str::<serde_yaml::Value>(content)
                .map_err(|_| TemplateAppError::EmptyOutput)?;
        }
        ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray | ProfileKind::Json => {
            serde_json::from_str::<serde_json::Value>(content)
                .map_err(|_| TemplateAppError::EmptyOutput)?;
        }
        ProfileKind::Shadowrocket | ProfileKind::UriList => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_output_rejects_empty() {
        assert!(validate_output("", ProfileKind::Mihomo).is_err());
        assert!(validate_output("   \n  ", ProfileKind::SingBox).is_err());
    }

    #[test]
    fn validate_output_accepts_valid_yaml() {
        let yaml = "proxies:\n  - name: test\n    type: trojan\n    server: a.com\n    port: 443\n";
        assert!(validate_output(yaml, ProfileKind::Mihomo).is_ok());
    }

    #[test]
    fn validate_output_accepts_valid_json() {
        let json = r#"{"outbounds":[]}"#;
        assert!(validate_output(json, ProfileKind::SingBox).is_ok());
        assert!(validate_output(json, ProfileKind::Xray).is_ok());
        assert!(validate_output(json, ProfileKind::V2Ray).is_ok());
    }

    #[test]
    fn validate_output_rejects_invalid_json() {
        assert!(validate_output("{not json", ProfileKind::SingBox).is_err());
    }

    #[test]
    fn validate_output_skips_parse_for_uri_list() {
        assert!(validate_output("trojan://pw@host:443", ProfileKind::UriList).is_ok());
        assert!(validate_output("any non-empty text", ProfileKind::Shadowrocket).is_ok());
    }

    #[test]
    fn emit_dispatches_all_profiles() {
        let nodes: Vec<Node> = vec![];
        assert!(emit(ProfileKind::Mihomo, &nodes).is_ok());
        assert!(emit(ProfileKind::SingBox, &nodes).is_ok());
        assert!(emit(ProfileKind::Xray, &nodes).is_ok());
        assert!(emit(ProfileKind::V2Ray, &nodes).is_ok());
        assert!(emit(ProfileKind::UriList, &nodes).is_ok());
    }
}
