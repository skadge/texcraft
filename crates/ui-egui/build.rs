//! Embeds the craft-i18n interface catalogs (storytold/craft-i18n) from `CRAFT_I18N_DIR`, following
//! craft-i18n's `docs/integration.md`: `OUT_DIR/craft_i18n.rs` holds `CRAFT_I18N`, one
//! (locale, shared catalog, TeXCraft catalog) entry per locale that has a `texcraft.tsv`. Without the
//! variable the list is empty and the interface is English only; `CRAFT_I18N_REQUIRED=1` (releases)
//! makes that an error.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const APP_CATALOG: &str = "texcraft.tsv";

fn main() {
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_REQUIRED");
    let required = std::env::var("CRAFT_I18N_REQUIRED").is_ok_and(|v| v.trim() == "1");
    let entries = match std::env::var_os("CRAFT_I18N_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => locales(&PathBuf::from(dir).join("locales")).unwrap_or_else(|e| {
            let level = if required { "error" } else { "warning" };
            println!("cargo::{level}=CRAFT_I18N_DIR: {e}; the interface is English only");
            Vec::new()
        }),
        None => {
            if required {
                println!("cargo::error=CRAFT_I18N_REQUIRED=1 but CRAFT_I18N_DIR is unset");
            }
            Vec::new()
        }
    };
    let mut out = String::from("/// The embedded craft-i18n catalogs (build.rs).\npub static CRAFT_I18N: &craft_i18n::Embedded = &[\n");
    for (code, shared, app) in entries {
        let shared = shared.map_or_else(|| "\"\"".to_owned(), |p| format!("include_str!({p:?})"));
        let _ = writeln!(out, "    ({code:?}, {shared}, include_str!({app:?})),");
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_i18n.rs");
    if let Err(e) = std::fs::write(&target, out) {
        println!("cargo::error=writing {}: {e}", target.display());
    }
}

type Locale = (String, Option<PathBuf>, PathBuf);

fn locales(dir: &Path) -> Result<Vec<Locale>, String> {
    // Not canonicalised (Windows `\\?\` paths break include_str!), but must be absolute, since
    // include_str! resolves relative paths against the generated file.
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    println!("cargo::rerun-if-changed={}", dir.display());
    let list = std::fs::read_dir(dir).map_err(|e| format!("{} is not a craft-i18n checkout ({e})", dir.display()))?;
    let mut out = Vec::new();
    for entry in list.flatten() {
        let path = entry.path();
        let Some(code) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        let app = path.join(APP_CATALOG);
        if !app.is_file() {
            continue;
        }
        println!("cargo::rerun-if-changed={}", app.display());
        let shared = Some(path.join("common.tsv")).filter(|p| p.is_file());
        out.push((code, shared, app));
    }
    out.sort();
    Ok(out)
}
