//! Proxy chain editor (NODE-017) — ordered list of relay nodes.
//!
//! `PUT /nodes/{id}/chain` replaces the entire chain. The editor fetches
//! all nodes for the picker, shows the current ordered chain with remove
//! buttons, and lets the admin append a node or clear the chain. The node
//! itself is excluded to prevent self-cycles.

#![cfg(target_family = "wasm")]

use dioxus::prelude::*;

use crate::api::{get, send};
use crate::i18n::{Language, t};
use crate::pages::node_types::{ListNodesResponse, NodeChainResponse, NodeDto, SetNodeChainRequest};

#[derive(Props, Clone)]
pub struct ChainModalProps {
    lang: Signal<Language>,
    node_id: String,
    /// Existing chain node IDs (preloaded from the node list).
    initial_chain: Vec<String>,
    on_close: EventHandler<()>,
    on_success: EventHandler<()>,
}

pub fn ChainModal(props: ChainModalProps) -> Element {
    let l = *props.lang.read();
    let mut all_nodes = use_signal(Vec::<NodeDto>::new);
    let mut chain = use_signal(|| props.initial_chain.clone());
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(String::new);

    use_future(move || async move {
        match get::<ListNodesResponse>("/nodes?limit=10000").await {
            Ok(resp) => {
                all_nodes.set(resp.nodes);
                loading.set(false);
            }
            Err(e) => {
                error.set(e.message);
                loading.set(false);
            }
        }
    });

    // Available nodes for the picker: not the node itself, not already in chain.
    let available: Vec<NodeDto> = all_nodes
        .read()
        .iter()
        .filter(|n| n.id != props.node_id && !chain.read().contains(&n.id))
        .cloned()
        .collect();

    let add_node = move |id: String| {
        chain.write().push(id);
    };

    let remove_at = move |idx: usize| {
        chain.write().remove(idx);
    };

    let clear = move |_| {
        chain.write().clear();
    };

    let submit = move |_| {
        saving.set(true);
        error.set(String::new());
        let req = SetNodeChainRequest {
            nodes: chain.read().clone(),
        };
        let nid = props.node_id.clone();
        spawn(async move {
            let path = format!("/nodes/{nid}/chain");
            match send::<NodeChainResponse, _>("PUT", &path, Some(&req)).await {
                Ok(_) => {
                    saving.set(false);
                    props.on_success.call(());
                    props.on_close.call(());
                }
                Err(e) => {
                    error.set(e.message);
                    saving.set(false);
                }
            }
        });
    };

    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            onclick: move |_| props.on_close.call(()),
            div {
                class: "w-full max-w-lg rounded-lg bg-white p-6 shadow-xl dark:bg-stone-900",
                onclick: move |e| e.stop_propagation(),
                h3 { class: "text-lg font-semibold text-stone-900 dark:text-stone-100",
                    {t(l, "nodes.chain_title")}
                }
                p { class: "mt-2 text-sm text-stone-500 dark:text-stone-400",
                    {t(l, "nodes.chain_hint")}
                }

                if *loading.read() {
                    div { class: "mt-4 flex justify-center py-8",
                        div { class: "h-5 w-5 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600" }
                    }
                } else {
                    // Current chain (ordered).
                    div { class: "mt-4",
                        label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300",
                            {t(l, "nodes.chain_current")}
                        }
                        if chain.read().is_empty() {
                            p { class: "mt-1 text-sm text-stone-400 dark:text-stone-500", {t(l, "nodes.chain_empty")} }
                        } else {
                            ol { class: "mt-2 space-y-1",
                                for (i, cid) in chain.read().iter().enumerate() {
                                    {
                                        let name = all_nodes.read().iter()
                                            .find(|n| &n.id == cid)
                                            .map(|n| n.display_name.clone())
                                            .unwrap_or_else(|| cid.clone());
                                        let rm_id = cid.clone();
                                        rsx! {
                                            li {
                                                class: "flex items-center justify-between rounded-md border border-stone-200 px-3 py-1.5 text-sm dark:border-stone-700",
                                                span { class: "text-stone-800 dark:text-stone-200",
                                                    span { class: "text-stone-400 mr-2", "{i + 1}." }
                                                    "{name}"
                                                }
                                                button {
                                                    class: "text-red-500 hover:text-red-700 text-xs",
                                                    onclick: move |_| remove_at(i),
                                                    {t(l, "nodes.chain_remove")}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Picker.
                    if !available.is_empty() {
                        div { class: "mt-4",
                            label { class: "block text-sm font-medium text-stone-700 dark:text-stone-300",
                                {t(l, "nodes.chain_add")}
                            }
                            select {
                                class: "mt-1 block w-full rounded-md border border-stone-300 px-3 py-2 text-sm dark:border-stone-700 dark:bg-stone-800",
                                value: "",
                                onchange: move |e| {
                                    let v = e.value();
                                    if !v.is_empty() {
                                        add_node(v);
                                    }
                                },
                                option { value: "", {t(l, "nodes.chain_pick")} }
                                for n in &available {
                                    option { value: "{n.id}", "{n.display_name}" }
                                }
                            }
                        }
                    }

                    if !chain.read().is_empty() {
                        button {
                            class: "mt-3 text-sm text-red-500 hover:text-red-700",
                            onclick: clear,
                            {t(l, "nodes.chain_clear")}
                        }
                    }

                    if !error.read().is_empty() {
                        div { class: "mt-3 rounded-md bg-red-50 p-3 text-sm text-red-600 dark:bg-red-900/20 dark:text-red-400", "{error}" }
                    }

                    div { class: "mt-6 flex justify-end gap-2",
                        button {
                            class: "rounded-md border border-stone-300 px-4 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800",
                            onclick: move |_| props.on_close.call(()),
                            {t(l, "common.cancel")}
                        }
                        button {
                            class: "rounded-md bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50",
                            disabled: *saving.read(),
                            onclick: submit,
                            if *saving.read() { {t(l, "common.loading")} } else { {t(l, "common.save")} }
                        }
                    }
                }
            }
        }
    }
}
