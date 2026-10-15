//! Shared native modal behavior: focus isolation, return and bounded scrolling.

use dioxus::prelude::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsCast;

/// Form dialogs use the browser's modal focus and background-inert semantics.
#[component]
pub fn ModalDialog(
    title: String,
    busy: bool,
    #[props(default)] wide: bool,
    on_close: EventHandler<()>,
    children: Element,
) -> Element {
    let dialog = use_hook(|| Rc::new(RefCell::new(None::<web_sys::HtmlDialogElement>)));
    let opener = use_hook(|| {
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.active_element())
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
    });
    let mounted_dialog = dialog.clone();
    let keyboard_dialog = dialog.clone();
    use_drop(move || {
        // WHY: removing an open dialog does not reliably restore focus. Close it
        // before returning focus, and skip controls removed by a successful delete.
        if let Some(dialog) = dialog.borrow().as_ref() {
            dialog.close();
        }
        if let Some(opener) = opener.filter(|element| element.is_connected()) {
            let _ = opener.focus();
        }
    });
    rsx! {
        dialog {
            class: if wide { "modal-dialog modal-dialog-wide rounded-xl bg-white p-6 text-stone-900 shadow-xl dark:bg-stone-900 dark:text-stone-100" } else { "modal-dialog rounded-xl bg-white p-6 text-stone-900 shadow-xl dark:bg-stone-900 dark:text-stone-100" },
            aria_label: "{title}",
            tabindex: "-1",
            aria_busy: busy,
            onmounted: move |event| {
                if let Some(element) = event.data().downcast::<web_sys::Element>() {
                    if let Ok(dialog) = element.clone().dyn_into::<web_sys::HtmlDialogElement>() {
                        let _ = dialog.show_modal();
                        *mounted_dialog.borrow_mut() = Some(dialog);
                    }
                }
            },
            onkeydown: move |event| {
                if event.key() != Key::Tab || event.modifiers().intersects(Modifiers::ALT | Modifiers::CONTROL | Modifiers::META) { return; }
                let Some(dialog) = keyboard_dialog.borrow().clone() else { return; };
                // WHY: native modal focus excludes page controls but Chromium
                // can still tab into browser chrome at the boundary.
                let controls = tabbable_controls(&dialog);
                let Some(first) = controls.first() else {
                    event.prevent_default();
                    let _ = dialog.focus();
                    return;
                };
                let Some(last) = controls.last() else { return; };
                let active = dialog.owner_document().and_then(|document| document.active_element());
                let target = if event.modifiers().contains(Modifiers::SHIFT) && active.as_ref().is_some_and(|active| first.is_same_node(Some(active))) {
                    Some(last)
                } else if !event.modifiers().contains(Modifiers::SHIFT) && active.as_ref().is_some_and(|active| last.is_same_node(Some(active))) {
                    Some(first)
                } else { None };
                if let Some(target) = target {
                    event.prevent_default();
                    let _ = target.focus();
                }
            },
            oncancel: move |event| {
                // WHY: keep Rust state synchronized; a pending command must not
                // finish into a different dialog opened while its request ran.
                event.prevent_default();
                if !busy { on_close.call(()); }
            },
            h3 { class: "text-lg font-semibold", "{title}" }
            {children}
        }
    }
}

// WHY: forms are not the only modal content. Match native sequential focus,
// excluding hidden controls, disabled fieldsets and collapsed details contents.
fn tabbable_controls(dialog: &web_sys::HtmlDialogElement) -> Vec<web_sys::HtmlElement> {
    let selector = "a[href], area[href], input:not([type=hidden]), select, textarea, button, summary, iframe, object, embed, audio[controls], video[controls], [tabindex], [contenteditable]:not([contenteditable=false])";
    let Ok(nodes) = dialog.query_selector_all(selector) else {
        return Vec::new();
    };
    let mut controls: Vec<web_sys::HtmlElement> = (0..nodes.length())
        .filter_map(|i| nodes.item(i))
        .filter_map(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
        .filter(|element| {
            let in_tab_order = element.tab_index() >= 0
                || (element.is_content_editable() && !element.has_attribute("tabindex"));
            if !in_tab_order
                || element.matches(":disabled").unwrap_or(true)
                || element
                    .closest("[hidden], [inert]")
                    .ok()
                    .flatten()
                    .is_some()
                || (element.offset_width() == 0 && element.offset_height() == 0)
            {
                return false;
            }
            let visibility = web_sys::window()
                .and_then(|window| window.get_computed_style(element).ok().flatten())
                .and_then(|style| style.get_property_value("visibility").ok())
                .unwrap_or_default();
            if visibility == "hidden" || visibility == "collapse" {
                return false;
            }
            if let Some(details) = element.closest("details:not([open])").ok().flatten() {
                let summary = details.query_selector("summary").ok().flatten();
                if !summary.is_some_and(|summary| summary.contains(Some(element))) {
                    return false;
                }
            }
            true
        })
        .collect();
    // Positive tabindex is uncommon here, but native focus puts it before zero.
    controls.sort_by_key(|element| {
        if element.tab_index() > 0 {
            element.tab_index()
        } else {
            i32::MAX
        }
    });
    controls
}
