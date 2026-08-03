use dioxus::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{AddEventListenerOptions, EventSource, MessageEvent};

use crate::chart::{build_chart, x_axis_labels, y_axis_labels, ChartConfig};
use crate::components::{ProgressBar, StatCard};
use crate::i18n::{t, Language};
use crate::mock::generate_traffic_data;

#[component]
pub fn DashboardPage(lang: Signal<Language>) -> Element {
    let l = *lang.read();

    let traffic = generate_traffic_data(30);
    let total_up: f64 = traffic.iter().map(|d| d.upload_gb).sum();
    let total_down: f64 = traffic.iter().map(|d| d.download_gb).sum();
    let net = total_down - total_up;

    let cfg = ChartConfig::default();
    let chart = build_chart(&traffic, &cfg);
    let y_labels = y_axis_labels(chart.y_max, &cfg, 4);
    let x_labels = x_axis_labels(30, &cfg, 6);
    let padding = cfg.padding;
    let w = cfg.width;
    let h = cfg.height;

    let progress = use_signal(|| 0.0);
    use_future(move || async move {
        let es = match EventSource::new("/api/progress") {
            Ok(es) => es,
            Err(_) => return,
        };
        let mut progress = progress.to_owned();
        let cb = Closure::new(move |evt: MessageEvent| {
            if let Some(data) = evt.data().as_string() {
                if let Ok(val) = data.trim().parse::<f64>() {
                    progress.set(val);
                }
            }
        });
        let opts = AddEventListenerOptions::new();
        opts.set_passive(true);
        es.add_event_listener_with_callback_and_add_event_listener_options(
            "message",
            cb.as_ref().unchecked_ref(),
            &opts,
        )
        .expect("add event listener");
        let _es = es;
        let _cb = cb;
        std::future::pending::<()>().await;
    });
    let pct = *progress.read();

    rsx! {
        div { class: "space-y-6",
            div { class: "grid grid-cols-1 gap-4 md:grid-cols-4",
                div { class: "md:col-span-2",
                    StatCard {
                        label: t(l, "dashboard.net_traffic").to_string(),
                        value: format!("{net:.2} GB"),
                        sub: Some(format!("↑ {:.1}% vs yesterday", 12.4)),
                        emphasis: true,
                    }
                }
                StatCard {
                    label: t(l, "dashboard.upload").to_string(),
                    value: format!("{total_up:.0} GB"),
                    sub: None,
                    emphasis: false,
                }
                StatCard {
                    label: t(l, "dashboard.download").to_string(),
                    value: format!("{total_down:.0} GB"),
                    sub: None,
                    emphasis: false,
                }
            }

            div { class: "rounded-lg border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900",
                h3 { class: "mb-4 text-sm font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400",
                    {t(l, "dashboard.traffic_30d")}
                }
                svg {
                    view_box: "0 0 {w} {h}",
                    class: "w-full h-auto",
                    for (y, _) in &y_labels {
                        line {
                            x1: "{padding}", y1: "{y}",
                            x2: "{w - padding}", y2: "{y}",
                            stroke: "#e7e5e4", stroke_width: "1",
                        }
                    }
                    path { d: "{chart.upload_area}", fill: "var(--color-accent)", fill_opacity: "0.08" }
                    path { d: "{chart.download_area}", fill: "#78716c", fill_opacity: "0.04" }
                    path {
                        d: "{chart.upload_path}",
                        stroke: "var(--color-accent)", stroke_width: "2",
                        fill: "none", stroke_linecap: "round", stroke_linejoin: "round",
                    }
                    path {
                        d: "{chart.download_path}",
                        stroke: "#78716c", stroke_width: "1.5",
                        fill: "none", stroke_linecap: "round", stroke_linejoin: "round",
                    }
                    for (y, label) in &y_labels {
                        text { x: "8", y: "{*y + 4.0}", font_size: "10", fill: "#a8a29e", "{label}" }
                    }
                    for (x, label) in &x_labels {
                        text { x: "{*x - 8.0}", y: "{h - 8.0}", font_size: "10", fill: "#a8a29e", "{label}" }
                    }
                    if let Some(&(cx, cy, _)) = chart.points.last() {
                        circle { cx: "{cx}", cy: "{cy}", r: "4", fill: "var(--color-accent)" }
                    }
                }
            }

            div { class: "rounded-lg border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900",
                h3 { class: "mb-4 text-sm font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400",
                    {t(l, "dashboard.refresh_progress")}
                }
                div { class: "flex items-center gap-4",
                    div { class: "flex-1", ProgressBar { value: pct } }
                    span { class: "text-sm font-mono text-stone-500", "{pct:.0}%" }
                }
            }
        }
    }
}
