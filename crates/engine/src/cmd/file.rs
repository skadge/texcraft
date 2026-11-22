//! Files of the project: creating, reading, writing, renaming, deleting, choosing the main
//! document, and which files are open in the editor.

use serde_json::{Value, json};
use texcraft_project::{Contents, ProjectPath};

use super::{CommandSpec, always, path_param, string};
use crate::{Change, Origin, Result, Session, Transaction};

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
            id: "file.find",
            label: "Find in File",
            menu: &[],
            shortcut: None,
            params: find_schema,
            enabled: always,
            run: find,
            changes: false,
        },
        CommandSpec {
            id: "file.replace",
            label: "Replace in File",
            menu: &[],
            shortcut: None,
            params: replace_schema,
            enabled: always,
            run: replace,
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

fn origin_schema() -> Value {
    let names: Vec<&str> = Origin::ALL.iter().map(|o| o.name()).collect();
    json!({"type": "string", "enum": names, "description": "Who made the change: `local` (default: typed, or made by a command or an agent here), `undo`, `redo`, `remote` (a peer), `review` (an accepted suggestion), `external` (changed outside TeXCraft)."})
}

fn write_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "text": {"type": "string", "description": "The file's whole new text. The session records it as the smallest edit that makes it."},
        "origin": origin_schema()
    }, "required": ["path", "text"], "additionalProperties": false})
}

fn edit_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "edits": {
            "type": "array",
            "description": "Replacements made at once. Offsets are characters (Unicode scalar values, from 0) in the text as it is before this call, not in the text earlier edits of the list make; the edits may come in any order but must not overlap.",
            "items": {"type": "object", "properties": {
                "start": {"type": "integer", "minimum": 0, "description": "Where the replaced range starts (characters)."},
                "end": {"type": "integer", "minimum": 0, "description": "Where it ends, excluded; equal to `start` to insert."},
                "text": {"type": "string", "description": "The replacement; empty to delete."}
            }, "required": ["start", "end", "text"], "additionalProperties": false}
        },
        "origin": origin_schema()
    }, "required": ["path", "edits"], "additionalProperties": false})
}

fn query_properties() -> serde_json::Map<String, Value> {
    let v = json!({
        "path": {"type": "string"},
        "pattern": {"type": "string", "description": "The text to find, or a regular expression with `regex`."},
        "regex": {"type": "boolean", "description": "`pattern` is a regular expression (Rust regex syntax). Default false."},
        "case": {"type": "boolean", "description": "Upper and lower case differ. Default false."},
        "word": {"type": "boolean", "description": "Only whole words match. Default false."}
    });
    v.as_object().cloned().unwrap_or_default()
}

fn find_schema() -> Value {
    json!({"type": "object", "properties": query_properties(), "required": ["path", "pattern"], "additionalProperties": false})
}

fn replace_schema() -> Value {
    let mut props = query_properties();
    props.insert("replacement".to_owned(), json!({"type": "string", "description": "The replacement; with `regex`, `$1` is the first group."}));
    json!({"type": "object", "properties": props, "required": ["path", "pattern", "replacement"], "additionalProperties": false})
}

fn query(p: &Value) -> Result<crate::find::Query> {
    let flag = |name: &str| p.get(name).and_then(Value::as_bool).unwrap_or(false);
    Ok(crate::find::Query { pattern: string(p, "pattern")?.to_owned(), regex: flag("regex"), case: flag("case"), word: flag("word") })
}

/// Lists the matches with their line and column (from 1, in characters).
fn find(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let q = query(p)?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    let ranges = q.matches(text)?;
    let mut line = 1;
    let mut line_start = 0;
    let mut scanned = 0;
    let matches: Vec<Value> = ranges
        .iter()
        .map(|r| {
            for (i, c) in text[scanned..r.start].char_indices() {
                if c == '\n' {
                    line += 1;
                    line_start = scanned + i + 1;
                }
            }
            scanned = r.start;
            let column = text[line_start..r.start].chars().count() + 1;
            json!({"line": line, "column": column, "start": r.start, "end": r.end, "text": &text[r.clone()]})
        })
        .collect();
    Ok(json!({"count": matches.len(), "matches": matches}))
}

/// Replaces every match, as one edit with a change per match (so what is between them, and the
/// comments on it, stays where it is).
fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let q = query(p)?;
    let replacement = string(p, "replacement")?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    let mut changes = Vec::new();
    let (mut chars, mut scanned) = (0, 0);
    for r in q.matches(text)? {
        chars += text[scanned..r.start].chars().count();
        let len = text[r.clone()].chars().count();
        changes.push(Change { start: chars, end: chars + len, text: q.replacement_for(text, r.clone(), replacement)? });
        chars += len;
        scanned = r.end;
    }
    let count = changes.len();
    let tx = Transaction::new(changes).map_err(|e| e.to_string())?;
    s.apply_edit(&path, &tx, Origin::Local)?;
    Ok(json!({"replaced": count, "revision": s.revision}))
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
    s.write_text(&path, text, origin(p)?)?;
    Ok(json!({"revision": s.revision}))
}

fn origin(p: &Value) -> Result<Origin> {
    match p.get("origin").and_then(Value::as_str) {
        None => Ok(Origin::Local),
        Some(name) => Origin::from_name(name).ok_or_else(|| format!("`{name}` is not an origin")),
    }
}

/// Makes several replacements at once (character offsets in the text before them).
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let list = p.get("edits").and_then(Value::as_array).ok_or("the parameter `edits` (a list) is missing")?;
    let offset = |e: &Value, name: &str| -> Result<usize> {
        let v = e.get(name).and_then(Value::as_u64).ok_or_else(|| format!("an edit has no `{name}` (a character offset)"))?;
        usize::try_from(v).map_err(|_| format!("`{name}` is too large"))
    };
    let changes = list
        .iter()
        .map(|e| {
            let text = e.get("text").and_then(Value::as_str).ok_or("an edit has no `text`")?;
            Ok(Change { start: offset(e, "start")?, end: offset(e, "end")?, text: text.to_owned() })
        })
        .collect::<Result<Vec<_>>>()?;
    let tx = Transaction::new(changes).map_err(|e| e.to_string())?;
    s.apply_edit(&path, &tx, origin(p)?)?;
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
        s.current = Some(to.clone());
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
