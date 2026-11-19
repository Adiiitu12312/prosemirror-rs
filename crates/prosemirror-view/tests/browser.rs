#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::wasm_bindgen_test_configure;

wasm_bindgen_test_configure!(run_in_browser);

#[path = "browser/composition.rs"]
mod composition;
#[path = "browser/domchange.rs"]
mod domchange;
#[path = "browser/draw.rs"]
mod draw;
#[path = "browser/markview.rs"]
mod markview;
#[path = "browser/nodeview.rs"]
mod nodeview;
#[path = "browser/support.rs"]
mod support;
#[path = "browser/end_of_textblock.rs"]
mod end_of_textblock;
#[path = "browser/selection.rs"]
mod selection;
#[path = "browser/view.rs"]
mod view;
