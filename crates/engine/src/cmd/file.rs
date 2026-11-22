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
            id: "file.edit",
            label: "Edit File Text",
            menu: &[],
            shortcut: None,
            params: edit_schema,
            enabled: always,
            run: edit,
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
            id: "file.import",
            label: "Add Files…",
            menu: &["File"],
            shortcut: None,
            params: import_schema,
            enabled: on_disk,
            run: import,
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

fn edit_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "start": {"type": "integer", "minimum": 0, "description": "Byte offset where the replaced range starts."},
        "end": {"type": "integer", "minimum": 0, "description": "Byte offset where it ends (excluded)."},
        "text": {"type": "string", "description": "The replacement."}
    }, "required": ["path", "start", "end", "text"], "additionalProperties": false})
}

fn import_schema() -> Value {
    json!({"type": "object", "properties": {
        "source": {"type": "string", "description": "A file on disk to copy into the project."},
        "path": {"type": "string", "description": "Its path in the project (default: its file name)."}
    }, "required": ["source"], "additionalProperties": false})
}

fn on_disk(_: &Session) -> Result<()> {
    if cfg!(target_arch = "wasm32") { Err("Adding files from disk is not available in the browser yet.".to_owned()) } else { Ok(()) }
}

/// Copies a file from disk into the project, replacing a file of the same path.
fn import(s: &mut Session, p: &Value) -> Result<Value> {
    on_disk(s)?;
    let source = std::path::PathBuf::from(string(p, "source")?);
    let source = match &s.roots {
        Some(roots) => roots.check(&source, crate::file_access::Access::Read)?,
        None => source,
    };
    let name = match p.get("path").and_then(Value::as_str) {
        Some(path) => path.to_owned(),
        None => source.file_name().and_then(|n| n.to_str()).ok_or_else(|| format!("{} has no file name", source.display()))?.to_owned(),
    };
    let path = ProjectPath::new(&name).map_err(|e| e.to_string())?;
    let len = std::fs::metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?.len();
    if len > texcraft_project::MAX_FILE_BYTES as u64 {
        return Err(format!("{} is larger than a project file may be", source.display()));
    }
    let bytes = std::fs::read(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    let contents = Contents::from_bytes(bytes);
    if s.project.get(&path).is_some() {
        s.project.set(&path, contents).map_err(|e| e.to_string())?;
    } else {
        s.project.add(path.clone(), contents).map_err(|e| e.to_string())?;
    }
    s.touch();
    Ok(json!({"path": path.as_str()}))
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

/// Replaces the bytes `start..end` of a text file. Both ends must fall on character boundaries.
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let offset = |name: &str| -> Result<usize> {
        let v = p.get(name).and_then(Value::as_u64).ok_or_else(|| format!("the parameter `{name}` (a byte offset) is missing"))?;
        usize::try_from(v).map_err(|_| format!("`{name}` is too large"))
    };
    let (start, end) = (offset("start")?, offset("end")?);
    let replacement = string(p, "text")?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    if start > end || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(format!("{start}..{end} is not a range of whole characters in `{path}` ({} bytes)", text.len()));
    }
    let mut new = String::with_capacity(text.len() - (end - start) + replacement.len());
    new.push_str(&text[..start]);
    new.push_str(replacement);
    new.push_str(&text[end..]);
    s.write_text(&path, new)?;
    Ok(json!({"revision": s.revision, "end": start + replacement.len()}))
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
