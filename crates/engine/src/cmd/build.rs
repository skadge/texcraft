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
            id: "build.suggestions",
            label: "Show Suggestions in the PDF",
            menu: &[],
            shortcut: None,
            params: suggestions_schema,
            enabled: always,
            run: suggestions,
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
            id: "build.package_server",
            label: "Package Server",
            menu: &[],
            shortcut: None,
            params: package_server_schema,
            enabled: always,
            run: package_server,
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
            id: "build.hide_messages",
            label: "Hide Messages Like This",
            menu: &[],
            shortcut: None,
            params: hide_schema,
            enabled: always,
            run: hide_messages,
            changes: true,
        },
        CommandSpec {
            id: "build.show_messages",
            label: "Show Messages Again",
            menu: &[],
            shortcut: None,
            params: show_schema,
            enabled: always,
            run: show_messages,
            changes: true,
        },
        CommandSpec {
            id: "build.hidden_messages",
            label: "Hidden Messages",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: |s, _| Ok(s.hidden_info()),
            changes: false,
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
            id: "build.export",
            label: "Export…",
            menu: &[],
            shortcut: None,
            params: export_schema,
            enabled: can_export,
            run: export,
            changes: false,
        },
        CommandSpec {
            id: "build.outputs",
            label: "Build Outputs",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: outputs,
            changes: false,
        },
        CommandSpec {
            id: "build.save_output",
            label: "Save Build Output As…",
            menu: &[],
            shortcut: None,
            params: save_output_schema,
            enabled: has_outputs,
            run: save_output,
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

fn suggestions_schema() -> Value {
    json!({"type": "object", "properties": {
        "on": {"type": "boolean", "description": "Build the sources with their open suggestions applied (the files are not changed); off, builds ignore suggestions."}
    }, "required": ["on"], "additionalProperties": false})
}

fn suggestions(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).ok_or("the parameter `on` (a boolean) is missing")?;
    s.set_with_suggestions(on);
    Ok(json!({"on": on}))
}

fn options_schema() -> Value {
    json!({"type": "object", "properties": {
        "engine": {"enum": ["auto", "lualatex", "pdflatex"], "description": "The engine the build is like: chosen from the document (auto), LuaLaTeX or pdfLaTeX."},
        "halt_on_error": {"type": "boolean", "description": "Stop at the first error instead of reporting every error."},
        "draft_images": {"type": "boolean", "description": "Draw images as framed boxes with their names, for faster builds."},
        "output": {"enum": ["screen", "accessible", "print"], "description": "The kind of PDF: for the screen; accessible (tagged, PDF/UA, checked for screen readers); or for print (a PDF/X beside the PDF, with preflight checks)."},
        "pdfx": {"enum": ["x4", "x1a"], "description": "For print: PDF/X-4 or PDF/X-1a."},
        "bleed_mm": {"type": "number", "minimum": 0, "maximum": texcraft_compile::MAX_BLEED_MM, "description": "For print: the bleed, in millimetres."},
        "crop_marks": {"type": "boolean", "description": "For print: crop marks outside the bleed."}
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
    if let Some(v) = p.get("output") {
        o.output = serde_json::from_value(v.clone()).map_err(|_| "`output` must be screen, accessible or print")?;
    }
    if let Some(v) = p.get("pdfx") {
        o.pdfx = serde_json::from_value(v.clone()).map_err(|_| "`pdfx` must be x4 or x1a")?;
    }
    if let Some(v) = p.get("bleed_mm") {
        let mm = v.as_f64().filter(|mm| (0.0..=f64::from(texcraft_compile::MAX_BLEED_MM)).contains(mm));
        o.bleed_mm = mm.ok_or(format!("`bleed_mm` must be a number from 0 to {}", texcraft_compile::MAX_BLEED_MM))? as f32;
    }
    if let Some(v) = p.get("crop_marks") {
        o.crop_marks = v.as_bool().ok_or("`crop_marks` must be a boolean")?;
    }
    s.set_build_options(o);
    serde_json::to_value(o).map_err(|e| e.to_string())
}

fn package_server_schema() -> Value {
    json!({"type": "object", "properties": {
        "server": {"type": "string", "description": format!("Where the browser build downloads the TeX Live packages its own TeX files lack: `default` ({}), `off`, or the https:// address of another mirror with the same layout (by TeX Live year, then snapshot). The desktop build uses retex's own settings.", texcraft_compile::packages::DEFAULT_SERVER)}
    }, "additionalProperties": false})
}

/// Sets the package server when given, and returns it.
fn package_server(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = p.get("server") {
        let text = v.as_str().ok_or("`server` must be a string: default, off or an https:// address")?;
        let server = texcraft_compile::packages::PackageServer::parse(text)?;
        s.set_package_server(server);
    }
    let server = &s.settings.package_server;
    Ok(json!({"server": String::from(server.clone()), "url": server.base()}))
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

fn hide_schema() -> Value {
    json!({"type": "object", "properties": {
        "kind": {"type": "string", "description": "The kind of message: as `build.hidden_messages` lists them, its origin then its wording with what varies replaced by …"},
        "index": {"type": "integer", "minimum": 0, "description": "Instead of `kind`: the kind of this message of `build.result`'s diagnostics, from 0."},
        "scope": {"enum": ["project", "user"], "description": "This project (its .texcraft/hidden-messages.txt; for the session when it has no folder), or every project (the user's preferences). Default: project."}
    }, "additionalProperties": false})
}

fn show_schema() -> Value {
    json!({"type": "object", "properties": {
        "kind": {"type": "string"},
        "scope": {"enum": ["project", "user"], "description": "Where to show it again; both without."}
    }, "required": ["kind"], "additionalProperties": false})
}

fn scope(p: &Value) -> Result<Option<crate::hidden::Scope>> {
    match p.get("scope").and_then(Value::as_str) {
        None => Ok(None),
        Some("project") => Ok(Some(crate::hidden::Scope::Project)),
        Some("user") => Ok(Some(crate::hidden::Scope::User)),
        Some(other) => Err(format!("unknown scope `{other}`: project or user")),
    }
}

/// Hides the messages of a kind: named, or that of one of the last build's messages. Errors stay.
fn hide_messages(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = match (p.get("kind").and_then(Value::as_str), p.get("index").and_then(Value::as_u64)) {
        (Some(kind), _) => kind.to_owned(),
        (None, Some(i)) => {
            let d = s.last_outcome().and_then(|o| o.diagnostics.get(i as usize)).ok_or_else(|| format!("The last build has no message {i}."))?;
            if d.severity == texcraft_compile::Severity::Error {
                return Err("Errors are not hidden: fix them instead.".to_owned());
            }
            crate::hidden::kind(d)
        }
        (None, None) => return Err("Name the kind of message, or the `index` of one of the last build's.".to_owned()),
    };
    let kept = s.hide_kind(&kind, scope(p)?.unwrap_or(crate::hidden::Scope::Project))?;
    Ok(json!({"kind": kind, "kept": kept, "hiddenNow": s.hidden_count()}))
}

fn show_messages(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = super::string(p, "kind")?;
    s.show_kind(kind, scope(p)?)?;
    Ok(json!({"kind": kind, "hiddenNow": s.hidden_count()}))
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
    let (pdf, kind) = s.pdf_to_save()?;
    std::fs::write(&path, pdf.as_slice()).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(json!({"path": path.display().to_string(), "bytes": pdf.len(), "kind": kind}))
}

fn outputs(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"outputs": s.outputs()}))
}

fn save_output_schema() -> Value {
    json!({"type": "object", "properties": {
        "output": {"enum": crate::OUTPUT_KINDS, "description": "Which of the last build's files: its PDF, a print build's PDF/X, the SyncTeX data or the raw log (`build.outputs` lists them)."},
        "path": {"type": "string", "description": "Where to write it. In the browser, the name of the file to download."}
    }, "required": ["output", "path"], "additionalProperties": false})
}

fn has_outputs(s: &Session) -> Result<()> {
    if s.outputs().is_empty() { Err("The last build made no files: compile the project first.".to_owned()) } else { Ok(()) }
}

/// Writes one of the last build's files (`build.outputs`) to `path`; in the browser, hands it to
/// the shell to download under `path`'s file name.
fn save_output(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p.get("output").and_then(Value::as_str).ok_or("the parameter `output` (a string) is missing")?;
    let kind = crate::OUTPUT_KINDS.into_iter().find(|k| *k == name).ok_or_else(|| format!("unknown output `{name}`: pdf, pdfx, synctex or log"))?;
    let data = s.output_data(kind).ok_or_else(|| format!("The last build made no {} file.", s.output_name(kind)))?;
    let path = std::path::PathBuf::from(p.get("path").and_then(Value::as_str).ok_or("the parameter `path` (a string) is missing")?);
    write_output(s, kind, &data, path)
}

#[cfg(target_arch = "wasm32")]
fn write_output(s: &mut Session, kind: &'static str, data: &[u8], path: std::path::PathBuf) -> Result<Value> {
    let name = path.file_name().map_or_else(|| s.output_name(kind), |n| n.to_string_lossy().into_owned());
    s.hand_out(name.clone(), data.to_vec());
    Ok(json!({"output": kind, "path": name, "bytes": data.len(), "download": true}))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_output(s: &mut Session, kind: &'static str, data: &[u8], path: std::path::PathBuf) -> Result<Value> {
    let path = match &s.roots {
        Some(roots) => roots.check(&path, crate::file_access::Access::Write)?,
        None => path,
    };
    std::fs::write(&path, data).map_err(|e| format!("writing {}: {e}", path.display()))?;
    s.output_saved(kind, path.clone());
    Ok(json!({"output": kind, "path": path.display().to_string(), "bytes": data.len()}))
}

fn export_schema() -> Value {
    json!({"type": "object", "properties": {
        "format": {"enum": ["pdf", "odt", "docx", "odp", "pptx", "html", "markdown", "svg"], "description": "The format: PDF is the last build's; the others build the project again and convert it."},
        "path": {"type": "string", "description": "Where to write the file. Markdown's pictures go to a folder beside it, named after it with `_media`."},
        "background": {"type": "boolean", "description": "Convert on a thread and return at once; the outcome comes from `Session::poll_export` (the window's status line). Default: wait for it."}
    }, "required": ["format", "path"], "additionalProperties": false})
}

fn can_export(s: &Session) -> Result<()> {
    has_main(s)?;
    if !s.can_convert() && s.pdf.is_none() {
        return Err("This typesetting engine cannot export: it has no converter to other formats.".to_owned());
    }
    Ok(())
}

/// Exports the project: the PDF is the last build's, any other format is a new build (in the
/// engine's own folder, apart from the shown build) converted by retex. It blocks until done, or
/// with `background` runs on a thread (the window stays responsive; `Session::poll_export`).
fn export(s: &mut Session, p: &Value) -> Result<Value> {
    can_export(s)?;
    let name = p.get("format").and_then(Value::as_str).ok_or("the parameter `format` (a string) is missing")?;
    let format =
        texcraft_compile::Export::parse(name).ok_or_else(|| format!("unknown format `{name}`: pdf, odt, docx, odp, pptx, html, markdown or svg"))?;
    let path = std::path::PathBuf::from(p.get("path").and_then(Value::as_str).ok_or("the parameter `path` (a string) is missing")?);
    export_to(s, p, format, path)
}

/// In the browser: the last build's PDF is handed to the shell, the other formats are made by the
/// engine's worker; the shell downloads the files (`Session::take_exported_files`). `path` is only
/// the file's name.
#[cfg(target_arch = "wasm32")]
fn export_to(s: &mut Session, _: &Value, format: texcraft_compile::Export, path: std::path::PathBuf) -> Result<Value> {
    let name = path.file_name().map_or_else(|| format!("document.{}", format.extension()), |n| n.to_string_lossy().into_owned());
    if format == texcraft_compile::Export::Pdf {
        let (pdf, kind) = s.pdf_to_save()?;
        s.hand_out(name.clone(), pdf.to_vec());
        return Ok(json!({"format": "pdf", "path": name, "bytes": pdf.len(), "kind": kind}));
    }
    s.start_web_export(format, name)?;
    Ok(json!({"format": format, "started": true}))
}

#[cfg(not(target_arch = "wasm32"))]
fn export_to(s: &mut Session, p: &Value, format: texcraft_compile::Export, path: std::path::PathBuf) -> Result<Value> {
    if format == texcraft_compile::Export::Pdf {
        let mut written = save_pdf(s, p)?;
        written["format"] = json!("pdf");
        return Ok(written);
    }
    if s.roots.is_some() {
        // As build.compile: retex cannot yet confine the files a document reads (RFC-0015).
        return Err("Exporting is not available to agents yet: the engine cannot confine the files a document reads.".to_owned());
    }
    let exporter = s.exporter().ok_or("This typesetting engine cannot export to other formats than PDF.")?;
    let job = s.export_job()?;
    let stem = path.file_stem().map_or_else(|| "document".to_owned(), |s| s.to_string_lossy().into_owned());
    let media = format!("{stem}_media");
    let convert = move || -> Result<Value> {
        let exported = exporter(&job, format, &media).map_err(|e| format!("Export to {} failed: {e}", format.extension()))?;
        write_export(format, &path, &exported)
    };
    if p.get("background").and_then(Value::as_bool).unwrap_or(false) {
        if s.exporting() {
            return Err("An export is already running.".to_owned());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("texcraft-export".to_owned())
            .spawn(move || {
                let _ = tx.send(crate::guard::catch_panic(convert).and_then(|r| r));
            })
            .map_err(|e| format!("The export could not start: {e}"))?;
        s.export = Some(rx);
        return Ok(json!({"format": format, "started": true}));
    }
    convert()
}

/// Writes an export's file and its media beside it; what was written, for the command's result.
#[cfg(not(target_arch = "wasm32"))]
fn write_export(format: texcraft_compile::Export, path: &std::path::Path, exported: &texcraft_compile::Exported) -> Result<Value> {
    std::fs::write(path, &exported.data).map_err(|e| format!("writing {}: {e}", path.display()))?;
    let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let mut files = vec![path.display().to_string()];
    for (rel, data) in &exported.media {
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, data).map_err(|e| format!("writing {}: {e}", target.display()))?;
        files.push(target.display().to_string());
    }
    Ok(json!({"format": format, "path": path.display().to_string(), "bytes": exported.data.len(), "files": files}))
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

/// The engine of the last build: the one the settings name, or the one the document chose and why
/// (`chosen: "document"`), or retex's own choice.
fn engine_of(s: &Session) -> Value {
    match (s.settings.build.engine, s.built_engine()) {
        (_, Some(c)) => json!({"engine": c.engine, "chosen": "document", "why": c.why}),
        (texcraft_compile::Engine::Auto, None) => json!({"engine": "auto", "chosen": "retex"}),
        (e, None) => json!({"engine": e, "chosen": "settings"}),
    }
}

fn result(s: &mut Session, _: &Value) -> Result<Value> {
    s.refresh_hidden();
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
        "hidden": s.hidden_count(),
        "pdfBytes": o.pdf.as_ref().map(|p| p.len()),
        "diagnostics": o.diagnostics,
        "draft": draft,
        "output": o.output,
        "engine": engine_of(s),
        "accessibility": o.accessibility().collect::<Vec<_>>(),
        "print": o.preflight.as_ref().map(|p| json!({
            "standard": p.standard.name(),
            "pdfxBytes": p.pdfx.as_ref().map(|x| x.len()),
            "ready": o.print_problems() == Some(0),
            "problems": o.print_problems(),
            "checks": p.checks,
            "rules": p.rules,
        })),
    }))
}
