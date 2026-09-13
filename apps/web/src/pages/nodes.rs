//! Nodes page — virtual scroll list, multi-select, filtering, and
//! management actions (import, batch enable/disable, batch tags, per-node
//! override/region/tags/chain).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::{get, send};
use crate::i18n::{Language, format_t, t};
use crate::pages::node_chain_modal::ChainModal;
use crate::pages::node_import_modal::ImportModal;
use crate::pages::node_override_modal::{OverrideModal, RegionModal};
use crate::pages::node_tag_modal::TagModal;
use crate::pages::node_tag_manager::TagManager;
use crate::pages::node_list::NodeList;
use crate::pages::node_types::{
    BatchEnabledRequest, BatchResultDto, ListNodesResponse, NodeDto, NodeModal,
};

const ITEM_HEIGHT: f64 = 72.0;
const VIEWPORT_HEIGHT: f64 = 600.0;
const BUFFER: usize = 5;
#[derive(Props, Clone, PartialEq)]
pub struct NodesProps {
    lang: Signal<Language>,
}

pub fn NodesPage(props: NodesProps) -> Element {
    let l = *props.lang.read();
    let mut nodes = use_signal(Vec::<NodeDto>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(String::new);
    let mut search = use_signal(String::new);
    let mut tag_filter = use_signal(String::new);
    let mut region_filter = use_signal(String::new);
    let mut status_filter = use_signal(String::new);
    let mut batch_busy = use_signal(|| false);
    let mut request_revision = use_signal(|| 0_u64);
    let mut protocol_filter = use_signal(String::new);
    let mut selected = use_signal(std::collections::HashSet::<String>::new);
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut cursor = use_signal(|| Option::<String>::None);
    let mut loading_more = use_signal(|| false);
    let mut modal = use_signal(|| NodeModal::None);
    let mut batch_msg = use_signal(String::new);

    let mut fetch_nodes = move || {
        *request_revision.write() += 1;
        let revision = *request_revision.read();
        spawn(async move {
            let result = get::<ListNodesResponse>("/nodes?include_inactive=true&limit=10000").await;
        if revision != *request_revision.read() { return; }
        match result {
                Ok(resp) => {
                    error.set(String::new());
                    if !tag_filter.read().is_empty() && !resp.nodes.iter().any(|node| node.tags.iter().any(|tag| tag.id == *tag_filter.read())) { tag_filter.set(String::new()); }
                    nodes.set(resp.nodes);
                    cursor.set(resp.next_cursor);
                    loading.set(false);
                }
                Err(e) => {
                    error.set(e.message);
                    loading.set(false);
                }
            }
        });
    };

    use_future(move || async move {
        let revision = *request_revision.read();
        let result = get::<ListNodesResponse>("/nodes?include_inactive=true&limit=10000").await;
        if revision != *request_revision.read() { return; }
        match result {
            Ok(resp) => {
                error.set(String::new());
                nodes.set(resp.nodes);
                cursor.set(resp.next_cursor);
                loading.set(false);
            }
            Err(e) => {
                error.set(e.message);
                loading.set(false);
            }
        }
    });

    let load_more = move |_| {
        if *loading_more.read() {
            return;
        }
        let c = cursor.read().clone();
        let Some(c) = c else { return };
        let revision = *request_revision.read();
        loading_more.set(true);
        spawn(async move {
            let path = format!("/nodes?include_inactive=true&limit=100&cursor={c}");
            let result = get::<ListNodesResponse>(&path).await;
            if revision != *request_revision.read() { loading_more.set(false); return; }
            match result {
                Ok(resp) => {
                    nodes.write().extend(resp.nodes);
                    cursor.set(resp.next_cursor);
                    loading_more.set(false);
                }
                Err(e) => {
                    error.set(e.message);
                    loading_more.set(false);
                }
            }
        });
    };

    let mut batch_set_enabled = move |enabled: bool| {
        if *batch_busy.read() { return; }
        let ids: Vec<String> = selected.read().iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        batch_busy.set(true);
        batch_msg.set(String::new());
        spawn(async move {
            let req = BatchEnabledRequest {
                node_ids: ids,
                enabled,
            };
            match send::<BatchResultDto, _>("POST", "/nodes/batch-enabled", Some(&req)).await {
                Ok(r) => {
                    batch_msg.set(format_t(
                        l,
                        if enabled {
                            "nodes.batch_enabled_ok"
                        } else {
                            "nodes.batch_disabled_ok"
                        },
                        r.updated as usize,
                    ));
                    fetch_nodes();
                    selected.write().clear();
                }
                Err(e) => {
                    batch_msg.set(e.message);
                }
            }
            batch_busy.set(false);
        });
    };

    let all = nodes.read();
    let filtered: Vec<&NodeDto> = {
        let s = search.read().to_lowercase();
        let p = protocol_filter.read().clone();
        all.iter()
            .filter(|n| s.is_empty() || n.display_name.to_lowercase().contains(&s))
            .filter(|n| p.is_empty() || n.protocol == p)
            .filter(|n| tag_filter.read().is_empty() || n.tags.iter().any(|tag| tag.id == *tag_filter.read()))
            .filter(|n| region_filter.read().is_empty() || n.region.as_deref() == Some(region_filter.read().as_str()))
            .filter(|n| status_filter.read().is_empty() || n.is_active == (*status_filter.read() == "enabled"))
            .collect()
    };

    let protocols: Vec<String> = {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for n in nodes.read().iter() {
            set.insert(n.protocol.clone());
        }
        set.into_iter().collect()
    };

    let tags: std::collections::BTreeMap<String, String> = all.iter().flat_map(|n| n.tags.iter().map(|t| (t.id.clone(), t.name.clone()))).collect();
    let regions: std::collections::BTreeSet<String> = all.iter().filter_map(|n| n.region.clone()).collect();
    let filtered_ids: Vec<String> = filtered.iter().map(|n| n.id.clone()).collect();
    let total = filtered.len();
    let total_height = total as f64 * ITEM_HEIGHT;
    let current_scroll = *scroll_top.read();
    let visible_count = ((VIEWPORT_HEIGHT / ITEM_HEIGHT) as usize) + 2 * BUFFER;
    // WHY: a filter or refresh may shorten the list before the DOM scroll
    // event arrives; the window must remain valid even during that render.
    let start_idx = ((current_scroll / ITEM_HEIGHT) as usize)
        .saturating_sub(BUFFER)
        .min(total.saturating_sub(visible_count));
    let end_idx = (start_idx + visible_count).min(total);
    let visible_items: Vec<NodeDto> = filtered
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
        .map(|node| (*node).clone())
        .collect();
    let offset_y = start_idx as f64 * ITEM_HEIGHT;

    let selected_count = selected.read().len();
    let has_selection = selected_count > 0;

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between",
                h2 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100", {t(l, "nodes.title")} }
                if selected_count > 0 {
                    span { class: "text-sm text-amber-600 dark:text-amber-500", {format_t(l, "nodes.selected", selected_count)} }
                }
            }

            // Toolbar.
            div { class: "flex flex-wrap items-center gap-2",
                button {
                    class: "rounded-md bg-amber-600 px-3 py-2 text-sm font-medium text-white hover:bg-amber-700",
                    onclick: move |_| modal.set(NodeModal::Import),
                    {t(l, "nodes.import_btn")}
                }
                button { class: "node-control", onclick: move |_| modal.set(NodeModal::ManageTags), {t(l, "nodes.manage_tags")} }
                button { class: "node-control", disabled: total == 0 || *batch_busy.read(),
                    onclick: move |_| selected.set(filtered_ids.iter().cloned().collect()), {t(l, "nodes.select_filtered")} }
                if has_selection {
                    button {
                        class: "rounded-md border border-green-300 px-3 py-2 text-sm text-green-700 hover:bg-green-50 dark:border-green-700 dark:text-green-400 dark:hover:bg-green-900/20",
                        disabled: *batch_busy.read(), onclick: move |_| batch_set_enabled(true),
                        {t(l, "nodes.batch_enable")}
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        disabled: *batch_busy.read(), onclick: move |_| batch_set_enabled(false),
                        {t(l, "nodes.batch_disable")}
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                        onclick: move |_| {
                            let ids: Vec<String> = selected.read().iter().cloned().collect();
                            modal.set(NodeModal::Tags(ids));
                        },
                        {t(l, "nodes.batch_tags")}
                    }
                    button {
                        class: "rounded-md border border-stone-300 px-3 py-2 text-sm text-stone-500 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800",
                        onclick: move |_| selected.write().clear(),
                        {t(l, "nodes.clear_selection")}
                    }
                }
            }

            if !batch_msg.read().is_empty() {
                div { class: "rounded-md bg-blue-50 p-3 text-sm text-blue-700 dark:bg-blue-900/20 dark:text-blue-400", "{batch_msg}" }
            }

            // Filters.
            div { class: "flex flex-wrap items-center gap-3",
                input {
                    class: "flex-1 rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                    r#type: "search",
                    placeholder: {t(l, "nodes.search")},
                    value: "{search}",
                    oninput: move |e| {
                        search.set(e.value());
                        scroll_top.set(0.0);
                        reset_list_scroll();
                    },
                }
                select {
                    class: "rounded-md border border-stone-300 px-3 py-2 text-sm shadow-sm focus:border-amber-500 focus:outline-none focus:ring-1 focus:ring-amber-500 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100",
                    value: "{protocol_filter}",
                    onchange: move |e| {
                        protocol_filter.set(e.value());
                        scroll_top.set(0.0);
                        reset_list_scroll();
                    },
                    option { value: "", {t(l, "nodes.all_protocols")} }
                    for p in &protocols {
                        option { value: "{p}", "{p}" }
                    }
                }
            }

            div { class: "flex flex-wrap items-center gap-2",
                select { class: "node-control", aria_label: t(l, "nodes.filter_tags"), value: "{tag_filter}", onchange: move |e| { tag_filter.set(e.value()); scroll_top.set(0.0); reset_list_scroll(); },
                    option { value: "", {t(l, "nodes.all_tags")} }
                    for (id, name) in tags { option { value: "{id}", "{name}" } }
                }
                select { class: "node-control", aria_label: t(l, "nodes.region"), value: "{region_filter}", onchange: move |e| { region_filter.set(e.value()); scroll_top.set(0.0); reset_list_scroll(); },
                    option { value: "", {t(l, "nodes.all_regions")} }
                    for region in regions { option { value: "{region}", "{region}" } }
                }
                select { class: "node-control", aria_label: t(l, "nodes.status"), value: "{status_filter}", onchange: move |e| { status_filter.set(e.value()); scroll_top.set(0.0); reset_list_scroll(); },
                    option { value: "", {t(l, "nodes.all_statuses")} }
                    option { value: "enabled", {t(l, "nodes.enabled")} }
                    option { value: "disabled", {t(l, "nodes.disabled")} }
                }
                span { class: "text-sm text-stone-500", "{total} / {all.len()}" }
            }

            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if !error.read().is_empty() {
                div { class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
            } else {
                NodeList { lang: props.lang, nodes: visible_items, selected, modal,
                    total, total_height, offset_y, scroll_top, item_height: ITEM_HEIGHT, viewport_height: VIEWPORT_HEIGHT }

                if cursor.read().is_some() {
                    div { class: "flex justify-center pt-4",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            disabled: *loading_more.read(),
                            onclick: load_more,
                            if *loading_more.read() { {t(l, "common.loading")} } else { {t(l, "nodes.load_more")} }
                        }
                    }
                }
            }
        }

        // Modals.
        match &*modal.read() {
            NodeModal::None => rsx! {},
            NodeModal::ManageTags => rsx! { TagManager { lang: props.lang,
                on_close: move |_| modal.set(NodeModal::None), on_success: move |_| fetch_nodes() } },
            NodeModal::Import => rsx! {
                ImportModal {
                    lang: props.lang,
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            },
            NodeModal::Tags(ids) => rsx! {
                TagModal {
                    lang: props.lang,
                    node_ids: ids.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            },
            NodeModal::SetRegion(id) => rsx! {
                RegionModal {
                    lang: props.lang,
                    node_id: id.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            },
            NodeModal::Override(id) => rsx! {
                OverrideModal {
                    lang: props.lang,
                    node_id: id.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            },
            NodeModal::Chain(id, chain) => rsx! {
                ChainModal {
                    lang: props.lang,
                    node_id: id.clone(),
                    initial_chain: chain.clone(),
                    on_close: move |_| modal.set(NodeModal::None),
                    on_success: move |_| fetch_nodes(),
                }
            },
        }
    }
}

fn reset_list_scroll() {
    if let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("nodes-scroll"))
    {
        element.set_scroll_top(0);
    }
}
