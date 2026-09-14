//! Visible manual categories derived from the tag catalog, not node pages.
#![cfg(target_family = "wasm")]

use super::node_types::{NodeDto, TagDto};
use crate::i18n::{Language, t};
use dioxus::prelude::*;
use std::collections::HashMap;

/// Presentation filter; tag IDs retain identity through renames and empty sets.
#[derive(Clone, Default, PartialEq)]
pub enum NodeCategory {
    #[default]
    All,
    Untagged,
    Tag(String),
}

impl NodeCategory {
    /// Intersect this category with the other loaded-node filters.
    pub fn matches(&self, node: &NodeDto) -> bool {
        match self {
            Self::All => true,
            Self::Untagged => node.tags.is_empty(),
            Self::Tag(id) => node.tags.iter().any(|tag| &tag.id == id),
        }
    }
}

/// Catalog metadata and counts are supplied by the node page's current reads.
#[derive(Props, Clone, PartialEq)]
pub struct NodeCategoriesProps {
    lang: Language,
    tags: Vec<TagDto>,
    counts: HashMap<String, usize>,
    total: usize,
    untagged: usize,
    counts_ready: bool,
    active: NodeCategory,
    loading: bool,
    error: String,
    on_select: EventHandler<NodeCategory>,
    on_refresh: EventHandler<()>,
}

/// Render wrapping category buttons with keyboard, selected and retry states.
pub fn NodeCategories(props: NodeCategoriesProps) -> Element {
    let l = props.lang;
    let count = |n: usize| if props.counts_ready { n.to_string() } else { "—".into() };
    rsx! {
        section { class: "node-categories", aria_label: t(l, "nodes.categories"),
            div { class: "flex flex-wrap items-center justify-between gap-x-4 gap-y-1",
                div { class: "flex flex-wrap items-baseline gap-x-3 gap-y-1",
                    h3 { class: "text-sm font-semibold", {t(l, "nodes.categories")} }
                    p { class: "text-xs text-stone-600 dark:text-stone-400", {t(l, "nodes.category_counts_hint")} }
                }
                button { class: "node-row-action px-2", onclick: move |_| props.on_refresh.call(()),
                    {t(l, "nodes.refresh_categories")} }
            }
            div { class: "node-category-options", role: "group", aria_label: t(l, "nodes.filter_tags"),
                button { class: "node-category", "data-node-category": "all", aria_pressed: props.active == NodeCategory::All,
                    onclick: move |_| props.on_select.call(NodeCategory::All),
                    {t(l, "nodes.all_nodes")} span { class: "node-category-count", {count(props.total)} }
                }
                button { class: "node-category", "data-node-category": "untagged", aria_pressed: props.active == NodeCategory::Untagged,
                    onclick: move |_| props.on_select.call(NodeCategory::Untagged),
                    {t(l, "nodes.untagged")} span { class: "node-category-count", {count(props.untagged)} }
                }
                for tag in &props.tags {
                    { let id = tag.id.clone(); rsx! {
                        button { key: "{tag.id}", class: "node-category", "data-node-category": "{tag.id}",
                            aria_pressed: props.active == NodeCategory::Tag(tag.id.clone()),
                            onclick: move |_| props.on_select.call(NodeCategory::Tag(id.clone())),
                            span { class: "node-tag-dot", style: format!("background-color: {}", tag.color.as_deref().unwrap_or("#b45309")), aria_hidden: "true" }
                            span { class: "node-category-name", "{tag.name}" }
                            span { class: "node-category-count", {count(*props.counts.get(&tag.id).unwrap_or(&0))} }
                        }
                    } }
                }
            }
            if props.loading { p { role: "status", class: "mt-2 text-xs text-stone-600 dark:text-stone-400", {t(l, "common.loading")} } }
            if !props.error.is_empty() {
                p { role: "alert", class: "mt-2 text-sm text-red-700 dark:text-red-400",
                    {t(l, "nodes.category_load_error")} " {props.error}"
                }
            } else if !props.loading && props.tags.is_empty() {
                p { class: "mt-2 text-sm text-stone-600 dark:text-stone-400", {t(l, "nodes.categories_empty")} }
            }
        }
    }
}
