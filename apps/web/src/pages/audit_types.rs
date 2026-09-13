//! Shared API wire types and local presentation state.

#![cfg(target_family = "wasm")]

pub use deve_sub_contract::{AuditLogDto, ListAuditLogsResponse};

// WHY: these constants must match the action/target_type strings the
// server actually writes (see `deve-sub-application/src/audit/commands.rs`).
// A drift here makes the audit filter silently return zero rows for the
// mismatched actions and hide real audited events from the UI (SV-001).
pub const ACTIONS: &[&str] = &[
    "audit.cleanup",
    "tag.create",
    "tag.update",
    "tag.delete",
    "node.import",
    "node.override.update",
    "node.override.delete",
    "node.region.update",
    "node.chain.update",
    "node.tags.update",
    "node.tags.batch",
    "node.enabled.batch",
    "auth.login",
    "auth.logout",
    "auth.2fa.enable",
    "auth.2fa.disable",
    "user.create",
    "user.disable",
    "user.force_logout",
    "source.create",
    "source.update",
    "source.delete",
    "source.refresh",
    "subscription.create",
    "subscription.update",
    "subscription.delete",
    "subscription.token.rotate",
    "template.create",
    "template.update",
    "template.delete",
    "template.rollback",
    "probe.source.create",
    "probe.source.update",
    "probe.source.delete",
    "probe.source.sync",
    "probe.run.start",
    "probe.run.cancel",
];

pub const TARGET_TYPES: &[&str] = &[
    "audit_log",
    "tag",
    "node",
    "user",
    "source",
    "subscription",
    "template",
    "probe_source",
    "probe_run",
];

/// Feature-local copy; no business policy is implemented in the UI.
pub fn tr(lang: crate::i18n::Language, zh: &'static str, en: &'static str) -> &'static str {
    if lang == crate::i18n::Language::Zh {
        zh
    } else {
        en
    }
}

/// Preserve the stable event code beside its human-readable label.
pub fn action_label(lang: crate::i18n::Language, action: &str) -> String {
    if lang == crate::i18n::Language::En {
        return action.to_string();
    }
    let label = match action {
        "audit.cleanup" => "清理日志",
        "auth.login" => "登录",
        "auth.logout" => "退出登录",
        "auth.2fa.enable" => "启用两步验证",
        "auth.2fa.disable" => "关闭两步验证",
        "user.create" => "创建用户",
        "user.disable" => "禁用用户",
        "user.force_logout" => "强制退出",
        "source.create" => "添加来源",
        "source.update" => "修改来源",
        "source.delete" => "删除来源",
        "source.refresh" => "刷新来源",
        "subscription.create" => "创建订阅",
        "subscription.update" => "修改订阅",
        "subscription.delete" => "删除订阅",
        "subscription.token.rotate" => "轮换订阅令牌",
        "template.create" => "创建模板",
        "template.update" => "修改模板",
        "template.delete" => "删除模板",
        "template.rollback" => "回滚模板",
        "tag.create" => "创建标签",
        "tag.update" => "修改标签",
        "tag.delete" => "删除标签",
        "node.import" => "导入节点",
        "node.override.update" => "修改节点覆盖",
        "node.override.delete" => "恢复节点继承",
        "node.region.update" => "修改地区",
        "node.chain.update" => "修改代理链",
        "node.tags.update" => "修改节点标签",
        "node.tags.batch" => "批量打标",
        "node.enabled.batch" => "批量启停节点",
        "probe.source.create" => "添加探测源",
        "probe.source.update" => "修改探测源",
        "probe.source.delete" => "删除探测源",
        "probe.source.sync" => "同步探测源",
        "probe.run.start" => "启动探测",
        "probe.run.cancel" => "取消探测",
        _ => return action.to_string(),
    };
    format!("{label} · {action}")
}
