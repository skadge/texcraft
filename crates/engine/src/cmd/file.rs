//! Files of the project: creating, reading, writing, renaming, deleting, choosing the main
//! document, and which files are open in the editor.

use serde_json::{Value, json};
use texcraft_project::{Contents, ProjectPath};

use super::{CommandSpec, always, path_param, string};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "file.new",
            label: "New File…",
            menu: &["File"],
            shortcut: None,
            params: new_schema,
            enabled: always,
            run: new,
            changes: true,
        },
        CommandSpec {
            id: "file.open",
            label: "Open File",
            menu: &[],
            shortcut: None,
            params: path_schema,
            enabled: always,
            run: open,
            changes: true,
        },
        CommandSpec {
            id: "file.close",
            label: "Close File",
            menu: &["File"],
            shortcut: Some("Cmd+W"),
            params: path_schema,
            enabled: always,
            run: close,
            changes: true,
        },
        CommandSpec {
            id: "file.read",
            label: "Read File",
            menu: &[],
            shortcut: None,
            params: path_schema,
            enabled: always,
            run: read,
            changes: false,
        },
        CommandSpec {
            id: "file.write",
            label: "Replace File Text",
            menu: &[],
            shortcut: None,
            params: write_schema,
            enabled: always,
            run: write,
            changes: true,
        },
        CommandSpec {
            id: "file.rename",
            label: "Rename…",
            menu: &[],
            shortcut: None,
            params: rename_schema,
            enabled: always,
            run: rename,
            changes: true,
        },
        CommandSpec {
            id: "file.delete",
            label: "Delete",
            menu: &[],
            shortcut: None,
            params: path_schema,
            enabled: always,
            run: delete,
            changes: true,
        },
        CommandSpec {
            id: "file.set_main",
            label: "Set as Main Document",
            menu: &[],
            shortcut: None,
            params: path_schema,
            enabled: always,
            run: set_main,
            changes: true,
        },
        CommandSpec {
            id: "file.outline",
            label: "Outline",
            menu: &[],
            shortcut: None,
            params: path_schema,
            enabled: always,
            run: outline,
            changes: false,
        },
    ]
}

fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string", "description": "A path in the project, such as `chapters/intro.tex`."}}, "required": ["path"], "additionalProperties": false})
}

fn new_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "A path in the project."},
        "text": {"type": "string", "description": "The initial text (empty by default)."}
    }, "required": ["path"], "additionalProperties": false})
}

fn write_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "text": {"type": "string", "description": "The file's whole new text."}
    }, "required": ["path", "text"], "additionalProperties": false})
}

fn rename_schema() -> Value {
    json!({"type": "object", "properties": {"from": {"type": "string"}, "to": {"type": "string"}}, "required": ["from", "to"], "additionalProperties": false})
}

fn new(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let text = p.get("text").and_then(Value::as_str).unwrap_or_default().to_owned();
    s.project.add(path.clone(), Contents::Text(text)).map_err(|e| e.to_string())?;
    s.touch();
    show(s, &path);
    Ok(json!({"path": path.as_str()}))
}

/// Makes `path` the shown tab, adding a tab for it.
fn show(s: &mut Session, path: &ProjectPath) {
    if !s.open.contains(path) {
        s.open.push(path.clone());
    }
    s.current = Some(path.clone());
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    if s.project.get(&path).is_none() {
        return Err(format!("there is no file `{path}` in the project"));
    }
    show(s, &path);
    Ok(json!({}))
}

fn close(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let at = s.open.iter().position(|o| *o == path);
    s.open.retain(|o| *o != path);
    if s.current.as_ref() == Some(&path) {
        let next = at.map(|i| i.min(s.open.len().saturating_sub(1)));
        s.current = next.and_then(|i| s.open.get(i).cloned());
    }
    Ok(json!({}))
}

fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    Ok(json!({"text": text}))
}

fn write(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let text = string(p, "text")?.to_owned();
    s.write_text(&path, text)?;
    Ok(json!({"revision": s.revision}))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = path_param(p, "from")?;
    let to = path_param(p, "to")?;
    s.project.rename(&from, to.clone()).map_err(|e| e.to_string())?;
    for o in s.open.iter_mut().filter(|o| **o == from) {
        *o = to.clone();
    }
    if s.current.as_ref() == Some(&from) {
        s.current = Some(to);
    }
    s.touch();
    Ok(json!({}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    s.project.remove(&path).map_err(|e| e.to_string())?;
    close(s, p)?;
    s.touch();
    Ok(json!({}))
}

fn set_main(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    if path.extension() != "tex" {
        return Err(format!("`{path}` is not a .tex file"));
    }
    s.project.set_main(&path).map_err(|e| e.to_string())?;
    s.touch();
    Ok(json!({}))
}

fn outline(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    Ok(json!({"headings": texcraft_latex::outline(text)}))
}
