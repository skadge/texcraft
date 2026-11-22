//! The language server's answers for agents: completion, hover, definition and the lint, as the
//! editor gets them. Lines and columns are from 1, in characters, as `file.find` gives them.

use serde_json::{Value, json};

use super::{CommandSpec, always, path_param};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    let spec =
        |id, label, params: fn() -> Value, run| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, changes: false };
    vec![
        spec("language.complete", "Completions", at_schema, complete),
        spec("language.hover", "Describe", at_schema, hover),
        spec("language.definition", "Go to Definition", at_schema, definition),
        spec("language.lint", "Hints", path_schema, lint),
    ]
}

fn at_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1, "description": "From 1."},
        "column": {"type": "integer", "minimum": 1, "description": "From 1, in characters; the caret is before this character."}
    }, "required": ["path", "line", "column"], "additionalProperties": false})
}

fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false})
}

fn at(p: &Value) -> Result<(texcraft_project::ProjectPath, usize, usize)> {
    let path = path_param(p, "path")?;
    let n = |name: &str| -> Result<usize> {
        p.get(name).and_then(Value::as_u64).filter(|n| *n >= 1).map(|n| n as usize - 1).ok_or_else(|| format!("`{name}` (from 1) is missing"))
    };
    Ok((path, n("line")?, n("column")?))
}

fn complete(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line, column) = at(p)?;
    let items: Vec<Value> = s
        .complete(&path, line, column)
        .into_iter()
        .map(|c| json!({"label": c.label, "detail": c.detail, "text": c.text, "replace_from_column": c.from + 1}))
        .collect();
    Ok(json!({"items": items}))
}

fn hover(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line, column) = at(p)?;
    Ok(json!({"markdown": s.hover(&path, line, column)}))
}

fn definition(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line, column) = at(p)?;
    Ok(match s.definition(&path, line, column) {
        Some((file, l, c)) => json!({"path": file.as_str(), "line": l + 1, "column": c + 1}),
        None => Value::Null,
    })
}

fn lint(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let hints: Vec<Value> = s
        .lint(&path)
        .into_iter()
        .map(|l| json!({"line": l.line + 1, "column": l.column + 1, "end_line": l.end_line + 1, "end_column": l.end_column + 1, "severity": l.severity, "message": l.message}))
        .collect();
    Ok(json!({"hints": hints}))
}
