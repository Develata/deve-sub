//! Audit retention status and explicit preview-confirm cleanup flow.
#![cfg(target_family = "wasm")]

use super::audit_types::tr;
use crate::{api, i18n::Language};
use deve_sub_contract::{
    AuditCleanupPreviewRequest, AuditCleanupPreviewResponse, AuditCleanupRequest,
    AuditCleanupResponse, AuditPolicyResponse,
};
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct AuditCleanupProps {
    lang: Signal<Language>,
    on_cleaned: EventHandler<()>,
}

pub fn AuditCleanup(props: AuditCleanupProps) -> Element {
    let l = *props.lang.read();
    let mut policy = use_signal(|| None::<AuditPolicyResponse>);
    let mut keep_days = use_signal(|| "90".to_string());
    let mut preview = use_signal(|| None::<AuditCleanupPreviewResponse>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut result = use_signal(String::new);
    use_future(move || async move {
        match api::get::<AuditPolicyResponse>("/audit-logs/policy").await {
            Ok(value) => {
                policy.set(Some(value));
            }
            Err(e) => error.set(e.message),
        }
    });
    let mut do_preview = move |_| {
        if *busy.read() {
            return;
        }
        let Ok(days) = keep_days.read().parse::<u32>() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        result.set(String::new());
        preview.set(None);
        spawn(async move {
            match api::send::<AuditCleanupPreviewResponse, _>(
                "POST",
                "/audit-logs/cleanup/preview",
                Some(&AuditCleanupPreviewRequest { keep_days: days }),
            )
            .await
            {
                Ok(value) => preview.set(Some(value)),
                Err(e) => error.set(e.message),
            }
            busy.set(false);
        });
    };
    let mut confirm = move |_| {
        if *busy.read() {
            return;
        }
        let Some(value) = preview.read().clone() else {
            return;
        };
        if value.entry_ids.is_empty() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        // Consume this preview even on a lost response; the next attempt must
        // inspect current history rather than submit an uncertain batch again.
        preview.set(None);
        spawn(async move {
            let request = AuditCleanupRequest {
                before_unix_ms: value.before_unix_ms,
                entry_ids: value.entry_ids,
            };
            match api::send::<AuditCleanupResponse, _>(
                "POST",
                "/audit-logs/cleanup",
                Some(&request),
            )
            .await
            {
                Ok(done) => result.set(format!(
                    "{} {} · {}",
                    tr(l, "已清理", "Deleted"),
                    done.deleted,
                    done.receipt_id
                )),
                Err(e) => error.set(if e.status == 409 {
                    tr(
                        l,
                        "日志已变化，请重新预览。",
                        "History changed. Preview again.",
                    )
                    .into()
                } else {
                    e.message
                }),
            }
            busy.set(false);
            props.on_cleaned.call(());
        });
    };
    rsx! {
        section { class: "space-y-3 rounded-lg border border-stone-200 p-4 dark:border-stone-800", aria_label: tr(l, "日志保留与清理", "Log retention and cleanup"),
            h3 { class: "font-medium", {tr(l, "日志保留与清理", "Log retention and cleanup")} }
            if let Some(value) = policy.read().as_ref() {
                p { class: "text-sm text-stone-500 dark:text-stone-400",
                    if value.retention_days == 0 { {tr(l, "自动回收已关闭。", "Automatic retention is disabled.")} }
                    else { {tr(l, "自动保留天数：", "Automatic retention days: ")} "{value.retention_days}" }
                    " · " {tr(l, "每分钟检查；配置变更需重启服务。", "Checked every minute; configuration changes require a restart.")}
                }
            }
            p { class: "text-sm text-stone-500 dark:text-stone-400", {tr(l, "清理范围为全部审计日志，不受下方筛选影响。至少保留最近一天；操作不可撤销，需要留档时先备份数据库。", "Cleanup covers all audit events regardless of the filters below. Keep at least one day. Deletion is permanent; back up the database first if you need an archive.")} }
            div { class: "flex flex-wrap items-end gap-3",
                label { class: "text-sm", {tr(l, "手动保留天数", "Days to keep manually")}
                    select { class: "ml-2 rounded-md border border-stone-300 px-3 py-2 dark:border-stone-700 dark:bg-stone-800", value: "{keep_days}", disabled: *busy.read(),
                        onchange: move |e| { keep_days.set(e.value()); preview.set(None); error.set(String::new()); result.set(String::new()); },
                        for days in [1, 7, 30, 90, 180, 365] { option { value: "{days}", "{days}" } }
                    }
                }
                button { class: "rounded-md border border-stone-300 px-4 py-2 text-sm disabled:opacity-50 dark:border-stone-700", disabled: *busy.read(), onclick: move |e| do_preview(e), {tr(l, "预览清理", "Preview cleanup")} }
            }
            if *busy.read() { p { role: "status", {tr(l, "正在处理…", "Working…")} } }
            if !error.read().is_empty() { p { role: "alert", class: "text-sm text-red-600 dark:text-red-400", "{error}" } }
            if !result.read().is_empty() { p { role: "status", class: "break-all text-sm text-emerald-700 dark:text-emerald-400", "{result}" } }
            if let Some(value) = preview.read().as_ref() {
                div { class: "space-y-2 rounded-md bg-amber-50 p-3 text-sm dark:bg-amber-950/30",
                    p { {tr(l, "截止时间（不含，UTC）：", "Exclusive cutoff (UTC): ")} {js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(value.before_unix_ms as f64)).to_iso_string().as_string().unwrap_or_default()} }
                    p { {tr(l, "本批待清理：", "Entries in this batch: ")} "{value.entry_ids.len()}" }
                    if value.has_more { p { {tr(l, "还有更早日志未纳入本批，完成后可再次预览清理。", "More old events remain beyond this batch. Preview again after completion.")} } }
                    if !value.entry_ids.is_empty() {
                        button { class: "rounded-md bg-red-700 px-4 py-2 font-medium text-white disabled:opacity-50", disabled: *busy.read(), onclick: move |e| confirm(e), {tr(l, "确认清理本批", "Confirm batch cleanup")} }
                    }
                    button { class: "ml-3 px-3 py-2", disabled: *busy.read(), onclick: move |_| preview.set(None), {tr(l, "取消", "Cancel")} }
                }
            }
        }
    }
}
