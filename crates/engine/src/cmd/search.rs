//! Finding and replacing across the project's text files: what the side panel's Search shows,
//! with the same matching as `file.find` and `file.replace` ([`crate::find::Query`]).

use serde_json::{Value, json};
use texcraft_project::{Contents, ProjectPath};

use super::file::{query, query_properties, replacements};
use super::{CommandSpec, always, string};
use crate::find::MAX_MATCHES;
use crate::{Origin, Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "project.find",
            label: "Find in Project",
            menu: &[],
            shortcut: None,
            params: find_schema,
            enabled: always,
            run: find,
            changes: false,
        },
        CommandSpec {
            id: "project.replace",
            label: "Replace in Project",
            menu: &[],
            shortcut: None,
            params: replace_schema,
            enabled: always,
            run: replace,
            changes: true,
        },
    ]
}

fn find_schema() -> Value {
    let mut props = query_properties();
    props.remove("path");
    json!({"type": "object", "properties": props, "required": ["pattern"], "additionalProperties": false})
}

fn replace_schema() -> Value {
    let mut props = query_properties();
    props.remove("path");
    props.insert("replacement".to_owned(), json!({"type": "string", "description": "The replacement; with `regex`, `$1` is the first group."}));
    props.insert(
        "files".to_owned(),
        json!({"type": "array", "items": {"type": "string"}, "description": "Only in these files (default: every text file of the project)."}),
    );
    json!({"type": "object", "properties": props, "required": ["pattern", "replacement"], "additionalProperties": false})
}

/// The project's text files, in path order.
fn text_files(s: &Session) -> Vec<ProjectPath> {
    s.project.files().filter(|(_, c)| matches!(c, Contents::Text(_))).map(|(p, _)| p.clone()).collect()
}

/// Lists the matches in every text file, grouped by file: each with its line and column (from 1,
/// in characters), its text, and its line's text before and after it. At most
/// [`MAX_MATCHES`] in all; `truncated` says when the limit was reached.
fn find(s: &mut Session, p: &Value) -> Result<Value> {
    let q = query(p)?;
    q.compile()?;
    let mut files = Vec::new();
    let mut total = 0;
    for path in text_files(s) {
        let text = s.project.text(&path).map_err(|e| e.to_string())?;
        let mut ranges = q.matches(text)?;
        if ranges.is_empty() {
            continue;
        }
        ranges.truncate(MAX_MATCHES - total);
        let places = crate::find::places(text, &ranges);
        let matches: Vec<Value> = ranges
            .iter()
            .zip(places)
            .map(|(r, at)| json!({"line": at.line, "column": at.column, "text": &text[r.clone()], "before": at.before, "after": at.after}))
            .collect();
        total += matches.len();
        files.push(json!({"path": path.as_str(), "count": matches.len(), "matches": matches}));
        if total >= MAX_MATCHES {
            break;
        }
    }
    // At the limit, there may be more.
    Ok(json!({"count": total, "files": files, "truncated": total >= MAX_MATCHES}))
}

/// Replaces every match in every text file (or in `files`): one edit per file, so Undo in the
/// editor takes back a file's replacements in one step. Nothing changes when a file cannot be
/// read as text or a replacement cannot be made.
fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let q = query(p)?;
    q.compile()?;
    let replacement = string(p, "replacement")?;
    let paths = match p.get("files") {
        None | Some(Value::Null) => text_files(s),
        Some(Value::Array(list)) => list
            .iter()
            .map(|v| {
                let name = v.as_str().ok_or("`files` lists paths (strings)")?;
                ProjectPath::new(name).map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => return Err("`files` is a list of paths".to_owned()),
    };
    // Every edit is worked out before any is made.
    let mut edits = Vec::new();
    for path in paths {
        let tx = replacements(s, &path, &q, replacement)?;
        if !tx.changes().is_empty() {
            edits.push((path, tx));
        }
    }
    let mut files = Vec::new();
    let mut total = 0;
    for (path, tx) in edits {
        s.apply_edit(&path, &tx, Origin::Local)?;
        total += tx.changes().len();
        files.push(json!({"path": path.as_str(), "replaced": tx.changes().len()}));
    }
    Ok(json!({"replaced": total, "files": files, "revision": s.revision}))
}
