use dioxus::prelude::*;

use crate::i18n::{t, Language};
use crate::mock::generate_group_items;

#[component]
pub fn GroupsPage(lang: Signal<Language>) -> Element {
    let mut items = use_signal(|| generate_group_items(500));
    let mut drag_index = use_signal(|| None::<usize>);
    let mut drag_over = use_signal(|| None::<usize>);

    rsx! {
        div { class: "space-y-4",
            div {
                h2 { class: "text-lg font-semibold", {t(*lang.read(), "groups.title")} }
                p { class: "mt-1 text-sm text-stone-400", {t(*lang.read(), "groups.hint")} }
            }

            div { class: "rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900",
                div { class: "max-h-[600px] overflow-y-auto",
                    for (i, name) in items.read().iter().enumerate() {
                        {
                            let is_dragging = drag_index() == Some(i);
                            let is_over = drag_over() == Some(i) && drag_index() != Some(i);
                            let bg_class = if is_dragging {
                                "bg-amber-50 opacity-50 dark:bg-amber-900/20"
                            } else if is_over {
                                "bg-amber-100 dark:bg-amber-900/30"
                            } else if i % 2 == 0 {
                                "bg-white dark:bg-stone-900"
                            } else {
                                "bg-stone-50 dark:bg-stone-800/50"
                            };
                            let idx = i;
                            rsx! {
                                div {
                                    key: "{idx}",
                                    class: "flex items-center gap-3 border-b border-stone-100 px-4 py-3 transition-colors dark:border-stone-800 {bg_class}",
                                    draggable: "true",
                                    ondragstart: move |_| drag_index.set(Some(idx)),
                                    ondragover: move |evt| {
                                        evt.prevent_default();
                                        drag_over.set(Some(idx));
                                    },
                                    ondrop: move |evt| {
                                        evt.prevent_default();
                                        if let Some(from) = drag_index.take() {
                                            let mut list = items.write();
                                            if from < idx {
                                                let item = list.remove(from);
                                                list.insert(idx, item);
                                            } else if from > idx {
                                                let item = list.remove(from);
                                                list.insert(idx, item);
                                            }
                                        }
                                        drag_over.set(None);
                                    },
                                    ondragend: move |_| {
                                        drag_index.set(None);
                                        drag_over.set(None);
                                    },

                                    span { class: "text-stone-300 dark:text-stone-600 select-none", "⋮⋮" }
                                    span { class: "w-8 text-right text-sm font-mono text-stone-400", "{idx + 1}" }
                                    span { class: "flex-1 text-sm font-medium text-stone-700 dark:text-stone-200", "{name}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
