//! Nodes page — virtual scroll list, multi-select, filtering, and
//! management actions (import, batch enable/disable, batch tags, per-node
//! override/region/tags/chain).

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::{get, send};
use crate::i18n::{Language, format_t, t};
use crate::pages::node_categories::{NodeCategories, NodeCategory};
use crate::pages::node_chain_modal::ChainModal;
use crate::pages::node_import_modal::ImportModal;
use crate::pages::node_override_modal::{OverrideModal, RegionModal};
use crate::pages::node_selection::NodeSelection;
use crate::pages::node_tag_modal::TagModal;
use crate::pages::node_tag_manager::TagManager;
use crate::pages::node_list::NodeList;
use crate::pages::node_types::{
    BatchEnabledRequest, BatchResultDto, ListNodesResponse, ListTagsResponse, NodeDto, NodeModal, TagDto,
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
    let mut category = use_signal(NodeCategory::default);
    let mut tags = use_signal(Vec::<TagDto>::new);
    let mut tags_loading = use_signal(|| true);
    let mut tags_error = use_signal(String::new);
    let mut nodes_loaded = use_signal(|| false);
    let mut region_filter = use_signal(String::new);
    let mut status_filter = use_signal(String::new);
    let mut batch_busy = use_signal(|| false);
    let mut request_revision = use_signal(|| 0_u64);
    let mut protocol_filter = use_signal(String::new);
    let mut selected = use_signal(NodeSelection::default);
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut cursor = use_signal(|| Option::<String>::None);
    let mut loading_more = use_signal(|| false);
    let mut refreshing = use_signal(|| false);
    let mut modal = use_signal(|| NodeModal::None);
    let mut batch_msg = use_signal(String::new);

    let mut fetch_nodes = move || {
        *request_revision.write() += 1;
        let revision = *request_revision.read();
        refreshing.set(true);
        loading_more.set(false);
        tags_loading.set(true);
        // WHY: catalog identity is independent of node pagination and membership.
        // A failed or superseded read must never make an empty category disappear.
        spawn(async move {
            let result = get::<ListTagsResponse>("/tags").await;
            if revision != *request_revision.read() { return; }
            match result {
                Ok(resp) => {
                    let deleted = matches!(&*category.read(), NodeCategory::Tag(id) if !resp.tags.iter().any(|tag| &tag.id == id));
                    if deleted {
                        category.set(NodeCategory::All);
                        selected.write().clear();
                        scroll_top.set(0.0);
                        reset_list_scroll();
                    }
                    tags.set(resp.tags);
                    tags_error.set(String::new());
                }
                Err(e) => tags_error.set(e.message),
            }
            tags_loading.set(false);
        });
        spawn(async move {
            let result = get::<ListNodesResponse>("/nodes?include_inactive=true&limit=10000").await;
        if revision != *request_revision.read() { return; }
        refreshing.set(false);
        match result {
                Ok(resp) => {
                    error.set(String::new());
                    // WHY: a first-page refresh can unload selected later-page
                    // nodes or reveal that membership moved to another category.
                    let active = category.read();
                    let available = resp.nodes.iter().filter(|node| active.matches(node))
                        .map(|node| node.id.as_str()).collect();
                    selected.write().retain(&available);
                    nodes.set(resp.nodes);
                    nodes_loaded.set(true);
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
        fetch_nodes();
    });

    let load_more = move |_| {
        if *loading_more.read() || *refreshing.read() {
            return;
        }
        let c = cursor.read().clone();
        let Some(c) = c else { return };
        let revision = *request_revision.read();
        loading_more.set(true);
        spawn(async move {
            let path = format!("/nodes?include_inactive=true&limit=100&cursor={c}");
            let result = get::<ListNodesResponse>(&path).await;
            if revision != *request_revision.read() { return; }
            match result {
                Ok(resp) => {
                    error.set(String::new());
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
        let ids: Vec<String> = selected.read().ids().iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        batch_busy.set(true);
        let selection_revision = selected.read().revision();
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
                    selected.write().clear_if_unchanged(selection_revision);
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
            .filter(|n| category.read().matches(n))
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

    let mut tag_counts = std::collections::HashMap::<String, usize>::new();
    for node in all.iter() {
        for tag in &node.tags { *tag_counts.entry(tag.id.clone()).or_default() += 1; }
    }
    let untagged = all.iter().filter(|node| node.tags.is_empty()).count();
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

    let selected_count = selected.read().ids().len();
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
                    onclick: move |_| selected.write().replace(filtered_ids.iter().cloned().collect()), {t(l, "nodes.select_filtered")} }
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
                            let ids: Vec<String> = selected.read().ids().iter().cloned().collect();
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
            NodeCategories { lang: l, tags: tags.read().clone(), counts: tag_counts,
                total: all.len(), untagged, counts_ready: *nodes_loaded.read(), active: category.read().clone(),
                loading: *tags_loading.read(), error: tags_error.read().clone(),
                on_refresh: move |_| fetch_nodes(),
                on_select: move |next| {
                    if *category.read() != next {
                        category.set(next);
                        selected.write().clear();
                        scroll_top.set(0.0);
                        reset_list_scroll();
                    }
                }
            }
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
                select { class: "node-control", aria_label: t(l, "nodes.region"), value: "{region_filter}", onchange: move |e| { region_filter.set(e.value()); scroll_top.set(0.0); reset_list_scroll(); },
                    option { value: "", {t(l, "nodes.all_regions")} }
                    for region in regions { option { value: "{region}", "{region}" } }
                }
                select { class: "node-control", aria_label: t(l, "nodes.status"), value: "{status_filter}", onchange: move |e| { status_filter.set(e.value()); scroll_top.set(0.0); reset_list_scroll(); },
                    option { value: "", {t(l, "nodes.all_statuses")} }
                    option { value: "enabled", {t(l, "nodes.enabled")} }
                    option { value: "disabled", {t(l, "nodes.disabled")} }
                }
                span { class: "text-sm text-stone-600 dark:text-stone-400",
                    {format_t(l, "nodes.matching_count", total)} " / " {format_t(l, "nodes.loaded_count", all.len())} }
            }

            if !error.read().is_empty() {
                div { role: "alert", class: "rounded-md bg-red-50 p-4 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400",
                    if *nodes_loaded.read() { {t(l, "nodes.load_retained")} " " }
                    "{error}"
                }
            }
            if *loading.read() {
                div { class: "flex items-center justify-center py-12",
                    div { class: "h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600 dark:border-stone-700 dark:border-t-amber-500" }
                }
            } else if *nodes_loaded.read() {
                NodeList { lang: props.lang, nodes: visible_items, selected, modal,
                    total, total_height, offset_y, scroll_top, item_height: ITEM_HEIGHT, viewport_height: VIEWPORT_HEIGHT }

                if cursor.read().is_some() {
                    div { class: "flex justify-center pt-4",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            disabled: *loading_more.read() || *refreshing.read(),
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
                    on_success: move |_| { selected.write().clear(); fetch_nodes(); },
                    on_catalog_change: move |_| fetch_nodes(),
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
