//! Building the project and reading the results.

use serde_json::{Value, json};

use super::{CommandSpec, always, no_params};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "build.compile",
            label: "Compile",
            menu: &["Build"],
            shortcut: Some("Cmd+Enter"),
            params: wait_schema,
            enabled: has_main,
            run: compile,
            changes: true,
        },
        CommandSpec {
            id: "build.stop",
            label: "Stop Compiling",
            menu: &["Build"],
            shortcut: None,
            params: no_params,
            enabled: can_stop,
            run: stop,
            changes: true,
        },
        CommandSpec {
            id: "build.auto",
            label: "Compile Automatically",
            menu: &["Build"],
            shortcut: None,
            params: on_schema,
            enabled: always,
            run: auto,
            changes: true,
        },
        CommandSpec {
            id: "build.options",
            label: "Build Options",
            menu: &[],
            shortcut: None,
            params: options_schema,
            enabled: always,
            run: options,
            changes: true,
        },
        CommandSpec {
            id: "build.clean",
            label: "Delete Build Files",
            menu: &["Build"],
            shortcut: None,
            params: no_params,
            enabled: can_clean,
            run: clean,
            changes: true,
        },
        CommandSpec {
            id: "build.save_pdf",
            label: "Save PDF As…",
            menu: &["File"],
            shortcut: None,
            params: path_schema,
            enabled: has_pdf,
            run: save_pdf,
            changes: false,
        },
        CommandSpec {
            id: "build.result",
            label: "Build Result",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: result,
            changes: false,
        },
    ]
}

fn wait_schema() -> Value {
    json!({"type": "object", "properties": {
        "wait": {"type": "boolean", "description": "Return only when the build has finished, with its result (default false)."}
    }, "additionalProperties": false})
}

fn on_schema() -> Value {
    json!({"type": "object", "properties": {
        "on": {"type": "boolean"},
        "delay_ms": {"type": "integer", "minimum": 200, "maximum": 10000, "description": "How long typing must pause before a build."}
    }, "required": ["on"], "additionalProperties": false})
}

fn options_schema() -> Value {
    json!({"type": "object", "properties": {
        "engine": {"enum": ["auto", "lualatex", "pdflatex"], "description": "The engine the build is like: chosen from the document (auto), LuaLaTeX or pdfLaTeX."},
        "halt_on_error": {"type": "boolean", "description": "Stop at the first error instead of reporting every error."},
        "draft_images": {"type": "boolean", "description": "Draw images as framed boxes with their names, for faster builds."}
    }, "additionalProperties": false})
}

/// Changes the options given and returns them all; with no parameters, only returns them.
fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = s.settings.build;
    if let Some(e) = p.get("engine") {
        o.engine = serde_json::from_value(e.clone()).map_err(|_| "`engine` must be auto, lualatex or pdflatex")?;
    }
    if let Some(v) = p.get("halt_on_error") {
        o.halt_on_error = v.as_bool().ok_or("`halt_on_error` must be a boolean")?;
    }
    if let Some(v) = p.get("draft_images") {
        o.draft_images = v.as_bool().ok_or("`draft_images` must be a boolean")?;
    }
    s.set_build_options(o);
    serde_json::to_value(o).map_err(|e| e.to_string())
}

fn can_clean(s: &Session) -> Result<()> {
    if !s.can_clean() {
        return Err("This typesetting engine keeps no build files.".to_owned());
    }
    if s.is_building() { Err("A build is running: stop it or wait for it first.".to_owned()) } else { Ok(()) }
}

fn clean(s: &mut Session, _: &Value) -> Result<Value> {
    can_clean(s)?;
    s.clean_build()?;
    Ok(json!({"deleted": true}))
}

fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string", "description": "Where to write the PDF."}}, "required": ["path"], "additionalProperties": false})
}

fn has_pdf(s: &Session) -> Result<()> {
    if cfg!(target_arch = "wasm32") {
        return Err("Saving to a file is not available in the browser yet.".to_owned());
    }
    if s.pdf.is_some() { Ok(()) } else { Err("There is no PDF yet: compile the project first.".to_owned()) }
}

fn save_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    has_pdf(s)?;
    let path = std::path::PathBuf::from(p.get("path").and_then(Value::as_str).ok_or("the parameter `path` (a string) is missing")?);
    let path = match &s.roots {
        Some(roots) => roots.check(&path, crate::file_access::Access::Write)?,
        None => path,
    };
    let pdf = s.pdf.as_ref().ok_or("There is no PDF yet.")?;
    std::fs::write(&path, pdf.as_slice()).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(json!({"path": path.display().to_string(), "bytes": pdf.len()}))
}

fn has_main(s: &Session) -> Result<()> {
    if s.project.main().is_some() { Ok(()) } else { Err("The project has no main document.".to_owned()) }
}

fn can_stop(s: &Session) -> Result<()> {
    if !s.is_building() {
        return Err("Nothing is being compiled.".to_owned());
    }
    if s.can_cancel() { Ok(()) } else { Err("This typesetting engine cannot stop a build; it finishes on its own.".to_owned()) }
}

fn compile(s: &mut Session, p: &Value) -> Result<Value> {
    let id = s.start_build()?;
    if p.get("wait").and_then(Value::as_bool).unwrap_or(false) {
        s.wait();
        return result(s, &json!({}));
    }
    Ok(json!({"job": id}))
}

fn stop(s: &mut Session, _: &Value) -> Result<Value> {
    if s.stop_build() { Ok(json!({"stopping": true})) } else { Err("No build is running, or the engine cannot stop it.".to_owned()) }
}

fn auto(s: &mut Session, p: &Value) -> Result<Value> {
    s.settings.auto_compile = p.get("on").and_then(Value::as_bool).ok_or("the parameter `on` (a boolean) is missing")?;
    if let Some(delay) = p.get("delay_ms") {
        let delay = delay.as_u64().filter(|d| (200..=10_000).contains(d)).ok_or("`delay_ms` must be between 200 and 10000")?;
        s.settings.auto_compile_delay_ms = delay;
    }
    Ok(json!({"on": s.settings.auto_compile, "delay_ms": s.settings.auto_compile_delay_ms}))
}

fn result(s: &mut Session, _: &Value) -> Result<Value> {
    // The running build's draft PDF, while its later passes run.
    let draft = s.draft_pdf().map(|d| json!({"job": d.job, "pass": d.pass, "pdfBytes": d.pdf.len()}));
    let Some(o) = s.last_outcome() else {
        return Ok(json!({"status": "none", "building": s.is_building(), "draft": draft}));
    };
    Ok(json!({
        "job": o.job,
        "result": o.status,
        "building": s.is_building(),
        "pages": o.pages,
        "seconds": o.seconds,
        "errors": o.errors(),
        "warnings": o.warnings(),
        "pdfBytes": o.pdf.as_ref().map(|p| p.len()),
        "diagnostics": o.diagnostics,
        "draft": draft,
    }))
}
