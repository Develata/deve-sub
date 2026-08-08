//! Generation pipeline: resolve → compat → strict check → emit → validate,
//! with cache lookup and atomic publish.
//!
//! This is the core generation command (M5 Slice 5a + 5b). It orchestrates
//! the existing building blocks (`resolve_template`, `check_compatibility`,
//! container emitters) into a single pipeline. Cache lookup skips
//! regeneration when inputs are unchanged; on miss, the result is stored and
//! atomically published. On any error, no store or activate occurs — the
//! previous active generation remains served (constraint #19, GEN-015).
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
};
use deve_sub_emitter::{
    emit_mihomo, emit_shadowrocket, emit_singbox, emit_uri_list, emit_v2ray, emit_xray,
};
use deve_sub_kernel::{GenerationCacheId, NodeId, TemplateId};

use super::compatibility::check_compatibility;
use super::error::TemplateAppError;
use super::selection::resolve_template;
use super::validation::parse_template_document;

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
    if super::get_template(template_repo, request.template_id)
        .await?
        .is_none()
    {
        return Err(TemplateAppError::TemplateNotFound);
    }

    let version = super::get_active_version(version_repo, request.template_id)
        .await?
        .ok_or(TemplateAppError::VersionNotFound)?;

    let doc = parse_template_document(&version.spec_yaml)?;

    let profile = ProfileKind::from_kebab(&request.profile)
        .ok_or_else(|| TemplateAppError::UnknownProfile(request.profile.clone()))?;

    let pool_revision = pool_meta_repo
        .get_revision()
        .await
        .map_err(|e| TemplateAppError::Storage(e.to_string()))?;

    let selection_mode = match doc.spec.node_selector.mode {
        SelectionMode::Dynamic => "dynamic",
        SelectionMode::Fixed => "fixed",
    };
    let selection_payload = serde_json::to_string(&doc.spec.node_selector)
        .map_err(|e| TemplateAppError::Storage(e.to_string()))?;

    let cache_key = CacheKeyParams {
        template_id: request.template_id,
        template_version: version.version,
        profile: &request.profile,
        selection_mode,
        selection_payload: &selection_payload,
        pool_revision,
    }
    .compute_key();

    if let Some(cached) = cache_repo.find_by_key(&cache_key).await? {
        return Ok(GenerationResult {
            content: cached.content,
            profile: cached.profile,
            included_node_ids: Vec::new(),
            excluded: Vec::new(),
            warnings: vec!["served from cache".to_owned()],
        });
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

    if request.mode == GenerationMode::Strict && !report.excluded.is_empty() {
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

    if node_refs.is_empty() {
        warnings.push("no compatible nodes available for generation".to_owned());
    }

    let content = emit(profile, &node_refs)?;

    validate_output(&content, profile)?;

    let entry = GenerationCacheEntry {
        id: GenerationCacheId::new(),
        template_id: request.template_id,
        template_version: version.version,
        profile: request.profile.clone(),
        selection_mode: selection_mode.to_owned(),
        selection_payload,
        pool_revision: pool_revision.value(),
        cache_key,
        content: content.clone(),
        is_active: false,
    };

    let result = GenerationResult {
        content,
        profile: request.profile,
        included_node_ids: report.included_node_ids,
        excluded: report.excluded,
        warnings,
    };

    cache_repo.store(&entry).await?;
    cache_repo
        .activate(entry.template_id, &entry.profile, entry.id)
        .await?;

    Ok(result)
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
        ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray => {
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
