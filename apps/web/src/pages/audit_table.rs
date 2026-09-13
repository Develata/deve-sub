//! Audit event presentation; immutable server-owned rows.
#![cfg(target_family = "wasm")]
use super::audit_types::{AuditLogDto, action_label, tr};
use crate::i18n::{Language, t};
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct AuditTableProps {
    pub entries: Vec<AuditLogDto>,
    pub lang: Language,
}

pub fn AuditTable(props: AuditTableProps) -> Element {
    let l = props.lang;
    rsx! {
                div { class: "overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-stone-200 bg-stone-50 dark:border-stone-800 dark:bg-stone-900",
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.time")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.action")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.actor")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.target")} }
                                th { class: "px-4 py-3 text-left font-medium text-stone-500 dark:text-stone-400", {t(l, "audit.details")} }
                            }
                        }
                        tbody {
                            for entry in props.entries.iter() {
                                {
                                    let actor = entry.actor_id.clone().unwrap_or_else(|| tr(l, "系统", "System").to_string());
                                    let target = match (&entry.target_type, &entry.target_id) {
                                        (Some(tt), Some(tid)) => format!("{tt}:{tid}"),
                                        (Some(tt), None) => tt.clone(),
                                        _ => "—".to_string(),
                                    };
                                    rsx! {
                                        tr {
                                            key: "{entry.id}",
                                            class: "border-b border-stone-100 hover:bg-stone-50 dark:border-stone-800 dark:hover:bg-stone-800/50",
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{entry.created_at}" }
                                            td { class: "px-4 py-3",
                                                span { class: "font-mono text-xs text-stone-700 dark:text-stone-300", "{action_label(l, &entry.action)}" }
                                            }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{actor}" }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400", "{target}" }
                                            td { class: "px-4 py-3 text-xs text-stone-500 dark:text-stone-400",
                                                if let Some(d) = &entry.details_json {
                                                    details {
                                                        summary { class: "cursor-pointer", {tr(l, "展开详情", "Show details")} }
                                                        pre { class: "mt-2 max-w-sm overflow-auto whitespace-pre-wrap break-all", "{d}" }
                                                    }
                                                } else {
                                                    "—"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
    }
}
