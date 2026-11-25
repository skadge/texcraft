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
        spec("language.formulas", "Typeset Formulas", path_schema, formulas),
        spec("language.fixes", "Quick Fixes", line_schema, fixes),
        CommandSpec {
            id: "language.fix",
            label: "Apply Quick Fix",
            menu: &[],
            shortcut: None,
            params: fix_schema,
            enabled: always,
            run: fix,
            changes: true,
        },
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

fn line_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}, "line": {"type": "integer", "minimum": 1}}, "required": ["path", "line"], "additionalProperties": false})
}

fn fix_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1},
        "index": {"type": "integer", "minimum": 0, "description": "Which of language.fixes's fixes for the line."}
    }, "required": ["path", "line", "index"], "additionalProperties": false})
}

fn line_of(p: &Value) -> Result<(texcraft_project::ProjectPath, usize)> {
    let path = path_param(p, "path")?;
    let line = p.get("line").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or("`line` (from 1) is missing")?;
    Ok((path, line as usize - 1))
}

fn fixes(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line) = line_of(p)?;
    let fixes: Vec<Value> = s.fixes(&path, line).iter().map(|f| json!({"title": f.title, "problem": f.problem, "preferred": f.preferred})).collect();
    Ok(json!({"fixes": fixes}))
}

/// Applies a fix to the file's text.
fn fix(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line) = line_of(p)?;
    let index = p.get("index").and_then(Value::as_u64).ok_or("`index` is missing")? as usize;
    let fix = s.fixes(&path, line).into_iter().nth(index).ok_or("There is no such fix on that line.")?;
    let text = s.project().text(&path).map_err(|e| e.to_string())?.to_owned();
    let new = crate::apply_edits(&text, &fix.edits)?;
    s.execute("file.write", &json!({"path": path.as_str(), "text": new}))?;
    Ok(json!({"applied": fix.title}))
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

/// The file's formulas typeset as the formatted view draws them, after waiting for the batch:
/// each one's box in TeX points, or why it was not typeset.
fn formulas(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    s.formulas(&path)?;
    s.wait_for_formulas();
    let found = s.formulas(&path)?;
    let text = s.project().text(&path).map_err(|e| e.to_string())?;
    let line_of = |byte: usize| text.get(..byte).map_or(0, |t| t.matches('\n').count()) + 1;
    let items: Vec<Value> = found
        .formulas
        .iter()
        .map(|f| {
            let mut v = json!({"line": line_of(f.range.start), "display": f.display, "source": f.source});
            let state = match &f.state {
                crate::FormulaState::Typeset(b) => {
                    json!({"status": "typeset", "width_pt": b.width_pt, "height_pt": b.height_pt, "depth_pt": b.depth_pt})
                }
                crate::FormulaState::Failed(messages) => json!({"status": "error", "messages": messages}),
                crate::FormulaState::Pending => json!({"status": "pending"}),
                crate::FormulaState::Unavailable(why) => json!({"status": "unavailable", "messages": [why]}),
            };
            if let (Some(v), Some(state)) = (v.as_object_mut(), state.as_object()) {
                v.extend(state.clone());
            }
            v
        })
        .collect();
    Ok(json!({"body_pt": found.body_pt, "formulas": items}))
}
