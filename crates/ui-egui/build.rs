//! Embeds the craft-i18n interface catalogs (storytold/craft-i18n) from `CRAFT_I18N_DIR`, following
//! craft-i18n's `docs/integration.md`: `OUT_DIR/craft_i18n.rs` holds `CRAFT_I18N`, one
//! (locale, shared catalog, TeXCraft catalog) entry per locale that has a `texcraft.tsv`. Without the
//! variable the list is empty and the interface is English only; `CRAFT_I18N_REQUIRED=1` (releases)
//! makes that an error.
//!
//! Also embeds the interface faces from storytold/craft-fonts' opt-in Latin catalog
//! (`fonts/latin-manifest.txt`) when `CRAFT_FONTS_DIR` names a checkout: Inter for the interface and
//! Liberation Mono for source text, each checked against the manifest's SHA-256. Without the
//! checkout `UI_FACES` is empty and egui's bundled faces are used (docs/design.md, Type).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const APP_CATALOG: &str = "texcraft.tsv";

fn main() {
    ui_faces();
    icons();
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

/// (name used in the app, family, style) of the faces embedded from the Latin catalog.
const FACES: &[(&str, &str, &str)] = &[
    ("Inter", "Inter", "Regular"),
    ("Inter-SemiBold", "Inter", "SemiBold"),
    ("Inter-Italic", "Inter", "Italic"),
    ("Mono", "Liberation Mono", "Regular"),
];

fn ui_faces() {
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let required = std::env::var("CRAFT_FONTS_REQUIRED").is_ok_and(|v| v.trim() == "1");
    let rows = match std::env::var_os("CRAFT_FONTS_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => latin_faces(&PathBuf::from(dir)).unwrap_or_else(|e| {
            let level = if required { "error" } else { "warning" };
            println!("cargo::{level}=CRAFT_FONTS_DIR: {e}; egui's bundled faces are used");
            Vec::new()
        }),
        None => {
            if required {
                println!("cargo::error=CRAFT_FONTS_REQUIRED=1 but CRAFT_FONTS_DIR is unset");
            }
            Vec::new()
        }
    };
    let mut out = String::from("/// Interface faces from craft-fonts (build.rs): (name, bytes).\npub static UI_FACES: &[(&str, &[u8])] = &[\n");
    for (name, path) in rows {
        let _ = writeln!(out, "    ({name:?}, include_bytes!({path:?})),");
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("ui_faces.rs");
    if let Err(e) = std::fs::write(&target, out) {
        println!("cargo::error=writing {}: {e}", target.display());
    }
}

fn latin_faces(dir: &Path) -> Result<Vec<(&'static str, PathBuf)>, String> {
    use sha2::Digest as _;
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    let manifest_path = dir.join("fonts/latin-manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest_path.display());
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let entries = craft_fonts_build::parse_manifest(&text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let mut out = Vec::new();
    for (name, family, style) in FACES {
        let entry =
            entries.iter().find(|e| e.family == *family && e.style == *style).ok_or_else(|| format!("the Latin catalog has no {family} {style}"))?;
        let file = dir.join(&entry.file);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let digest: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !digest.eq_ignore_ascii_case(&entry.sha256) {
            return Err(format!("{} does not match the manifest's SHA-256", file.display()));
        }
        println!("cargo::rerun-if-changed={}", file.display());
        out.push((*name, file));
    }
    Ok(out)
}

/// `ICONS`: every SVG in `assets/icons/` (the Lucide subset listed in ATTRIBUTION.md), by name.
fn icons() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|list| list.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".svg")).map(str::to_owned)).collect())
        .unwrap_or_default();
    names.sort();
    let mut out = String::from("/// The embedded icon catalog (build.rs): (name, SVG).\npub static ICONS: &[(&str, &[u8])] = &[\n");
    for name in names {
        let path = dir.join(format!("{name}.svg"));
        let _ = writeln!(out, "    ({name:?}, include_bytes!({:?})),", path.display().to_string());
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("icons.rs");
    if let Err(e) = std::fs::write(&target, out) {
        println!("cargo::error=writing {}: {e}", target.display());
    }
}
