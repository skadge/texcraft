//! Review: comment threads on passages of the sources (docs/collaboration-roadmap.md, R1).
//! Positions are characters from 0, as `file.edit`'s.

use serde_json::{Value, json};
use texcraft_review::{Anchor, Filter, Id, Message, Thread};

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
    ]
}

fn list_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "Only the threads of this file."},
        "state": {"type": "string", "enum": ["open", "resolved", "all"], "description": "Default `all`."},
        "author": {"type": "string", "description": "Only threads with a message by this name or key."}
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
    let filter = Filter {
        path: p.get("path").and_then(Value::as_str).map(str::to_owned),
        open,
        author: p.get("author").and_then(Value::as_str).map(str::to_owned),
    };
    let threads: Vec<Value> = s.review.review.list(&filter).map(|t| describe(s, t)).collect();
    let all = s.review.review.threads();
    let open = all.iter().filter(|t| t.is_open()).count();
    Ok(json!({
        "threads": threads,
        "open": open,
        "resolved": all.len() - open,
        "problems": s.review.problems,
    }))
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
