//! Review: comment threads on passages of the sources (docs/collaboration-roadmap.md, R1) and
//! suggested changes (R2). Positions are characters from 0, as `file.edit`'s.

use serde_json::{Value, json};
use texcraft_buffer::{Change, Origin, Transaction};
use texcraft_review::{Accepted, Anchor, Filter, Id, Kind, Message, Thread};

use super::{CommandSpec, always, path_param, string};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    let spec = |id, label, params, run, changes| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, changes };
    vec![
        spec("review.list", "List Comments", list_schema, list, false),
        spec("review.comment", "Comment", comment_schema, comment, true),
        spec("review.reply", "Reply", reply_schema, reply, true),
        spec("review.edit", "Edit Comment", edit_schema, edit, true),
        spec("review.delete", "Delete Comment", delete_schema, delete, true),
        spec("review.resolve", "Resolve", id_schema, resolve, true),
        spec("review.reopen", "Reopen", id_schema, reopen, true),
        spec("review.suggest", "Suggest a Change", suggest_schema, suggest, true),
        spec("review.accept", "Accept Suggestion", id_schema, accept, true),
        spec("review.reject", "Reject Suggestion", id_schema, reject, true),
        spec("review.accept_all", "Accept All Suggestions", all_schema, accept_all, true),
        spec("review.reject_all", "Reject All Suggestions", all_schema, reject_all, true),
    ]
}

fn list_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "Only the threads of this file."},
        "state": {"type": "string", "enum": ["open", "resolved", "all"], "description": "Default `all`."},
        "author": {"type": "string", "description": "Only threads with a message or suggestion by this name or key."},
        "kind": {"type": "string", "enum": ["comment", "suggestion", "all"], "description": "Default `all`."}
    }, "additionalProperties": false})
}

fn suggest_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "start": {"type": "integer", "minimum": 0, "description": "Where the passage to replace starts (characters from 0)."},
        "end": {"type": "integer", "minimum": 0, "description": "Where it ends, excluded; equal to `start` to insert."},
        "text": {"type": "string", "description": "What it would become; empty to delete it."},
        "id": {"type": "string", "description": "One's own open suggestion to change instead of making a new one: it then replaces start..end with text."}
    }, "required": ["path", "start", "end", "text"], "additionalProperties": false})
}

fn all_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "Only this file's suggestions."},
        "author": {"type": "string", "description": "Only this author's (a name or a key)."}
    }, "additionalProperties": false})
}

fn comment_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "start": {"type": "integer", "minimum": 0, "description": "Where the passage starts (characters from 0)."},
        "end": {"type": "integer", "minimum": 0, "description": "Where it ends, excluded."},
        "text": {"type": "string", "description": "The comment."}
    }, "required": ["path", "start", "end", "text"], "additionalProperties": false})
}

fn reply_schema() -> Value {
    json!({"type": "object", "properties": {"id": {"type": "string", "description": "The thread."}, "text": {"type": "string"}}, "required": ["id", "text"], "additionalProperties": false})
}

fn edit_schema() -> Value {
    json!({"type": "object", "properties": {
        "id": {"type": "string", "description": "The thread."},
        "message": {"type": "string", "description": "One's own message in it."},
        "text": {"type": "string", "description": "Its new text."}
    }, "required": ["id", "message", "text"], "additionalProperties": false})
}

fn delete_schema() -> Value {
    json!({"type": "object", "properties": {
        "id": {"type": "string", "description": "The thread: all of it, when one started it."},
        "message": {"type": "string", "description": "Only this message of one's own."}
    }, "required": ["id"], "additionalProperties": false})
}

fn id_schema() -> Value {
    json!({"type": "object", "properties": {"id": {"type": "string", "description": "The thread."}}, "required": ["id"], "additionalProperties": false})
}

fn id_param(p: &Value, name: &str) -> Result<Id> {
    let text = string(p, name)?;
    Id::parse(text).ok_or_else(|| format!("`{text}` is not an id"))
}

fn offset(p: &Value, name: &str) -> Result<usize> {
    let v = p.get(name).and_then(Value::as_u64).ok_or_else(|| format!("the parameter `{name}` (a character offset) is missing"))?;
    usize::try_from(v).map_err(|_| format!("`{name}` is too large"))
}

/// A thread for automation: as in its file, with the line (from 1) it is on now.
fn describe(s: &Session, t: &Thread) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
    let line = texcraft_project::ProjectPath::new(&t.anchor.path)
        .ok()
        .and_then(|p| s.project.text(&p).ok().map(|text| text.chars().take(t.anchor.start).filter(|c| *c == '\n').count() + 1));
    if let Some(map) = v.as_object_mut() {
        map.insert("line".to_owned(), json!(line));
        map.insert("kind".to_owned(), json!(if t.is_suggestion() { "suggestion" } else { "comment" }));
    }
    v
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let open = match p.get("state").and_then(Value::as_str) {
        None | Some("all") => None,
        Some("open") => Some(true),
        Some("resolved") => Some(false),
        Some(other) => return Err(format!("`{other}` is not a state: use open, resolved or all")),
    };
    let kind = match p.get("kind").and_then(Value::as_str) {
        None | Some("all") => None,
        Some("comment") => Some(Kind::Comment),
        Some("suggestion") => Some(Kind::Suggestion),
        Some(other) => return Err(format!("`{other}` is not a kind: use comment, suggestion or all")),
    };
    let filter = Filter {
        path: p.get("path").and_then(Value::as_str).map(str::to_owned),
        open,
        author: p.get("author").and_then(Value::as_str).map(str::to_owned),
        kind,
    };
    let threads: Vec<Value> = s.review.review.list(&filter).map(|t| describe(s, t)).collect();
    let all = s.review.review.threads();
    let open = all.iter().filter(|t| t.is_open()).count();
    let suggestions = all.iter().filter(|t| t.is_open() && t.is_suggestion()).count();
    Ok(json!({
        "threads": threads,
        "open": open,
        "resolved": all.len() - open,
        "openSuggestions": suggestions,
        "problems": s.review.problems,
    }))
}

/// Suggests replacing `start..end` of a file with `text`, without changing the file; with `id`,
/// changes one's own open suggestion instead.
fn suggest(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let (start, end) = (offset(p, "start")?, offset(p, "end")?);
    let text = string(p, "text")?;
    let author = s.author_or_refuse()?;
    let source = s.project.text(&path).map_err(|e| e.to_string())?.to_owned();
    let len = source.chars().count();
    if start > end || end > len {
        return Err(texcraft_review::Error::OutOfText { start, end, len }.to_string());
    }
    let now = s.now();
    let id = match p.get("id") {
        Some(_) => {
            let id = id_param(p, "id")?;
            if s.review.review.get(id).is_some_and(|t| t.anchor.path != path.as_str()) {
                return Err(format!("the suggestion `{id}` is not in `{path}`"));
            }
            let anchor = Anchor::new(path.as_str(), &source, start, end);
            s.review.review.revise(id, &author, anchor, text, now).map_err(|e| e.to_string())?;
            id
        }
        None => {
            let id = s.new_id()?;
            let anchor = Anchor::new(path.as_str(), &source, start, end);
            s.review.review.suggest(id, anchor, &source, text, author, now).map_err(|e| e.to_string())?;
            id
        }
    };
    s.review_changed(id)?;
    let thread = s.review.review.get(id).map(|t| describe(s, t));
    Ok(json!({"id": id.to_string(), "thread": thread}))
}

/// Makes `list` (edits of one file each, grouped by file in order) as one edit per file, with
/// the review origin, and marks them accepted.
fn make(s: &mut Session, list: Vec<Accepted>) -> Result<usize> {
    let author = s.author_or_refuse()?;
    let mut done = 0;
    let mut rest = list.as_slice();
    while let Some(first) = rest.first() {
        let n = rest.iter().take_while(|a| a.path == first.path).count();
        let (group, next) = rest.split_at(n);
        rest = next;
        let path = texcraft_project::ProjectPath::new(&first.path).map_err(|e| e.to_string())?;
        let changes = group.iter().map(|a| Change { start: a.start, end: a.end, text: a.replacement.clone() }).collect();
        let tx = Transaction::new(changes).map_err(|e| e.to_string())?;
        s.apply_edit(&path, &tx, Origin::Review)?;
        let now = s.now();
        s.review.review.accepted(group, &author, now);
        for a in group {
            s.review_changed(a.id)?;
        }
        done += group.len();
    }
    Ok(done)
}

/// The suggestions still open that cannot be applied (their passage changed), of `filter`.
fn outdated(s: &Session, filter: &Filter) -> Vec<String> {
    s.review.review.list(filter).filter(|t| t.is_open() && t.is_suggestion() && !t.can_accept()).map(|t| t.id.to_string()).collect()
}

fn accept(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id")?;
    s.author_or_refuse()?;
    let a = s.review.review.to_accept(id).map_err(|e| e.to_string())?;
    make(s, vec![a])?;
    Ok(json!({"revision": s.revision, "outdated": outdated(s, &Filter::default())}))
}

fn reject(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id")?;
    let author = s.author_or_refuse()?;
    let now = s.now();
    s.review.review.reject(id, &author, now).map_err(|e| e.to_string())?;
    s.review_changed(id)?;
    Ok(json!({}))
}

fn all_filter(p: &Value) -> Result<Filter> {
    let path = match p.get("path") {
        Some(_) => Some(path_param(p, "path")?.as_str().to_owned()),
        None => None,
    };
    Ok(Filter { path, open: Some(true), author: p.get("author").and_then(Value::as_str).map(str::to_owned), kind: Some(Kind::Suggestion) })
}

/// Accepts every open suggestion (of a file, of an author) that can be applied: one undoable
/// edit per file. Suggestions that overlap one accepted become outdated, and are listed.
fn accept_all(s: &mut Session, p: &Value) -> Result<Value> {
    let filter = all_filter(p)?;
    s.author_or_refuse()?;
    let list = s.review.review.to_accept_all(&filter);
    let accepted = make(s, list)?;
    Ok(json!({"accepted": accepted, "outdated": outdated(s, &filter), "revision": s.revision}))
}

fn reject_all(s: &mut Session, p: &Value) -> Result<Value> {
    let filter = all_filter(p)?;
    let author = s.author_or_refuse()?;
    let ids: Vec<Id> = s.review.review.list(&filter).map(|t| t.id).collect();
    let now = s.now();
    for id in &ids {
        s.review.review.reject(*id, &author, now).map_err(|e| e.to_string())?;
        s.review_changed(*id)?;
    }
    Ok(json!({"rejected": ids.len()}))
}

fn comment(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let (start, end) = (offset(p, "start")?, offset(p, "end")?);
    let text = string(p, "text")?;
    let author = s.author_or_refuse()?;
    let source = s.project.text(&path).map_err(|e| e.to_string())?;
    let len = source.chars().count();
    if start > end || end > len {
        return Err(texcraft_review::Error::OutOfText { start, end, len }.to_string());
    }
    let anchor = Anchor::new(path.as_str(), source, start, end);
    let (id, message) = (s.new_id()?, s.new_id()?);
    let now = s.now();
    s.review.review.comment(id, anchor, Message::new(message, author, now, text)).map_err(|e| e.to_string())?;
    s.review_changed(id)?;
    let thread = s.review.review.get(id).map(|t| describe(s, t));
    Ok(json!({"id": id.to_string(), "thread": thread}))
}

fn reply(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id")?;
    let author = s.author_or_refuse()?;
    let message = s.new_id()?;
    let now = s.now();
    s.review.review.reply(id, Message::new(message, author, now, string(p, "text")?)).map_err(|e| e.to_string())?;
    s.review_changed(id)?;
    Ok(json!({"message": message.to_string()}))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, message) = (id_param(p, "id")?, id_param(p, "message")?);
    let author = s.author_or_refuse()?;
    let now = s.now();
    s.review.review.edit(id, message, &author, string(p, "text")?, now).map_err(|e| e.to_string())?;
    s.review_changed(id)?;
    Ok(json!({}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id")?;
    let author = s.author_or_refuse()?;
    match p.get("message") {
        Some(_) => s.review.review.delete_message(id, id_param(p, "message")?, &author).map_err(|e| e.to_string())?,
        None => {
            s.review.review.remove(id, &author).map_err(|e| e.to_string())?;
        }
    }
    s.review_changed(id)?;
    Ok(json!({}))
}

fn resolve(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id")?;
    let author = s.author_or_refuse()?;
    let now = s.now();
    s.review.review.resolve(id, &author, now).map_err(|e| e.to_string())?;
    s.review_changed(id)?;
    Ok(json!({}))
}

fn reopen(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id")?;
    s.review.review.reopen(id).map_err(|e| e.to_string())?;
    s.review_changed(id)?;
    Ok(json!({}))
}
