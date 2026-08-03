use std::collections::HashSet;

use dioxus::prelude::*;

use crate::components::{Badge, SearchInput, Select};
use crate::i18n::{format_t, t, Language};
use crate::mock::{generate_nodes, MockNode};

const ITEM_HEIGHT: f64 = 48.0;
const CONTAINER_H: f64 = 600.0;
const BUFFER: usize = 5;
const PER_PAGE: usize = 50;

fn get_scroll_top() -> f64 {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("node-list-scroll"))
        .map_or(0.0, |el| el.scroll_top() as f64)
}

fn set_scroll_top(offset: f64) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("node-list-scroll"))
    {
        el.set_scroll_top(offset as i32);
    }
}

#[component]
pub fn NodesPage(lang: Signal<Language>) -> Element {
    let l = *lang.read();

    let all_nodes = use_signal(|| generate_nodes(10_000));
    let mut search = use_signal(String::new);
    let mut protocol = use_signal(|| "all".to_string());
    let mut selected = use_signal(HashSet::<u32>::new);
    let mut scroll_top = use_signal(|| 0.0);

    let filtered = use_memo(move || {
        let s = search.read();
        let p = protocol.read();
        let p_str = p.as_str();
        all_nodes
            .read()
            .iter()
            .filter(|n| s.is_empty() || n.name.contains(s.as_str()))
            .filter(|n| p_str == "all" || n.protocol == p_str)
            .cloned()
            .collect::<Vec<MockNode>>()
    });

    let total = filtered.read().len();
    let st = *scroll_top.read();
    let start = ((st / ITEM_HEIGHT) as usize).saturating_sub(BUFFER);
    let vis_count = (CONTAINER_H / ITEM_HEIGHT).ceil() as usize + 2 * BUFFER;
    let end = (start + vis_count).min(total);
    let top_h = start as f64 * ITEM_HEIGHT;
    let bottom_h = (total - end) as f64 * ITEM_HEIGHT;

    let cur_page = ((st / (PER_PAGE as f64 * ITEM_HEIGHT)) as usize) + 1;
    let total_pages = (total + PER_PAGE - 1) / PER_PAGE.max(1);

    let all_selected = !filtered.read().is_empty()
        && filtered.read().iter().all(|n| selected.read().contains(&n.id));

    let protocol_options = vec![
        ("all".to_string(), t(l, "nodes.all_protocols").to_string()),
        ("Vless".to_string(), "Vless".to_string()),
        ("VMess".to_string(), "VMess".to_string()),
        ("Trojan".to_string(), "Trojan".to_string()),
        ("Shadowsocks".to_string(), "Shadowsocks".to_string()),
        ("Hysteria2".to_string(), "Hysteria2".to_string()),
        ("TuicV5".to_string(), "TuicV5".to_string()),
        ("NaiveProxy".to_string(), "NaiveProxy".to_string()),
    ];

    rsx! {
        div { class: "space-y-4",
            div { class: "flex flex-col gap-3 sm:flex-row sm:items-center",
                div { class: "flex-1",
                    SearchInput {
                        value: search.read().clone(),
                        placeholder: t(l, "nodes.search").to_string(),
                        on_input: move |v| {
                            search.set(v);
                            scroll_top.set(0.0);
                            set_scroll_top(0.0);
                        },
                    }
                }
                Select {
                    value: protocol.read().clone(),
                    options: protocol_options,
                    on_change: move |v| {
                        protocol.set(v);
                        scroll_top.set(0.0);
                        set_scroll_top(0.0);
                    },
                }
                if !selected.read().is_empty() {
                    span { class: "text-sm font-medium text-amber-600 dark:text-amber-400 whitespace-nowrap",
                        {format_t(l, "nodes.selected", selected.read().len())}
                    }
                }
            }

            div { class: "overflow-hidden rounded-lg border border-stone-200 dark:border-stone-800",
                div { class: "flex h-12 items-center border-b border-stone-200 bg-stone-50 px-4 dark:border-stone-800 dark:bg-stone-800/50",
                    div { class: "w-8",
                        input {
                            r#type: "checkbox",
                            class: "h-4 w-4 rounded border-stone-300 text-amber-600 focus:ring-amber-500",
                            checked: all_selected,
                            onchange: move |_| {
                                let mut sel = selected.write();
                                let ids: HashSet<u32> = filtered.read().iter().map(|n| n.id).collect();
                                if sel.is_superset(&ids) {
                                    sel.retain(|id| !ids.contains(id));
                                } else {
                                    sel.extend(ids);
                                }
                            },
                        }
                    }
                    div { class: "flex-1 text-xs font-semibold uppercase tracking-wide text-stone-400", {t(l, "nodes.search").split("…").next().unwrap_or("Name")} }
                    div { class: "w-24 text-xs font-semibold uppercase tracking-wide text-stone-400", {t(l, "nodes.protocol")} }
                    div { class: "w-20 text-xs font-semibold uppercase tracking-wide text-stone-400", {t(l, "nodes.region")} }
                    div { class: "w-24 text-right text-xs font-semibold uppercase tracking-wide text-stone-400", {t(l, "nodes.latency")} }
                    div { class: "w-20 text-xs font-semibold uppercase tracking-wide text-stone-400", {t(l, "nodes.status")} }
                }

                if total == 0 {
                    div { class: "flex h-48 items-center justify-center text-stone-400", "No results" }
                } else {
                    div {
                        id: "node-list-scroll",
                        class: "overflow-y-auto",
                        style: "max-height: {CONTAINER_H}px",
                        onscroll: move |_| {
                            scroll_top.set(get_scroll_top());
                        },
                        div { style: "height: {top_h}px" }
                        for i in start..end {
                            {
                                let node = filtered.read()[i].clone();
                                let is_sel = selected.read().contains(&node.id);
                                let bg = if is_sel {
                                    "bg-amber-50 dark:bg-amber-900/10"
                                } else if i % 2 == 0 {
                                    "bg-white dark:bg-stone-900"
                                } else {
                                    "bg-stone-50/50 dark:bg-stone-800/30"
                                };
                                rsx! {
                                    div {
                                        key: "{node.id}",
                                        class: "flex h-12 items-center border-b border-stone-100 px-4 dark:border-stone-800 {bg}",
                                        div { class: "w-8",
                                            input {
                                                r#type: "checkbox",
                                                class: "h-4 w-4 rounded border-stone-300 text-amber-600 focus:ring-amber-500",
                                                checked: is_sel,
                                                onchange: move |_| {
                                                    let mut sel = selected.write();
                                                    if sel.contains(&node.id) {
                                                        sel.remove(&node.id);
                                                    } else {
                                                        sel.insert(node.id);
                                                    }
                                                },
                                            }
                                        }
                                        div { class: "flex-1 truncate text-sm font-medium text-stone-700 dark:text-stone-200", "{node.name}" }
                                        div { class: "w-24", Badge { text: node.protocol.clone(), color: Some("amber".to_string()) } }
                                        div { class: "w-20 text-sm text-stone-500 dark:text-stone-400", "{node.region}" }
                                        div { class: "w-24 text-right text-sm tabular-nums text-stone-500 dark:text-stone-400", "{node.latency_ms} ms" }
                                        div { class: "w-20",
                                            if node.enabled {
                                                Badge { text: t(l, "nodes.enabled").to_string(), color: Some("green".to_string()) }
                                            } else {
                                                Badge { text: t(l, "nodes.disabled").to_string(), color: None }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { style: "height: {bottom_h}px" }
                    }
                }
            }

            div { class: "flex items-center justify-between px-2",
                span { class: "text-sm text-stone-500 dark:text-stone-400",
                    "Page {cur_page} of {total_pages.max(1)}  ·  {total} nodes"
                }
                div { class: "flex gap-2",
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-40 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        disabled: cur_page <= 1,
                        onclick: move |_| {
                            let offset = ((cur_page - 2) as f64) * PER_PAGE as f64 * ITEM_HEIGHT;
                            set_scroll_top(offset);
                            scroll_top.set(offset);
                        },
                        "← Prev"
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-40 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        disabled: cur_page >= total_pages,
                        onclick: move |_| {
                            let offset = (cur_page as f64) * PER_PAGE as f64 * ITEM_HEIGHT;
                            set_scroll_top(offset);
                            scroll_top.set(offset);
                        },
                        "Next →"
                    }
                }
            }
        }
    }
}
