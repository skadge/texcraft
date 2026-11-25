//! `cargo xtask version-worker [DIR]`: names the engine's files of the browser build after their
//! contents, so that a browser never runs an old engine after an update (docs/ROADMAP.md, W10).
//!
//! Trunk names the app's own script and module after a hash of their contents, but not the
//! engine's Web Worker (`engine-worker.js`, `engine-worker_bg.wasm`, the loader that starts them,
//! `engine-worker_loader.js`) nor retex's module (`retex.wasm`, copied as is): a browser could
//! keep any of them from its HTTP cache after the site is updated, and run an old worker with a
//! new editor. This step, Trunk's post-build hook (`Trunk.toml`), renames the four with a hash
//! `H` of the three it starts (`engine-worker-H.js`, `engine-worker-H_bg.wasm`,
//! `engine-worker-H_loader.js`, `retex-H.wasm`), points the loader at the renamed files, and
//! writes the loader's name in the page (`<meta name="texcraft-engine">`, which the app reads).
//! The worker finds `H` in its own loader's name and fetches `retex-H.wasm`
//! (`texcraft_compile::web_engine`). The page itself, `index.html`, is the one file not renamed,
//! as for every Trunk build. A build without this step keeps the plain names, which the app and
//! the worker also take.
//!
//! The worker's Cache Storage stays consistent: its caches are named after a hash of retex's
//! module and of the tree's index, whatever the files are called.

use std::path::Path;

/// The loader's name in `index.html` before this step, which it replaces.
const PAGE_LOADER: &str = "content=\"engine-worker_loader.js\"";

/// A 64-bit FNV-1a hash of `parts`, each preceded by its length.
fn hash(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in (part.len() as u64).to_le_bytes().iter().chain(part.iter()) {
            h = (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

fn read(dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    std::fs::read(dir.join(name)).map_err(|e| format!("{}: {e}", dir.join(name).display()))
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(dir.join(name), bytes).map_err(|e| format!("{}: {e}", dir.join(name).display()))
}

/// Renames the engine's files in `dir` (a Trunk build's folder) after their contents; returns
/// the hash.
pub fn run(dir: &Path) -> Result<String, String> {
    let script = read(dir, "engine-worker.js")?;
    let module = read(dir, "engine-worker_bg.wasm")?;
    let retex = read(dir, "retex.wasm")?;
    let loader = String::from_utf8(read(dir, "engine-worker_loader.js")?).map_err(|_| "engine-worker_loader.js is not UTF-8")?;
    let page = String::from_utf8(read(dir, "index.html")?).map_err(|_| "index.html is not UTF-8")?;
    let h = format!("{:016x}", hash(&[&script, &module, &retex]));
    // The loader Trunk writes: `importScripts("./engine-worker.js");wasm_bindgen("./engine-worker_bg.wasm");`.
    let mut renamed = loader.clone();
    for (from, to) in [("./engine-worker.js", format!("./engine-worker-{h}.js")), ("./engine-worker_bg.wasm", format!("./engine-worker-{h}_bg.wasm"))]
    {
        if !renamed.contains(from) {
            return Err(format!("engine-worker_loader.js does not name {from}: Trunk's loader changed ({loader:?})"));
        }
        renamed = renamed.replace(from, &to);
    }
    if !page.contains(PAGE_LOADER) {
        return Err(format!("index.html has no <meta name=\"texcraft-engine\" {PAGE_LOADER}>"));
    }
    let page = page.replace(PAGE_LOADER, &format!("content=\"engine-worker-{h}_loader.js\""));
    write(dir, &format!("engine-worker-{h}.js"), &script)?;
    write(dir, &format!("engine-worker-{h}_bg.wasm"), &module)?;
    write(dir, &format!("retex-{h}.wasm"), &retex)?;
    write(dir, &format!("engine-worker-{h}_loader.js"), renamed.as_bytes())?;
    write(dir, "index.html", page.as_bytes())?;
    for old in ["engine-worker.js", "engine-worker_bg.wasm", "retex.wasm", "engine-worker_loader.js"] {
        std::fs::remove_file(dir.join(old)).map_err(|e| format!("{old}: {e}"))?;
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(dir: &Path, retex: &[u8]) {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("engine-worker.js"), "var wasm_bindgen;").unwrap();
        std::fs::write(dir.join("engine-worker_bg.wasm"), b"\0asm").unwrap();
        std::fs::write(dir.join("retex.wasm"), retex).unwrap();
        std::fs::write(dir.join("engine-worker_loader.js"), r#"importScripts("./engine-worker.js");wasm_bindgen("./engine-worker_bg.wasm");"#)
            .unwrap();
        std::fs::write(dir.join("index.html"), r#"<meta name="texcraft-engine" content="engine-worker_loader.js">"#).unwrap();
    }

    #[test]
    fn the_page_names_a_loader_that_starts_the_renamed_files_and_a_new_engine_gets_new_names() {
        let dir = std::env::temp_dir().join(format!("xtask-version-worker-{}", std::process::id()));
        build(&dir, b"retex 1");
        let first = run(&dir).unwrap();
        let page = std::fs::read_to_string(dir.join("index.html")).unwrap();
        let loader = page.split("content=\"").nth(1).and_then(|r| r.split('"').next()).unwrap().to_owned();
        let shim = std::fs::read_to_string(dir.join(&loader)).unwrap();
        for name in shim.split('"').filter(|s| s.starts_with("./")) {
            assert!(dir.join(name).exists(), "the loader names {name}, which is not there");
        }
        assert!(loader.contains(&first) && dir.join(format!("retex-{first}.wasm")).exists());
        assert!(!dir.join("retex.wasm").exists() && !dir.join("engine-worker_loader.js").exists(), "the plain names are left");
        // Another retex: every name changes.
        build(&dir, b"retex 2");
        let second = run(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_ne!(first, second);
    }
}
