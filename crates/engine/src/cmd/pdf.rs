//! Going between the PDF and the source, for agents as the window does: a point of a page to the
//! source line that made it, and a source line to its place in the PDF. Pages are from 1, points
//! are PDF points from the page's top left corner, lines are from 1.

use serde_json::{Value, json};

use super::{CommandSpec, always, path_param};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "pdf.source",
            label: "Go to Source",
            menu: &[],
            shortcut: None,
            params: source_schema,
            enabled: always,
            run: source,
            changes: false,
        },
        CommandSpec {
            id: "pdf.locate",
            label: "Show in PDF",
            menu: &[],
            shortcut: None,
            params: locate_schema,
            enabled: always,
            run: locate,
            changes: false,
        },
    ]
}

fn source_schema() -> Value {
    json!({"type": "object", "properties": {
        "page": {"type": "integer", "minimum": 1},
        "x": {"type": "number", "description": "PDF points from the page's left edge."},
        "y": {"type": "number", "description": "PDF points from the page's top edge."}
    }, "required": ["page", "x", "y"], "additionalProperties": false})
}

fn locate_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}, "line": {"type": "integer", "minimum": 1}}, "required": ["path", "line"], "additionalProperties": false})
}

fn number(p: &Value, name: &str) -> Result<f64> {
    p.get(name).and_then(Value::as_f64).ok_or_else(|| format!("`{name}` is missing"))
}

fn source(s: &mut Session, p: &Value) -> Result<Value> {
    let page = p.get("page").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or("`page` (from 1) is missing")?;
    let (x, y) = (number(p, "x")? as f32, number(p, "y")? as f32);
    match s.source_at(page as u32 - 1, x, y) {
        Some((path, line)) => Ok(json!({"path": path.as_str(), "line": line})),
        None => Err("No source line is known there: build the project first, or point at its text.".to_owned()),
    }
}

fn locate(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let line = p.get("line").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or("`line` (from 1) is missing")?;
    let rects: Vec<Value> =
        s.locate(&path, line as u32).iter().map(|r| json!({"page": r.page + 1, "x": r.x, "y": r.y, "width": r.width, "height": r.height})).collect();
    Ok(json!({"rects": rects}))
}
