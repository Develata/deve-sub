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
                div { class: "overflow-x-auto rounded-lg border border-line-soft",
                    table { class: "w-full text-sm",
                        thead {
                            tr { class: "border-b border-line-soft bg-canvas",
                                th { class: "px-4 py-3 text-left font-medium text-muted", {t(l, "audit.time")} }
                                th { class: "px-4 py-3 text-left font-medium text-muted", {t(l, "audit.action")} }
                                th { class: "px-4 py-3 text-left font-medium text-muted", {t(l, "audit.actor")} }
                                th { class: "px-4 py-3 text-left font-medium text-muted", {t(l, "audit.target")} }
                                th { class: "px-4 py-3 text-left font-medium text-muted", {t(l, "audit.details")} }
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
                                            class: "border-b border-line-soft hover:bg-canvas",
                                            td { class: "px-4 py-3 text-xs text-muted", "{entry.created_at}" }
                                            td { class: "px-4 py-3",
                                                span { class: "font-mono text-xs text-foreground", "{action_label(l, &entry.action)}" }
                                            }
                                            td { class: "px-4 py-3 text-xs text-muted", "{actor}" }
                                            td { class: "px-4 py-3 text-xs text-muted", "{target}" }
                                            td { class: "px-4 py-3 text-xs text-muted",
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
