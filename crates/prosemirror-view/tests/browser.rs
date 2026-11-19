#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::wasm_bindgen_test_configure;

wasm_bindgen_test_configure!(run_in_browser);

#[path = "browser/support.rs"]
mod support;
#[path = "browser/view.rs"]
mod view;
