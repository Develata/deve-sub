//! Present server-owned daily traffic, with dates, units and a text alternative.
use crate::i18n::{Language, t};
use deve_sub_contract::TrafficHistoryPointDto;
use dioxus::prelude::*;

const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

fn byte_scale(bytes: f64) -> (f64, &'static str) {
    let mut scale = 1.0;
    let mut unit = 0;
    while bytes / scale >= 1024.0 && unit < UNITS.len() - 1 {
        scale *= 1024.0;
        unit += 1;
    }
    (scale, UNITS[unit])
}

/// Format byte counts using the IEC units used by the chart axis.
pub(super) fn format_bytes(bytes: u128) -> String {
    let (scale, unit) = byte_scale(bytes as f64);
    if scale == 1.0 {
        format!("{bytes} {unit}")
    } else {
        format!("{:.2} {unit}", bytes as f64 / scale)
    }
}

/// Render the existing daily history and its screen-reader equivalent.
pub(super) fn traffic_chart(l: Language, points: &[TrafficHistoryPointDto]) -> Element {
    let width = 760.0;
    let height = 240.0;
    let left = 64.0;
    let right = 16.0;
    let top = 16.0;
    let bottom = 32.0;
    let plot_height = height - top - bottom;
    let max = points
        .iter()
        .map(|p| p.total_upload as u128 + p.total_download as u128)
        .max()
        .unwrap_or(0) as f64;
    let (scale, unit) = byte_scale(max);
    let step = (width - left - right) / points.len().max(1) as f64;
    let start = points.first().map(|p| p.date.as_str()).unwrap_or_default();
    let end = points.last().map(|p| p.date.as_str()).unwrap_or_default();

    rsx! {
        figure { class: "mt-5", aria_label: t(l, "dashboard.history"),
            figcaption { class: "flex flex-wrap items-center justify-between gap-3 text-sm text-stone-600 dark:text-stone-400",
                span { "{start} – {end} · {unit}" }
                div { class: "flex gap-4",
                    span { class: "inline-flex items-center gap-2", span { class: "traffic-swatch traffic-upload", aria_hidden: "true" } {t(l, "dashboard.upload")} }
                    span { class: "inline-flex items-center gap-2", span { class: "traffic-swatch traffic-download", aria_hidden: "true" } {t(l, "dashboard.download")} }
                }
            }
            p { id: "traffic-scroll-hint", class: "mt-3 text-xs text-stone-600 dark:text-stone-400 sm:hidden", {t(l, "dashboard.scroll_hint")} }
            div { class: "traffic-chart-scroll mt-3", tabindex: "0", role: "region",
                aria_label: t(l, "dashboard.history"), aria_describedby: "traffic-scroll-hint",
                svg { class: "traffic-chart", width: "100%", height: "{height}", view_box: "0 0 {width} {height}", "aria-hidden": "true",
                    defs {
                        pattern { id: "traffic-upload-pattern", width: "6", height: "6", pattern_units: "userSpaceOnUse",
                            rect { width: "6", height: "6", fill: "var(--color-chart-upload)" }
                            path { d: "M0 6 L6 0", stroke: "var(--color-chart-upload-stripe)", stroke_width: "2" }
                        }
                    }
                    for i in 0..=4 {
                        {
                            let y = top + plot_height * i as f64 / 4.0;
                            let value = max / scale * (4 - i) as f64 / 4.0;
                            rsx! {
                                line { x1: "{left}", y1: "{y}", x2: "{width - right}", y2: "{y}", class: "traffic-grid" }
                                text { x: "{left - 10.0}", y: "{y + 4.0}", text_anchor: "end", class: "traffic-axis", {format!("{value:.2}")} }
                            }
                        }
                    }
                    for (i, point) in points.iter().enumerate() {
                        {
                            let x = left + i as f64 * step + step * 0.15;
                            let up = point.total_upload as f64 / max.max(1.0) * plot_height;
                            let down = point.total_download as f64 / max.max(1.0) * plot_height;
                            rsx! {
                                g {
                                    rect { x: "{x}", y: "{height - bottom - up - down}", width: "{step * 0.7}", height: "{down}", fill: "var(--color-chart-download)" }
                                    rect { x: "{x}", y: "{height - bottom - up}", width: "{step * 0.7}", height: "{up}", fill: "url(#traffic-upload-pattern)" }
                                    if i == 0 || i == points.len() - 1 || i == points.len() / 2 {
                                        text { x: "{x + step * 0.35}", y: "{height - 8.0}", text_anchor: if i == 0 { "start" } else if i == points.len() - 1 { "end" } else { "middle" },
                                            class: "traffic-axis", "{point.date}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // WHY: the text alternative retains native table semantics, while a
            // plain wrapper clips row minimum heights out of the page scroll range.
            div { class: "sr-only",
                table {
                    caption { {t(l, "dashboard.history")} " · UTC" }
                    thead { tr {
                        th { scope: "col", {t(l, "dashboard.date")} }
                        th { scope: "col", {t(l, "dashboard.upload")} }
                        th { scope: "col", {t(l, "dashboard.download")} }
                    } }
                    tbody { for point in points { tr {
                        th { scope: "row", "{point.date}" }
                        td { {format_bytes(point.total_upload as u128)} }
                        td { {format_bytes(point.total_download as u128)} }
                    } } }
                }
            }
        }
    }
}
