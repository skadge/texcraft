//! The project's history (docs/ROADMAP.md, C8): the git repository its folder is in. A folder in
//! none becomes one on its first save and each save of it is a commit; a repository someone else
//! made is committed to only when asked (`history.commit`).

use base64::Engine as _;
use serde_json::{Value, json};
use texcraft_history::{History, Owner, Person, Snapshot};
use texcraft_project::{Contents, ProjectPath};

use super::{CommandSpec, always, path_param, string};
use crate::file_access::Access;
use crate::{Origin, Result, Session};

/// The message of the commit each save makes.
const SAVE_MESSAGE: &str = "Saved with TeXCraft";
/// How many commits `history.list` gives without a `limit`, and at most.
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 1000;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "history.status",
            label: "History Status",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: always,
            run: status,
            changes: false,
        },
        CommandSpec {
            id: "history.list",
            label: "Project History",
            menu: &[],
            shortcut: None,
            params: list_schema,
            enabled: has_history,
            run: list,
            changes: false,
        },
        CommandSpec {
            id: "history.show",
            label: "Show a File's Version",
            menu: &[],
            shortcut: None,
            params: show_schema,
            enabled: has_history,
            run: show,
            changes: false,
        },
        CommandSpec {
            id: "history.restore",
            label: "Restore a Version",
            menu: &[],
            shortcut: None,
            params: restore_schema,
            enabled: has_history,
            run: restore,
            changes: true,
        },
        CommandSpec {
            id: "history.commit",
            label: "Commit Project",
            menu: &[],
            shortcut: None,
            params: commit_schema,
            enabled: can_commit,
            run: commit,
            changes: true,
        },
    ]
}

fn list_schema() -> Value {
    json!({"type": "object", "properties": {
        "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "How many commits, newest first (50 by default)."}
    }, "additionalProperties": false})
}

fn show_schema() -> Value {
    json!({"type": "object", "properties": {
        "commit": {"type": "string", "description": "A commit's full id, from history.list."},
        "path": {"type": "string", "description": "A file of the project, as at that commit."}
    }, "required": ["commit", "path"], "additionalProperties": false})
}

fn restore_schema() -> Value {
    json!({"type": "object", "properties": {
        "commit": {"type": "string", "description": "A commit's full id, from history.list."},
        "path": {"type": "string", "description": "The file to bring back; without it, the whole project as it was."}
    }, "required": ["commit"], "additionalProperties": false})
}

fn commit_schema() -> Value {
    json!({"type": "object", "properties": {
        "message": {"type": "string", "description": "What the commit changes."}
    }, "required": ["message"], "additionalProperties": false})
}

fn has_history(s: &Session) -> Result<()> {
    match (&s.history, &s.library) {
        (Some(_), _) => Ok(()),
        (None, Some(_)) => Err("A project's history is not kept in the browser yet.".to_owned()),
        (None, None) => Err("The project has no history yet: save it in a folder first.".to_owned()),
    }
}

fn can_commit(s: &Session) -> Result<()> {
    has_history(s)?;
    if s.dirty { Err("Save the project first: a commit holds the files as saved.".to_owned()) } else { Ok(()) }
}

/// Whether the session may reach `history`'s repository for `access` (agents: inside the roots).
fn allowed(s: &Session, history: &History, access: Access) -> Result<()> {
    match &s.roots {
        Some(roots) => roots.check(history.git_dir(), access).map(|_| ()),
        None => Ok(()),
    }
}

/// Finds the repository the project's folder is in (opening a folder: none is made).
pub(super) fn find(s: &mut Session) {
    s.history = None;
    let Some(dir) = s.dir.clone() else { return };
    match History::find(&dir) {
        Ok(Some(history)) => match allowed(s, &history, Access::Read) {
            Ok(()) => s.history = Some(history),
            Err(e) => log::info!("history not used: {e}"),
        },
        Ok(None) => {}
        Err(e) => log::warn!("{e}"),
    }
}

/// After the project was saved in its folder: finds or makes its repository (when the folder
/// changed or it had none), and in TeXCraft's own repository commits the save. Says what was done.
pub(super) fn after_save(s: &mut Session, moved: bool) -> Value {
    match commit_save(s, moved) {
        Ok(value) => value,
        Err(e) => json!({"repository": s.history.as_ref().map(|h| h.owner().as_str()), "commit": null, "error": e}),
    }
}

fn commit_save(s: &mut Session, moved: bool) -> Result<Value> {
    if moved || s.history.is_none() {
        s.history = None;
        let dir = s.dir.clone().ok_or("The project has no folder.")?;
        let history = History::open_or_init(&dir)?;
        allowed(s, &history, Access::Read)?;
        s.history = Some(history);
    }
    let Some(history) = &s.history else { return Ok(json!({"repository": null, "commit": null})) };
    if history.owner() == Owner::Theirs {
        // Someone else's repository: commits only when asked.
        return Ok(json!({"repository": "theirs", "commit": null}));
    }
    let id = commit_now(s, SAVE_MESSAGE)?;
    Ok(json!({"repository": "ours", "commit": id}))
}

/// Commits the session's files and review as they are with `message`; none when nothing changed.
fn commit_now(s: &mut Session, message: &str) -> Result<Option<String>> {
    let snapshot = snapshot(s)?;
    let history = s.history.as_ref().ok_or("The project has no history.")?;
    allowed(s, history, Access::Write)?;
    let author = s.author.as_ref().map(|a| Person::new(a.name.clone(), ""));
    history.commit(&snapshot, message, author.as_ref(), s.now())
}

/// The project's files and its review threads' files, as the session holds them.
fn snapshot(s: &mut Session) -> Result<Snapshot> {
    let dir = texcraft_review::format::DIR;
    let mut files: Vec<(String, Vec<u8>)> = s.project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect();
    let review = s.review.files(&s.project).map_err(|e| format!("The review could not be written: {e}"))?;
    files.extend(review.into_iter().map(|(name, text)| (format!("{dir}/{name}"), text.into_bytes())));
    Ok(Snapshot { files, owned_dirs: vec![dir.to_owned()] })
}

/// The repository the project is in, for `project.open` and `history.status`.
pub(super) fn status_of(s: &Session) -> Value {
    let Some(h) = &s.history else { return json!({"repository": null}) };
    let head = h.head();
    json!({
        "repository": h.owner().as_str(),
        "folder": h.work_dir().display().to_string(),
        "prefix": h.prefix(),
        "branch": h.branch(),
        "head": head.as_ref().ok().cloned().flatten(),
        "error": head.err(),
    })
}

fn status(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(status_of(s))
}

fn history(s: &Session, access: Access) -> Result<&History> {
    let history = s.history.as_ref().ok_or("The project has no history yet: save it in a folder first.")?;
    allowed(s, history, access)?;
    Ok(history)
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let limit = match p.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_LIMIT).contains(n))
            .ok_or_else(|| format!("`limit` is a whole number from 1 to {MAX_LIMIT}"))?,
    };
    let entries = history(s, Access::Read)?.log(limit)?;
    let commits: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "parent": e.parent,
                "time": e.time.to_rfc3339(),
                "author": e.author,
                "message": e.message,
                "changes": e.changes.iter().map(|(path, kind)| json!({"path": path, "change": kind.as_str()})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({"commits": commits}))
}

fn show(s: &mut Session, p: &Value) -> Result<Value> {
    let commit = string(p, "commit")?;
    let path = path_param(p, "path")?;
    let bytes = version(s, commit, &path)?;
    Ok(match Contents::from_bytes(bytes) {
        Contents::Text(text) => json!({"path": path.as_str(), "text": text}),
        Contents::Binary(bytes) => json!({"path": path.as_str(), "base64": base64::engine::general_purpose::STANDARD.encode(bytes)}),
    })
}

/// File `path` at `commit`, or an error saying it was not in that version.
fn version(s: &Session, commit: &str, path: &ProjectPath) -> Result<Vec<u8>> {
    history(s, Access::Read)?.file_at(commit, path.as_str())?.ok_or_else(|| format!("`{path}` is not in the project at commit {commit}."))
}

/// Brings back a file, or the whole project, as at a commit: as unsaved edits, so the next save
/// commits them (the history itself is never rewritten).
fn restore(s: &mut Session, p: &Value) -> Result<Value> {
    let commit = string(p, "commit")?.to_owned();
    let files: Vec<(ProjectPath, Vec<u8>)> = match p.get("path") {
        None | Some(Value::Null) => {
            let files = history(s, Access::Read)?.files_at(&commit)?;
            // A path the project cannot hold is left out, as opening the folder would.
            files.into_iter().filter_map(|(path, bytes)| ProjectPath::new(&path).ok().map(|p| (p, bytes))).collect()
        }
        Some(_) => {
            let path = path_param(p, "path")?;
            let bytes = version(s, &commit, &path)?;
            vec![(path, bytes)]
        }
    };
    let mut removed = Vec::new();
    if p.get("path").is_none_or(Value::is_null) {
        let kept: std::collections::BTreeSet<&ProjectPath> = files.iter().map(|(p, _)| p).collect();
        let gone: Vec<ProjectPath> = s.project.paths().filter(|p| !kept.contains(p)).cloned().collect();
        for path in gone {
            s.project.remove(&path).map_err(|e| e.to_string())?;
            super::file::close_tab(s, &path);
            removed.push(path.as_str().to_owned());
        }
        if !removed.is_empty() {
            s.touch();
        }
    }
    let mut restored = Vec::new();
    for (path, bytes) in files {
        if s.project.get(&path).map(Contents::as_bytes) == Some(bytes.as_slice()) {
            continue;
        }
        match (Contents::from_bytes(bytes), s.project.get(&path)) {
            // Text over text as an edit, so comments' anchors follow it.
            (Contents::Text(text), Some(Contents::Text(_))) => s.write_text(&path, text, Origin::Local)?,
            (contents, Some(_)) => {
                s.project.set(&path, contents).map_err(|e| e.to_string())?;
                s.touch();
            }
            (contents, None) => {
                s.project.add(path.clone(), contents).map_err(|e| e.to_string())?;
                s.touch();
            }
        }
        restored.push(path.as_str().to_owned());
    }
    Ok(json!({"commit": commit, "restored": restored, "removed": removed, "unsaved": s.dirty}))
}

fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    let message = string(p, "message")?.trim().to_owned();
    if message.is_empty() {
        return Err("A commit needs a message.".to_owned());
    }
    let id = commit_now(s, &message)?;
    let note = if id.is_none() { Some("Nothing changed since the last commit.") } else { None };
    Ok(json!({"commit": id, "note": note}))
}
