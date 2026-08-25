//! Shared utility functions for web frontend pages.

#![cfg(target_family = "wasm")]

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

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

pub async fn sleep_ms(ms: u32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let ms = ms.min(i32::MAX as u32) as i32;
    let mut cb = move |resolve: js_sys::Function, _reject: js_sys::Function| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            resolve.unchecked_ref(),
            ms,
        );
    };
    let promise = js_sys::Promise::new(&mut cb);
    let _ = JsFuture::from(promise).await;
}
