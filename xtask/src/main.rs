//! Repository checks: `cargo xtask layers | wasm | ci`.
//!
//! - `layers`: each workspace crate may depend only on crates of a lower layer, and only the
//!   `ui-egui` layer and the apps may use a UI toolkit.
//! - `wasm`: the web app and the crates it uses build for wasm32-unknown-unknown.
//! - `ci`: fmt, clippy, tests, layers, wasm, in that order.
//! - `icons`: every app icon file, from the SVG master in `assets/app-icon/`.
//! - `templates [ID]`: fetches every template of the catalogue (or one) from its source, with its
//!   checks, into a scratch store, and builds each with `texcraft-cli compile`. Needs the network.

mod icons;

use std::process::{Command, ExitCode};

use serde_json::Value;

/// (crate, layer). A crate may depend on crates with a lower layer only. `None`: exempt (apps).
const LAYERS: &[(&str, Option<u8>)] = &[
    ("texcraft-project", Some(0)),
    ("texcraft-latex", Some(0)),
    ("texcraft-buffer", Some(0)),
    ("texcraft-review", Some(0)),
    ("texcraft-wasi", Some(0)),
    ("texcraft-vim", Some(1)),
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
        "icons" => icons::run(),
        "templates" => templates(std::env::args().nth(2).as_deref()),
        _ => Err("usage: cargo xtask layers | wasm | ci | icons | templates [ID]".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
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
    wasm()
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
