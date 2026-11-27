//! Repository checks: `cargo xtask layers | wasm | ci`.
//!
//! - `layers`: each workspace crate may depend only on crates of a lower layer, and only the
//!   `ui-egui` layer and the apps may use a UI toolkit.
//! - `wasm`: the web app and the crates it uses build for wasm32-unknown-unknown.
//! - `ci`: fmt, clippy, tests, layers, the i18n lint, wasm, guide-check, in that order.
//! - `guide-check`: the user guide's pages, links and anchors (`docs/guide/`; see `guide.rs`).
//! - `i18n-report [OUT]`: the English interface strings and what each catalog in `i18n/` lacks
//!   (`i18n.rs`); with `OUT`, the strings are written there for translators.
//! - `i18n-lint`: no English literal reaches a widget without `tl!` (part of `ci`).
//! - `icons`: every app icon file, from the SVG master in `assets/app-icon/`.
//! - `retex-wasm`: retex's engine for the browser, from the release `Cargo.lock` pins (needs
//!   nightly and wasi-sdk; see `retex_wasm.rs`).
//! - `templates [ID]`: fetches every template of the catalogue (or one) from its source, with its
//!   checks, into a scratch store, and builds each with `texcraft-cli compile`. Needs the network.
//! - `templates-site OUT`: the templates for the browser build, in `OUT/templates/`.
//! - `tree-site OUT [--bundle[=ARCHIVE] [--no-installation]]`: the TeX tree of the browser build
//!   and its seed, in `OUT/tree/` and `OUT/seed/`, from a TeX installation, or from retex's bundle
//!   with the installation's additions or alone (see `tree_site.rs`).
//! - `version-worker [DIR]`: names the browser build's engine files after their contents (Trunk's
//!   post-build hook, on its staging folder; see `web_version.rs`).

mod guide;
mod i18n;
mod icons;
mod retex_wasm;
mod tree_site;
mod web_version;

use std::process::{Command, ExitCode};

use serde_json::Value;

/// (crate, layer). A crate may depend on crates with a lower layer only. `None`: exempt (apps).
const LAYERS: &[(&str, Option<u8>)] = &[
    ("texcraft-project", Some(0)),
    ("texcraft-latex", Some(0)),
    ("texcraft-buffer", Some(0)),
    ("texcraft-review", Some(0)),
    ("texcraft-history", Some(0)),
    ("texcraft-gitwire", Some(0)),
    ("texcraft-wasi", Some(0)),
    ("texcraft-vim", Some(1)),
    ("texcraft-forge", Some(1)),
    ("texcraft-templates", Some(0)),
    ("texcraft-compile", Some(1)),
    ("texcraft-preview", Some(1)),
    ("texcraft-engine", Some(2)),
    ("texcraft-ui-egui", Some(3)),
    ("texcraft-mcp", Some(3)),
    ("texcraft", None),
    ("texcraft-cli", None),
    ("texcraft-web", None),
    ("xtask", None),
];

const UI_CRATES: &[&str] = &["egui", "eframe", "winit", "egui_kittest", "craft-ui", "craft-keymap", "rfd"];
const UI_MIN_LAYER: u8 = 3;

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "layers" => layers(),
        "wasm" => wasm(),
        "ci" => ci(),
        "guide-check" => guide::check(),
        "icons" => icons::run(),
        "i18n-report" => i18n::report(&root(), std::env::args().nth(2).as_deref().map(std::path::Path::new)),
        "i18n-lint" => i18n::lint(&root()),
        "retex-wasm" => std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| "xtask is not in a workspace".to_owned())
            .and_then(retex_wasm::run),
        "templates" => templates(std::env::args().nth(2).as_deref()),
        "tree-site" => match std::env::args().nth(2) {
            Some(out) => {
                let rest: Vec<String> = std::env::args().skip(3).collect();
                tree_site::Source::from_args(&root(), &rest).and_then(|source| tree_site::run(&root(), std::path::Path::new(&out), &source))
            }
            None => Err(
                "usage: cargo xtask tree-site OUT [--bundle[=ARCHIVE] [--no-installation]] (the web build's folder, its templates written first)".to_owned(),
            ),
        },
        "templates-site" => match std::env::args().nth(2) {
            Some(out) => templates_site(std::path::Path::new(&out)),
            None => Err("usage: cargo xtask templates-site OUT (the web build's folder)".to_owned()),
        },
        "version-worker" => match std::env::args().nth(2).or_else(|| std::env::var("TRUNK_STAGING_DIR").ok()) {
            Some(dir) => web_version::run(std::path::Path::new(&dir)).map(|h| eprintln!("the engine's files are named after {h}")),
            None => Err("usage: cargo xtask version-worker DIR (a Trunk build's folder; Trunk's hooks set TRUNK_STAGING_DIR)".to_owned()),
        },
        _ => Err(
            "usage: cargo xtask layers | wasm | ci | guide-check | icons | i18n-report [OUT] | i18n-lint | retex-wasm | templates [ID] | templates-site OUT | tree-site OUT [--bundle[=ARCHIVE] [--no-installation]] | version-worker [DIR]"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace's root.
fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    eprintln!("$ {program} {}", args.join(" "));
    let status = Command::new(program).args(args).status().map_err(|e| format!("{program}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("`{program} {}` failed", args.join(" "))) }
}

fn layer_of(name: &str) -> Option<Option<u8>> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

fn layers() -> Result<(), String> {
    let out =
        Command::new(env!("CARGO")).args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
    let mut problems = Vec::new();
    for package in meta["packages"].as_array().into_iter().flatten() {
        let name = package["name"].as_str().unwrap_or_default();
        let Some(layer) = layer_of(name) else {
            problems.push(format!("{name} has no layer in xtask's LAYERS table"));
            continue;
        };
        let Some(layer) = layer else { continue };
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            if dep["kind"].as_str() == Some("dev") {
                continue;
            }
            let dep_name = dep["name"].as_str().unwrap_or_default();
            if let Some(Some(dep_layer)) = layer_of(dep_name)
                && dep_layer >= layer
            {
                problems.push(format!("{name} (layer {layer}) depends on {dep_name} (layer {dep_layer})"));
            }
            if layer < UI_MIN_LAYER && UI_CRATES.contains(&dep_name) {
                problems.push(format!("{name} (layer {layer}) depends on the UI crate {dep_name}"));
            }
        }
    }
    if problems.is_empty() {
        eprintln!("layers: ok");
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn wasm() -> Result<(), String> {
    run(env!("CARGO"), &["check", "--target", "wasm32-unknown-unknown", "-p", "texcraft-web"])
}

fn ci() -> Result<(), String> {
    let cargo = env!("CARGO");
    run(cargo, &["fmt", "--all", "--check"])?;
    run(cargo, &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    run(cargo, &["test", "--workspace"])?;
    layers()?;
    i18n::lint(&root())?;
    wasm()?;
    guide::check()
}

fn templates(only: Option<&str>) -> Result<(), String> {
    let catalog = texcraft_templates::Catalog::builtin().map_err(|e| e.to_string())?;
    let scratch = std::env::temp_dir().join(format!("texcraft-templates-{}", std::process::id()));
    let store = texcraft_templates::Store::new(scratch.join("store"));
    run(env!("CARGO"), &["build", "--release", "-q", "-p", "texcraft-cli"])?;
    let cli = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/release/texcraft-cli");
    let mut failed = Vec::new();
    for t in catalog.templates().iter().filter(|t| only.is_none_or(|id| t.id == id)) {
        let dir = match store.fetch(t) {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("FETCH FAIL {}: {e}", t.id);
                failed.push(t.id.clone());
                continue;
            }
        };
        let out = scratch.join(format!("{}.pdf", t.id));
        let status = Command::new(&cli)
            .args(["compile", &dir.display().to_string(), "--output", &out.display().to_string()])
            .output()
            .map_err(|e| format!("texcraft-cli: {e}"))?;
        let log = String::from_utf8_lossy(&status.stderr);
        let errors = log.lines().filter(|l| l.contains("error:")).count();
        if status.status.success() && errors == 0 {
            eprintln!("ok         {}  {}", t.id, log.lines().last().unwrap_or_default());
        } else {
            eprintln!(
                "BUILD FAIL {} ({errors} errors)\n{}",
                t.id,
                log.lines().filter(|l| l.contains("error")).take(5).collect::<Vec<_>>().join("\n")
            );
            failed.push(t.id.clone());
        }
    }
    eprintln!("store: {}", scratch.display());
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

/// The templates for the browser build, which cannot download from TeX Live's mirrors (no CORS):
/// each template of the catalogue, fetched and checked as the desktop app does, is written to
/// `<out>/templates/<id>/` at its project paths with its licence note (`TEMPLATE-LICENSE`), and
/// `files.txt` lists them. The web app fetches them from its own site.
fn templates_site(out: &std::path::Path) -> Result<(), String> {
    let catalog = texcraft_templates::Catalog::builtin().map_err(|e| e.to_string())?;
    let scratch = std::env::temp_dir().join(format!("texcraft-templates-site-{}", std::process::id()));
    let store = texcraft_templates::Store::new(&scratch);
    let site = out.join("templates");
    if site.exists() {
        std::fs::remove_dir_all(&site).map_err(|e| format!("{}: {e}", site.display()))?;
    }
    let mut failed = Vec::new();
    for t in catalog.templates() {
        // TeX Live's mirrors are sometimes mid-update: a failed download is tried twice more.
        let mut fetched = store.fetch(t);
        for _ in 0..2 {
            match &fetched {
                Err(e) => {
                    eprintln!("retrying {} ({e})", t.id);
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    fetched = store.fetch(t);
                }
                Ok(_) => break,
            }
        }
        let kept = match fetched {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("FETCH FAIL {}: {e}", t.id);
                failed.push(t.id.clone());
                continue;
            }
        };
        let mut paths: Vec<String> = t.project_paths().into_iter().map(|(_, p)| p).collect();
        paths.push("TEMPLATE-LICENSE".to_owned());
        let dir = site.join(&t.id);
        for rel in &paths {
            let to = dir.join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::copy(kept.join(rel), &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        std::fs::write(dir.join("files.txt"), paths.join("\n") + "\n").map_err(|e| format!("{}: {e}", dir.display()))?;
        eprintln!("ok {} ({} files)", t.id, paths.len());
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if failed.is_empty() { Ok(()) } else { Err(format!("templates not fetched: {}", failed.join(", "))) }
}

/// FNV-1a, 64 bits.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}
