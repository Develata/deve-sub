//! Dashboard page — traffic stats, latency overview, source health.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, t};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardTrafficResponse {
    pub total_upload: u64,
    pub total_download: u64,
    pub by_source_kind: Vec<SourceKindBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceKindBreakdown {
    pub source_kind: String,
    pub upload: u64,
    pub download: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficHistoryResponse {
    pub scoped_to_subscription: bool,
    pub points: Vec<TrafficHistoryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficHistoryPoint {
    pub date: String,
    pub total_upload: u64,
    pub total_download: u64,
}

#[derive(Props, Clone, PartialEq)]
pub struct DashboardProps {
    lang: Signal<Language>,
}

pub fn DashboardPage(props: DashboardProps) -> Element {
    let l = *props.lang.read();
    let mut traffic = use_signal(|| Option::<DashboardTrafficResponse>::None);
    let mut history = use_signal(Vec::<TrafficHistoryPoint>::new);
    let mut loading = use_signal(|| true);

    use_future(move || async move {
        match crate::api::get::<DashboardTrafficResponse>("/dashboard/traffic").await {
            Ok(t) => {
                traffic.set(Some(t));
            }
            Err(_) => {
                // WHY: Dashboard is non-blocking — show partial data.
            }
        }
        match crate::api::get::<TrafficHistoryResponse>("/dashboard/traffic/history?days=30").await
        {
            Ok(h) => {
                history.set(h.points);
            }
            Err(_) => {}
        }
        loading.set(false);
    });

    let total_up = traffic.read().as_ref().map_or(0, |t| t.total_upload);
    let total_down = traffic.read().as_ref().map_or(0, |t| t.total_download);

    // SVG chart dimensions.
    let chart_w = 800.0_f64;
    let chart_h = 240.0_f64;
    let padding = 40.0_f64;
    let points = history.read();

    let max_val = points
        .iter()
        .map(|p| (p.total_upload + p.total_download).max(1))
        .max()
        .unwrap_or(1) as f64;

    let day_count = points.len().max(1);
    let bar_width = (chart_w - 2.0 * padding) / day_count as f64 * 0.7;
    let bar_gap = (chart_w - 2.0 * padding) / day_count as f64 * 0.3;

    rsx! {
        div { class: "space-y-6",
            // Stats cards.
            div { class: "grid grid-cols-1 gap-4 sm:grid-cols-3",
                div { class: "rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900",
                    p { class: "text-xs font-medium text-stone-500 dark:text-stone-400", "总上传" }
                    p { class: "mt-1 text-2xl font-bold text-stone-900 dark:text-stone-100", {format_bytes(total_up)} }
                }
                div { class: "rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900",
                    p { class: "text-xs font-medium text-stone-500 dark:text-stone-400", "总下载" }
                    p { class: "mt-1 text-2xl font-bold text-stone-900 dark:text-stone-100", {format_bytes(total_down)} }
                }
                div { class: "rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900",
                    p { class: "text-xs font-medium text-stone-500 dark:text-stone-400", "总计" }
                    p { class: "mt-1 text-2xl font-bold text-stone-900 dark:text-stone-100", {format_bytes(total_up + total_down)} }
                }
            }

            // Traffic chart (30-day).
            div { class: "rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900",
                h3 { class: "mb-4 text-sm font-medium text-stone-700 dark:text-stone-300", "30 天流量趋势" }
                if *loading.read() {
                    div { class: "flex items-center justify-center py-12",
                        div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                    }
                } else if points.is_empty() {
                    p { class: "py-12 text-center text-sm text-stone-400 dark:text-stone-500", "暂无数据" }
                } else {
                    svg {
                        width: "100%",
                        height: "{chart_h}",
                        view_box: "0 0 {chart_w} {chart_h}",
                        class: "overflow-visible",
                        // Grid lines.
                        for i in 0..=4 {
                            {
                                let y = padding + (chart_h - 2.0 * padding) * i as f64 / 4.0;
                                rsx! {
                                    line {
                                        x1: "{padding}",
                                        y1: "{y}",
                                        x2: "{chart_w - padding}",
                                        y2: "{y}",
                                        stroke: "currentColor",
                                        "stroke-width": "0.5",
                                        class: "text-stone-200 dark:text-stone-800",
                                    }
                                }
                            }
                        }
                        // Bars: upload (bottom) + download (top) stacked.
                        for (i, point) in points.iter().enumerate() {
                            {
                                let x = padding + i as f64 * (bar_width + bar_gap);
                                let total = (point.total_upload + point.total_download) as f64;
                                let bar_h = (total / max_val) * (chart_h - 2.0 * padding);
                                let up_h = (point.total_upload as f64 / total.max(1.0)) * bar_h;
                                let down_h = bar_h - up_h;
                                let y_down = chart_h - padding - bar_h;
                                let y_up = y_down + down_h;
                                rsx! {
                                    g {
                                        rect {
                                            x: "{x}",
                                            y: "{y_down}",
                                            width: "{bar_width}",
                                            height: "{down_h}",
                                            fill: "#d97706",
                                            rx: "2",
                                        }
                                        rect {
                                            x: "{x}",
                                            y: "{y_up}",
                                            width: "{bar_width}",
                                            height: "{up_h}",
                                            fill: "#f59e0b",
                                            rx: "2",
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

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{val:.2} {}", UNITS[unit])
    }
}
