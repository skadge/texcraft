//! Files of the project: creating, reading, writing, renaming, deleting, choosing the main
//! document, and which files are open in the editor.

use serde_json::{Value, json};
use texcraft_project::{Contents, ProjectPath};

use super::{CommandSpec, always, no_params, path_param, string};
use crate::import::{self, Gathered};
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
            id: "file.new_folder",
            label: "New Folder…",
            menu: &["File"],
            shortcut: None,
            params: path_schema,
            enabled: always,
            run: new_folder,
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
            enabled: always,
            run: import,
            changes: true,
        },
        CommandSpec {
            id: "file.unzip",
            label: "Import a Zip File…",
            menu: &["File"],
            shortcut: None,
            params: unzip_schema,
            enabled: always,
            run: unzip,
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
        CommandSpec {
            id: "file.document_outline",
            label: "Document Outline",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: document_outline,
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

pub(super) fn query_properties() -> serde_json::Map<String, Value> {
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

pub(super) fn query(p: &Value) -> Result<crate::find::Query> {
    let flag = |name: &str| p.get(name).and_then(Value::as_bool).unwrap_or(false);
    Ok(crate::find::Query { pattern: string(p, "pattern")?.to_owned(), regex: flag("regex"), case: flag("case"), word: flag("word") })
}

/// Lists the matches with their line and column (from 1, in characters).
fn find(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let q = query(p)?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    let ranges = q.matches(text)?;
    let places = crate::find::places(text, &ranges);
    let matches: Vec<Value> = ranges
        .iter()
        .zip(places)
        .map(|(r, at)| json!({"line": at.line, "column": at.column, "start": r.start, "end": r.end, "text": &text[r.clone()]}))
        .collect();
    Ok(json!({"count": matches.len(), "matches": matches}))
}

/// Replaces every match, as one edit with a change per match (so what is between them, and the
/// comments on it, stays where it is).
fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let q = query(p)?;
    let replacement = string(p, "replacement")?;
    let tx = replacements(s, &path, &q, replacement)?;
    let count = tx.changes().len();
    s.apply_edit(&path, &tx, Origin::Local)?;
    Ok(json!({"replaced": count, "revision": s.revision}))
}

/// The edit that replaces every match of `q` in `path`'s text with `replacement`: a change per
/// match, so it is undone in one step.
pub(super) fn replacements(s: &Session, path: &ProjectPath, q: &crate::find::Query, replacement: &str) -> Result<Transaction> {
    let text = s.project.text(path).map_err(|e| e.to_string())?;
    let mut changes = Vec::new();
    let (mut chars, mut scanned) = (0, 0);
    for r in q.matches(text)? {
        chars += text[scanned..r.start].chars().count();
        let len = text[r.clone()].chars().count();
        changes.push(Change { start: chars, end: chars + len, text: q.replacement_for(text, r.clone(), replacement)? });
        chars += len;
        scanned = r.end;
    }
    Transaction::new(changes).map_err(|e| e.to_string())
}

fn import_schema() -> Value {
    let mut props = json!({
        "source": {"type": "string", "description": "A file on disk to copy into the project, or a folder: its files are copied with their folders, in a folder of its name (hidden files, build outputs and links out of it are left out)."},
        "data": {"type": "string", "description": "Or one file's contents, in base64 (the browser, a dropped file); `path` is then required."},
        "path": {"type": "string", "description": "The one file's path in the project (default: its file name)."},
        "files": {"type": "array", "description": "Or several files by path and contents (a folder the browser read), filtered as a folder is.", "items": {"type": "object", "properties": {
            "path": {"type": "string", "description": "Its path, such as `figures/plot.png`."},
            "data": {"type": "string", "description": "Its contents, in base64."}
        }, "required": ["path", "data"], "additionalProperties": false}}
    });
    if let Some(map) = props.as_object_mut() {
        map.extend(import::shared_properties());
    }
    json!({"type": "object", "properties": props, "additionalProperties": false})
}

fn unzip_schema() -> Value {
    let mut props = json!({
        "source": {"type": "string", "description": "A zip file on disk."},
        "data": {"type": "string", "description": "Or the zip file's contents, in base64."}
    });
    if let Some(map) = props.as_object_mut() {
        map.extend(import::shared_properties());
    }
    json!({"type": "object", "properties": props, "additionalProperties": false})
}

fn on_disk(_: &Session) -> Result<()> {
    if cfg!(target_arch = "wasm32") { Err("Adding files from disk is not available in the browser yet.".to_owned()) } else { Ok(()) }
}

/// Bytes given in base64 as parameter `name`.
pub(super) fn base64_param(p: &Value, name: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;
    let data = p.get(name).and_then(Value::as_str).ok_or_else(|| format!("`{name}` must be a base64 string"))?;
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| format!("`{name}` is not base64: {e}"))
}

/// The files of parameter `files`: paths and contents in base64.
pub(super) fn files_param(p: &Value) -> Result<Vec<(String, Vec<u8>)>> {
    let list = p.get("files").and_then(Value::as_array).ok_or("`files` must be a list of paths and contents")?;
    list.iter().map(|f| Ok((string(f, "path")?.to_owned(), base64_param(f, "data")?))).collect()
}

/// A file or folder on disk named by parameter `source`, checked in an agent session.
pub(super) fn source_param(s: &Session, p: &Value) -> Result<std::path::PathBuf> {
    on_disk(s)?;
    let source = std::path::PathBuf::from(string(p, "source")?);
    match &s.roots {
        Some(roots) => roots.check(&source, crate::file_access::Access::Read),
        None => Ok(source),
    }
}

/// The bytes of file `source`, refused past `max` bytes.
pub(super) fn read_source(source: &std::path::Path, max: u64) -> Result<Vec<u8>> {
    let len = std::fs::metadata(source).map_err(|e| format!("{}: {e}", source.display()))?.len();
    if len > max {
        return Err(format!("{} is larger than TeXCraft reads ({max} bytes)", source.display()));
    }
    std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))
}

/// Copies files into the project: one file from disk or given in base64, a folder from disk, or
/// several files given in base64. A file of the same path is replaced unless `existing` says
/// otherwise; with `check`, nothing changes and the answer says what would.
fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let limits = import::Limits::default();
    let gathered = if p.get("files").is_some() {
        Gathered::from_files(files_param(p)?, limits, false)
    } else if p.get("data").is_some() {
        let bytes = base64_param(p, "data")?;
        if bytes.len() > texcraft_project::MAX_FILE_BYTES {
            return Err("The file is larger than a project file may be.".to_owned());
        }
        let path = ProjectPath::new(string(p, "path")?).map_err(|e| e.to_string())?;
        Gathered::one(path.into(), bytes)
    } else {
        let source = source_param(s, p)?;
        if source.is_dir() {
            if p.get("path").is_some() {
                return Err("`path` names one file: a folder keeps its name (use `into` for where it goes).".to_owned());
            }
            Gathered::from_folder(&source, limits, true, s.roots.as_ref())?
        } else {
            let name = match p.get("path").and_then(Value::as_str) {
                Some(path) => path.to_owned(),
                None => source.file_name().and_then(|n| n.to_str()).ok_or_else(|| format!("{} has no file name", source.display()))?.to_owned(),
            };
            let path = ProjectPath::new(&name).map_err(|e| e.to_string())?;
            let bytes = read_source(&source, texcraft_project::MAX_FILE_BYTES as u64)
                .map_err(|_| format!("{} is larger than a project file may be, or cannot be read", source.display()))?;
            Gathered::one(path.into(), bytes)
        }
    };
    bring_in(s, p, gathered)
}

/// Unpacks a zip file into the project (`file.unzip`).
fn unzip(s: &mut Session, p: &Value) -> Result<Value> {
    let limits = import::Limits::default();
    let bytes = if p.get("data").is_some() { base64_param(p, "data")? } else { read_source(&source_param(s, p)?, limits.bytes)? };
    let gathered = Gathered::from_zip(&bytes, limits)?;
    bring_in(s, p, gathered)
}

/// Adds what was gathered, or with `check` says what adding it would do.
fn bring_in(s: &mut Session, p: &Value, gathered: Gathered) -> Result<Value> {
    let into = import::into(p)?;
    let existing = import::Existing::from_params(p)?;
    if p.get("check").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(import::check(s, into.as_ref(), &gathered));
    }
    let report = import::apply(s, into.as_ref(), gathered, existing);
    let mut result = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    // One file: its path, as adding one file has always answered.
    if let ([only], Some(map)) = (report.files.as_slice(), result.as_object_mut()) {
        map.insert("path".to_owned(), json!(only));
    }
    Ok(result)
}

fn rename_schema() -> Value {
    json!({"type": "object", "properties": {"from": {"type": "string"}, "to": {"type": "string"}}, "required": ["from", "to"], "additionalProperties": false})
}

fn new(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let text = p.get("text").and_then(Value::as_str).unwrap_or_default().to_owned();
    if s.folders.contains(&path) {
        return Err(format!("a folder `{path}` already exists"));
    }
    s.project.add(path.clone(), Contents::Text(text)).map_err(|e| e.to_string())?;
    s.touch();
    show(s, &path);
    Ok(json!({"path": path.as_str()}))
}

/// Adds an empty folder to the file tree. A project holds files only, so the folder is kept by
/// the session until a file is created in it or moved to it (`file.new`, `file.rename`); one that
/// is still empty is not saved.
fn new_folder(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    if s.project.get(&path).is_some() {
        return Err(format!("a file `{path}` already exists"));
    }
    let inside = format!("{}/", path.as_str());
    if s.folders.contains(&path) || s.project.paths().any(|f| f.as_str().starts_with(&inside)) {
        return Err(format!("a folder `{path}` already exists"));
    }
    if s.project.paths().any(|f| path.as_str().starts_with(&format!("{}/", f.as_str()))) {
        return Err(format!("`{path}` is inside a file"));
    }
    // Nothing to save until a file is in it, so the project does not become unsaved.
    s.folders.insert(path.clone());
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
    close_tab(s, &path);
    Ok(json!({}))
}

/// Closes `path`'s tab, if open; the next one is shown when it was.
pub(super) fn close_tab(s: &mut Session, path: &ProjectPath) {
    let at = s.open.iter().position(|o| o == path);
    s.open.retain(|o| o != path);
    if s.current.as_ref() == Some(path) {
        let next = at.map(|i| i.min(s.open.len().saturating_sub(1)));
        s.current = next.and_then(|i| s.open.get(i).cloned());
    }
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
    if from == to {
        return Ok(json!({}));
    }
    if s.folders.contains(&to) {
        return Err(format!("a folder `{to}` already exists"));
    }
    s.project.rename(&from, to.clone()).map_err(|e| e.to_string())?;
    for o in s.open.iter_mut().filter(|o| **o == from) {
        *o = to.clone();
    }
    if s.current.as_ref() == Some(&from) {
        s.current = Some(to.clone());
    }
    s.touch();
    for id in s.review.review.rename(from.as_str(), to.as_str()) {
        s.review_changed(id)?;
    }
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

/// The whole document's outline, from the main file through its `\input`s and `\include`s: each
/// entry with its file and line (from 0), a heading's level, title and number, or an include that
/// is not followed and why (`missing`, `loop`, `too_deep`).
fn document_outline(s: &mut Session, _: &Value) -> Result<Value> {
    let Some(main) = s.project.main().cloned() else { return Err("The project has no main document.".to_owned()) };
    let entries = s.document_outline();
    Ok(json!({"main": main.as_str(), "entries": *entries}))
}
