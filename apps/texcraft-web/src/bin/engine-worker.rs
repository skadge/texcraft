//! The engine's Web Worker: retex's WebAssembly module over an in-memory file system
//! (`texcraft_compile::web_engine`). Trunk builds it beside the app (`index.html`).

#[cfg(target_arch = "wasm32")]
fn main() {
    texcraft_compile::web_engine::worker_main();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("engine-worker runs only in the browser, as the web build's Web Worker.");
}
