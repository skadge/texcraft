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
    json!({"type": "object", "properties": {"on": {"type": "boolean"}}, "required": ["on"], "additionalProperties": false})
}

fn has_main(s: &Session) -> Result<()> {
    if s.project.main().is_some() { Ok(()) } else { Err("The project has no main document.".to_owned()) }
}

fn can_stop(s: &Session) -> Result<()> {
    if !s.is_building() {
        return Err("Nothing is being compiled.".to_owned());
    }
    Err("The typesetting engine cannot stop a build yet; it finishes on its own.".to_owned())
}

fn compile(s: &mut Session, p: &Value) -> Result<Value> {
    let id = s.start_build()?;
    if p.get("wait").and_then(Value::as_bool).unwrap_or(false) {
        s.wait();
        return result(s, &json!({}));
    }
    Ok(json!({"job": id}))
}

fn stop(_: &mut Session, _: &Value) -> Result<Value> {
    Err("The typesetting engine cannot stop a build yet.".to_owned())
}

fn auto(s: &mut Session, p: &Value) -> Result<Value> {
    s.settings.auto_compile = p.get("on").and_then(Value::as_bool).ok_or("the parameter `on` (a boolean) is missing")?;
    Ok(json!({"on": s.settings.auto_compile}))
}

fn result(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(o) = s.last_outcome() else {
        return Ok(json!({"status": "none", "building": s.is_building()}));
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
    }))
}
