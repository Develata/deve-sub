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

use deve_sub_compatibility::{ProfileKind, capability_for, check_group_type};
use deve_sub_domain::source::NodePoolRepository;
use deve_sub_domain::template::{
    GroupMember, ProxyGroup, TemplateRepository, TemplateVersionRepository,
};
use deve_sub_domain::{
    CacheKeyParams, GenerationCacheEntry, GenerationCacheRepository, GenerationError,
    GenerationMode, GenerationRequest, GenerationResult, IncompatibleGroup, Node,
    PoolMetaRepository, SelectionMode, TemplateVersion,
};
use deve_sub_emitter::{
    AssembledGroup, AssembledTemplate, emit_json, emit_mihomo_full, emit_shadowrocket,
    emit_singbox, emit_uri_list, emit_v2ray, emit_xray,
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

    // WHY: validate the requested profile is declared in the template's
    // targetProfiles. Without this, a template authored for mihomo-only
    // could be generated for sing-box, silently producing a proxy-only
    // document that drops the template's groups/rules/dns/tun (constraint
    // #7: no silent dropping).
    if !doc.spec.target_profiles.is_empty() && !doc.spec.target_profiles.contains(&request.profile)
    {
        return Err(TemplateAppError::InvalidInput(format!(
            "profile '{}' is not in template target_profiles {:?}",
            request.profile, doc.spec.target_profiles
        )));
    }

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

    let name_by_id: std::collections::HashMap<NodeId, String> = nodes
        .iter()
        .map(|(n, _)| (n.id, n.display_name.clone()))
        .collect();

    let node_refs: Vec<Node> = nodes.into_iter().map(|(n, _)| n).collect();

    // WHY: returning an error here — before `emit` and before any cache
    // mutation — ensures the prior active generation remains served when the
    // pool is temporarily empty (constraint #19). Emitting an empty-but-
    // structurally-valid document would silently replace the last good
    // subscription with a blank one.
    if node_refs.is_empty() {
        return Err(TemplateAppError::NoCompatibleNodes);
    }

    // WHY: check each proxy group's type against the profile capability
    // matrix. Without this, a template with a `relay` group generated for
    // sing-box (which rejects relay) would silently emit a malformed or
    // dropped group (constraint #7: no silent dropping).
    let cap = capability_for(profile);
    let mut incompatible_groups: Vec<IncompatibleGroup> = Vec::new();
    for spec_g in &doc.spec.proxy_groups {
        if let Err(reason) = check_group_type(spec_g.group_type, &cap) {
            incompatible_groups.push(IncompatibleGroup {
                group_name: spec_g.name.clone(),
                group_type: spec_g.group_type.to_string(),
                reason: reason.to_string(),
            });
        }
    }
    if !incompatible_groups.is_empty() {
        if mode == GenerationMode::Strict {
            let names = incompatible_groups
                .iter()
                .map(|g| format!("{} ({})", g.group_name, g.group_type))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(TemplateAppError::Generation(
                GenerationError::IncompatibleGroupTypes {
                    count: incompatible_groups.len(),
                    names,
                    groups: incompatible_groups,
                },
            ));
        }
        for g in &incompatible_groups {
            warnings.push(format!(
                "group '{}': type {} incompatible with profile '{}': {}",
                g.group_name,
                g.group_type,
                profile.as_kebab(),
                g.reason
            ));
        }
    }

    let groups = assemble_groups(&doc.spec.proxy_groups, &resolution.groups, &name_by_id);

    // WHY: profiles other than mihomo do not yet have full-template
    // emitters. Emitting only nodes would silently drop the template's
    // groups/rules/dns/tun (constraint #7). Warn so the user knows the
    // output is proxy-only; mihomo gets the full document.
    if profile != ProfileKind::Mihomo
        && (!groups.is_empty()
            || !doc.spec.rules.is_empty()
            || !doc.spec.dns.is_null()
            || !doc.spec.tun.is_null())
    {
        warnings.push(format!(
            "profile '{}' does not yet emit groups/rules/dns/tun; output is proxy-only",
            profile.as_kebab()
        ));
    }

    let template = AssembledTemplate {
        nodes: node_refs,
        groups,
        rules: doc.spec.rules.iter().map(|r| r.value.clone()).collect(),
        dns: doc.spec.dns.clone(),
        tun: doc.spec.tun.clone(),
        output: doc.spec.output.clone(),
    };

    let content = emit(profile, &template)?;

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
    // WHY: the cache stores only `content`, not the CompatibilityReport.
    // Returning empty `included_node_ids`/`excluded` without explanation
    // would imply "zero nodes included, zero excluded" — a false report.
    // The warning honestly states the report is unavailable from cache.
    GenerationResult {
        content: cached.content.clone(),
        profile: cached.profile.clone(),
        included_node_ids: Vec::new(),
        excluded: Vec::new(),
        warnings: vec![
            "served from cache".to_owned(),
            "included/excluded report unavailable from cache".to_owned(),
        ],
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

/// Resolve proxy group specs into assembled groups with display-name members.
fn assemble_groups(
    spec_groups: &[ProxyGroup],
    resolutions: &[deve_sub_domain::template::GroupResolution],
    name_by_id: &std::collections::HashMap<NodeId, String>,
) -> Vec<AssembledGroup> {
    let res_by_name: std::collections::HashMap<&str, &deve_sub_domain::template::GroupResolution> =
        resolutions
            .iter()
            .map(|r| (r.group_name.as_str(), r))
            .collect();

    spec_groups
        .iter()
        .map(|spec| {
            let res = res_by_name.get(spec.name.as_str());
            let mut members: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();

            for member in &spec.members {
                match member {
                    GroupMember::Node { id } => {
                        if let Some(name) = name_by_id.get(id)
                            && seen.insert(name.clone())
                        {
                            members.push(name.clone());
                        }
                    }
                    GroupMember::Group { name } => {
                        if seen.insert(name.clone()) {
                            members.push(name.clone());
                        }
                    }
                }
            }

            if let Some(r) = res {
                for id in r
                    .quick_group_node_ids
                    .iter()
                    .chain(r.explicit_node_ids.iter())
                {
                    if let Some(name) = name_by_id.get(id)
                        && seen.insert(name.clone())
                    {
                        members.push(name.clone());
                    }
                }
            }

            if let Some(sort) = spec.sort_order {
                match sort {
                    deve_sub_domain::SortOrder::Asc | deve_sub_domain::SortOrder::Latency => {
                        members.sort();
                    }
                    deve_sub_domain::SortOrder::Desc => {
                        members.sort();
                        members.reverse();
                    }
                }
            }

            AssembledGroup {
                name: spec.name.clone(),
                group_type: spec.group_type,
                members,
            }
        })
        .collect()
}

fn emit(profile: ProfileKind, template: &AssembledTemplate) -> Result<String, TemplateAppError> {
    match profile {
        ProfileKind::Mihomo => emit_mihomo_full(template),
        ProfileKind::SingBox => emit_singbox(&template.nodes),
        ProfileKind::Xray => emit_xray(&template.nodes),
        ProfileKind::V2Ray => emit_v2ray(&template.nodes),
        ProfileKind::Shadowrocket => emit_shadowrocket(&template.nodes),
        ProfileKind::UriList => emit_uri_list(&template.nodes),
        ProfileKind::Json => emit_json(&template.nodes),
    }
    .map_err(|e| TemplateAppError::Emit(e.to_string()))
}

fn validate_output(content: &str, profile: ProfileKind) -> Result<(), TemplateAppError> {
    if content.trim().is_empty() {
        return Err(TemplateAppError::EmptyOutput);
    }
    match profile {
        ProfileKind::Mihomo => {
            let v: serde_yaml::Value =
                serde_yaml::from_str(content).map_err(|_| TemplateAppError::EmptyOutput)?;
            let proxies = v.get("proxies").ok_or_else(|| {
                TemplateAppError::InvalidStructure("missing 'proxies' key".into())
            })?;
            let arr = proxies.as_sequence().ok_or_else(|| {
                TemplateAppError::InvalidStructure("'proxies' is not an array".into())
            })?;
            if arr.is_empty() {
                return Err(TemplateAppError::InvalidStructure(
                    "'proxies' array is empty".into(),
                ));
            }
        }
        ProfileKind::SingBox | ProfileKind::Xray | ProfileKind::V2Ray | ProfileKind::Json => {
            let v: serde_json::Value =
                serde_json::from_str(content).map_err(|_| TemplateAppError::EmptyOutput)?;
            if !v.is_array() && !v.is_object() {
                return Err(TemplateAppError::InvalidStructure(
                    "expected top-level array or object".into(),
                ));
            }
        }
        // WHY: uri_list and shadowrocket are free-form line-oriented text.
        // The only structural invariant is "has at least one non-empty line",
        // which the `trim().is_empty()` check above already enforces; no
        // further schema exists to validate.
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
    fn validate_output_mihomo_missing_proxies_rejected() {
        let yaml = "proxy-groups: []\nrules: []\n";
        let err = validate_output(yaml, ProfileKind::Mihomo)
            .expect_err("mihomo without proxies must fail structural validation");
        assert!(
            matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("proxies")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_output_mihomo_empty_proxies_array_rejected() {
        let yaml = "proxies: []\n";
        let err =
            validate_output(yaml, ProfileKind::Mihomo).expect_err("empty proxies array must fail");
        assert!(
            matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("empty")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_output_mihomo_proxies_not_array_rejected() {
        let yaml = "proxies: notarray\n";
        let err =
            validate_output(yaml, ProfileKind::Mihomo).expect_err("non-array proxies must fail");
        assert!(
            matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("not an array")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_output_json_scalar_rejected() {
        let err = validate_output("\"just a string\"", ProfileKind::SingBox)
            .expect_err("JSON scalar must fail structural validation");
        assert!(
            matches!(err, TemplateAppError::InvalidStructure(ref m) if m.contains("array or object")),
            "got {err:?}"
        );
        let err =
            validate_output("42", ProfileKind::Xray).expect_err("JSON number scalar must fail");
        assert!(
            matches!(err, TemplateAppError::InvalidStructure(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_output_json_array_accepted() {
        assert!(validate_output("[]", ProfileKind::SingBox).is_ok());
        assert!(validate_output(r#"{"outbounds":[]}"#, ProfileKind::V2Ray).is_ok());
    }

    #[test]
    fn validate_output_uri_list_only_blank_lines_rejected() {
        let err = validate_output("   \n\n  \n", ProfileKind::UriList)
            .expect_err("blank-only uri_list must fail");
        assert!(
            matches!(err, TemplateAppError::EmptyOutput),
            "blank-only uri_list should be EmptyOutput, got {err:?}"
        );
        let err = validate_output("   \n\n  \n", ProfileKind::Shadowrocket)
            .expect_err("blank-only shadowrocket must fail");
        assert!(matches!(err, TemplateAppError::EmptyOutput), "got {err:?}");
    }

    #[test]
    fn validate_output_uri_list_with_content_accepted() {
        assert!(validate_output("trojan://a@b:1\ntrojan://c@d:2\n", ProfileKind::UriList).is_ok());
    }

    #[test]
    fn emit_dispatches_all_profiles() {
        let template = AssembledTemplate::from_nodes(vec![]);
        assert!(emit(ProfileKind::Mihomo, &template).is_ok());
        assert!(emit(ProfileKind::SingBox, &template).is_ok());
        assert!(emit(ProfileKind::Xray, &template).is_ok());
        assert!(emit(ProfileKind::V2Ray, &template).is_ok());
        assert!(emit(ProfileKind::UriList, &template).is_ok());
    }
}
