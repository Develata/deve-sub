//! Dashboard traffic summary and history with independent request states.
#![cfg(target_family = "wasm")]

use crate::i18n::{Language, t};
use crate::pages::dashboard_chart::{format_bytes, traffic_chart};
use deve_sub_contract::{DashboardTrafficResponse, TrafficHistoryResponse};
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct DashboardProps {
    lang: Signal<Language>,
}

pub fn DashboardPage(props: DashboardProps) -> Element {
    let l = *props.lang.read();
    let mut traffic = use_signal(|| None::<Result<DashboardTrafficResponse, String>>);
    let mut history = use_signal(|| None::<Result<TrafficHistoryResponse, String>>);
    let mut load_traffic = move || {
        traffic.set(None);
        spawn(async move {
            traffic.set(Some(
                crate::api::get("/dashboard/traffic")
                    .await
                    .map_err(|e| e.message),
            ));
        });
    };
    let mut load_history = move || {
        history.set(None);
        spawn(async move {
            history.set(Some(
                crate::api::get("/dashboard/traffic/history?days=30")
                    .await
                    .map_err(|e| e.message),
            ));
        });
    };
    use_future(move || async move {
        // WHY: one unavailable endpoint must not hold the other region in loading.
        load_traffic();
        load_history();
    });

    rsx! {
        div { class: "space-y-6",
            h2 { class: "text-lg font-semibold", {t(l, "nav.dashboard")} }
            section { aria_label: t(l, "dashboard.summary"), aria_busy: traffic.read().is_none(),
                match &*traffic.read() {
                    None => rsx! { p { class: "dashboard-state", role: "status", {t(l, "dashboard.loading_summary")} } },
                    Some(Err(message)) => rsx! {
                        DashboardError { title: t(l, "dashboard.summary_failed"), message: message.clone(), lang: l,
                            on_retry: move |_| load_traffic() }
                    },
                    Some(Ok(data)) => rsx! {
                        dl { class: "grid grid-cols-1 gap-4 sm:grid-cols-3",
                            for (label, value) in [
                                ("dashboard.total_upload", data.total_upload as u128),
                                ("dashboard.total_download", data.total_download as u128),
                                ("dashboard.total", data.total_upload as u128 + data.total_download as u128),
                            ] {
                                div { class: "dashboard-card",
                                    dt { class: "text-sm text-stone-600 dark:text-stone-400", {t(l, label)} }
                                    dd { class: "mt-2 text-2xl font-semibold tabular-nums", {format_bytes(value)} }
                                }
                            }
                        }
                    },
                }
            }
            section { class: "dashboard-card", aria_label: t(l, "dashboard.history"), aria_busy: history.read().is_none(),
                h3 { class: "text-base font-semibold", {t(l, "dashboard.history")} }
                p { class: "mt-1 text-sm text-stone-600 dark:text-stone-400", {t(l, "dashboard.daily_utc")} }
                match &*history.read() {
                    None => rsx! { p { class: "dashboard-state", role: "status", {t(l, "dashboard.loading_history")} } },
                    Some(Err(message)) => rsx! {
                        div { class: "mt-4",
                            DashboardError { title: t(l, "dashboard.history_failed"), message: message.clone(), lang: l,
                                on_retry: move |_| load_history() }
                        }
                    },
                    Some(Ok(data)) if data.points.is_empty() => rsx! {
                        p { class: "py-12 text-center text-sm text-stone-600 dark:text-stone-400", {t(l, "dashboard.empty_history")} }
                    },
                    Some(Ok(data)) => traffic_chart(l, &data.points),
                }
            }
        }
    }
}

#[component]
fn DashboardError(
    title: String,
    message: String,
    lang: Language,
    on_retry: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "ui-error flex flex-wrap items-center justify-between gap-3",
            div { role: "alert", class: "min-w-0 break-words",
                p { class: "font-medium", "{title}" }
                p { class: "mt-1 text-sm", "{message}" }
            }
            button { class: "ui-button", onclick: move |_| on_retry.call(()), {t(lang, "dashboard.retry")} }
        }
    }
}
