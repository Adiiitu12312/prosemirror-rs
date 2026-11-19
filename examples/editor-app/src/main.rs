#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    editor_app::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
