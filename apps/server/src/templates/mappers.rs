//! DTO mappers for template management routes.
//!
//! Converts domain types (`SubscriptionTemplate`, `TemplateVersion`,
//! `TemplateResolution`, `CompatibilityReport`) to their contract DTO
//! representations.

use deve_sub_contract::{
    ChainEdgeDto, CompatibilityReportDto, ExcludedNodeDto, GroupResolutionDto, MissingNodeRefDto,
    ResolveTemplateResponse, TemplateDto, TemplateVersionDto,
};
use deve_sub_domain::{SubscriptionTemplate, TemplateVersion};

use crate::auth::ts_to_iso8601;

pub(super) fn template_to_dto(t: &SubscriptionTemplate) -> TemplateDto {
    TemplateDto {
        id: t.id.to_string(),
        name: t.name.clone(),
        description: t.description.clone(),
        active_version: t.active_version,
        active_version_id: t.active_version_id.map(|id| id.to_string()),
        created_at: ts_to_iso8601(t.created_at),
        updated_at: ts_to_iso8601(t.updated_at),
    }
}

pub(super) fn version_to_dto(v: &TemplateVersion) -> TemplateVersionDto {
    TemplateVersionDto {
        id: v.id.to_string(),
        template_id: v.template_id.to_string(),
        version: v.version,
        spec_yaml: v.spec_yaml.clone(),
        is_active: v.is_active,
        created_at: ts_to_iso8601(v.created_at),
    }
}

pub(super) fn compat_report_to_dto(
    r: &deve_sub_domain::CompatibilityReport,
) -> CompatibilityReportDto {
    CompatibilityReportDto {
        profile: r.profile.clone(),
        included_node_ids: r
            .included_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        excluded: r
            .excluded
            .iter()
            .map(|n| ExcludedNodeDto {
                node_id: n.node_id.to_string(),
                display_name: n.display_name.clone(),
                reason: n.reason.clone(),
            })
            .collect(),
    }
}

pub(super) fn resolution_to_dto(
    r: &deve_sub_domain::TemplateResolution,
    chain_graph: &deve_sub_domain::ChainGraph,
) -> ResolveTemplateResponse {
    ResolveTemplateResponse {
        selected_node_ids: r
            .selected_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        selection_missing: r.selection_missing.iter().map(missing_to_dto).collect(),
        groups: r.groups.iter().map(group_resolution_to_dto).collect(),
        chain_edges: chain_graph
            .edges()
            .into_iter()
            .map(|e| ChainEdgeDto {
                from: e.from.to_string(),
                to: e.to.to_string(),
            })
            .collect(),
    }
}

fn missing_to_dto(m: &deve_sub_domain::MissingNodeRef) -> MissingNodeRefDto {
    MissingNodeRefDto {
        node_id: m.node_id.to_string(),
        reason: m.reason.to_string(),
    }
}

fn group_resolution_to_dto(g: &deve_sub_domain::GroupResolution) -> GroupResolutionDto {
    GroupResolutionDto {
        group_name: g.group_name.clone(),
        explicit_node_ids: g
            .explicit_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        quick_group_node_ids: g
            .quick_group_node_ids
            .iter()
            .map(|id| id.to_string())
            .collect(),
        missing: g.missing.iter().map(missing_to_dto).collect(),
    }
}
