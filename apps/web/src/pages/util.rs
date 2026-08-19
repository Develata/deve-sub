//! Shared utility functions for web frontend pages.

#![cfg(target_family = "wasm")]

use wasm_bindgen_futures::JsFuture;

/// Copy text to the system clipboard via the async Clipboard API.
pub async fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or("no window")?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();
    let promise = clipboard.write_text(text);
    JsFuture::from(promise)
        .await
        .map(|_| ())
        .map_err(|e| format!("clipboard error: {e:?}"))
}
