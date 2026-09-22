//! Native modal behavior for source forms: focus isolation and bounded scrolling.

use dioxus::prelude::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsCast;

/// Source dialogs use the browser's modal focus and background-inert semantics.
#[component]
pub fn SourceDialog(
    title: String,
    busy: bool,
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
            class: "source-dialog rounded-xl bg-white p-6 text-stone-900 shadow-xl dark:bg-stone-900 dark:text-stone-100",
            aria_label: "{title}",
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
                let Ok(controls) = dialog.query_selector_all("input:not(:disabled), select:not(:disabled), button:not(:disabled)") else { return; };
                let Some(first) = controls.item(0) else { return; };
                let Some(last) = controls.item(controls.length().saturating_sub(1)) else { return; };
                let active = dialog.owner_document().and_then(|document| document.active_element());
                let target = if event.modifiers().contains(Modifiers::SHIFT) && active.as_ref().is_some_and(|active| first.is_same_node(Some(active))) {
                    Some(last)
                } else if !event.modifiers().contains(Modifiers::SHIFT) && active.as_ref().is_some_and(|active| last.is_same_node(Some(active))) {
                    Some(first)
                } else { None };
                if let Some(target) = target.and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok()) {
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
