//! Embeds TeXCraft's own templates (`templates/builtin/<id>/…`, CC0-1.0) into the crate: a table
//! of `(id, path in the project, bytes)`, written to `$OUT_DIR/builtin.rs` and included by
//! `builtin.rs`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/builtin");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut table = String::from("&[\n");
    let mut files = Vec::new();
    walk(&root, &mut files);
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let Ok(rel) = path.strip_prefix(&root) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Some((id, inside)) = rel.split_once('/') else { continue };
        let abs = path.canonicalize().unwrap_or(path.clone());
        let _ = writeln!(table, "    ({id:?}, {inside:?}, include_bytes!({:?}).as_slice()),", abs.display().to_string());
    }
    table.push(']');
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("builtin.rs");
    if let Err(e) = std::fs::write(&out, table) {
        println!("cargo:warning=builtin templates not embedded: {e}");
    }
}
