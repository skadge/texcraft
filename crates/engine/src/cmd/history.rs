//! The project's history (docs/ROADMAP.md, C8): the git repository its folder is in. A folder in
//! none becomes one on its first save and each save of it is a commit; in a repository someone
//! else made, File › Save commits nothing, and versions are the automatic ones and those asked for.
//! An automatic version amends the one this session made before it while that one is not pushed
//! (C9), so they stay few in every repository.
//!
//! A project of the library (the browser) keeps its history there instead, as values: each save
//! that changed it is a version, and the same commands read it.

use base64::Engine as _;
use serde_json::{Value, json};
use texcraft_history::{Amended, History, Owner, Person, Snapshot, Versioned};
use texcraft_project::{Contents, ProjectPath};

use super::{CommandSpec, always, path_param, string};
use crate::file_access::Access;
use crate::{Origin, Result, Session};

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
            id: "history.diff",
            label: "Compare a File With Its Previous Version",
            menu: &[],
            shortcut: None,
            params: show_schema,
            enabled: has_history,
            run: diff,
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
            id: "history.version",
            label: "Make a Version",
            menu: &[],
            shortcut: None,
            params: version_schema,
            enabled: has_place,
            run: version_command,
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

fn version_schema() -> Value {
    json!({"type": "object", "properties": {
        "message": {"type": "string", "description": "The version's name (its commit message); by default it says what changed."},
        "automatic": {"type": "boolean", "description": "Made by TeXCraft itself (after a pause in typing, when the project closes). A version without a message amends the one this session made before it, when that one was not named and not pushed, so automatic versions stay few. Default false."}
    }, "additionalProperties": false})
}

/// The project has a place for its versions: a folder, or a name in the browser's library.
fn has_place(s: &Session) -> Result<()> {
    if s.dir.is_some() {
        return Ok(());
    }
    Err(if s.library.is_some() {
        "The project has no name yet: save it first.".to_owned()
    } else {
        "The project has no folder yet: save it in a folder first.".to_owned()
    })
}

/// `history.version`: the project's changes written, then a version made of them.
fn version_command(s: &mut Session, p: &Value) -> Result<Value> {
    let message = p.get("message").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned);
    let automatic = p.get("automatic").and_then(Value::as_bool).unwrap_or(false);
    make_version(s, message.as_deref(), automatic)
}

/// Writes the project's changes, then makes a version of it with `message` (none: what changed,
/// [`crate::describe`]): a commit in its folder's repository (made when there is none), or a
/// version in the library, in any repository. It amends the session's own automatic version
/// before it while that one is not pushed ([`commit_now`]); `automatic` (TeXCraft's own, after a
/// pause, on closing, before files are rewritten) makes it one the next version amends. Says what
/// was done; `commit` is null when nothing changed since the latest version.
pub(crate) fn make_version(s: &mut Session, message: Option<&str>, automatic: bool) -> Result<Value> {
    let Some(dir) = s.dir.clone() else { return Ok(json!({"commit": null, "why": "no place"})) };
    if s.dirty {
        super::project::write_to(s, &dir)?;
    }
    if s.library.is_some() {
        let name = dir.display().to_string();
        let (id, amended) = record_in_library(s, &name, message, automatic)?;
        return Ok(json!({"repository": "ours", "commit": id, "amended": amended, "automatic": automatic}));
    }
    if s.history.is_none() {
        let history = History::open_or_init(&dir)?;
        allowed(s, &history, Access::Read)?;
        s.history = Some(history);
    }
    let owner = s.history.as_ref().map(History::owner);
    let (id, amended) = commit_now(s, message, automatic)?;
    Ok(json!({"repository": owner.map(Owner::as_str), "commit": id, "amended": amended, "automatic": automatic}))
}

/// Records the project as a version of library project `name`, with `message` (none: what
/// changed), in place of the session's own automatic version while it is the latest, as
/// [`commit_now`] amends a commit.
fn record_in_library(s: &mut Session, name: &str, message: Option<&str>, automatic: bool) -> Result<(Option<String>, bool)> {
    let snapshot = snapshot(s)?;
    let empty = texcraft_history::store::Versions::default();
    let author = s.author.as_ref().map_or_else(|| "TeXCraft".to_owned(), |a| a.name.clone());
    let now = s.now();
    let own = s.own_version.clone().filter(|id| s.library.as_ref().and_then(|l| l.history(name)).and_then(|v| v.head()) == Some(id.as_str()));
    let library = s.library.as_mut().ok_or("The project is not kept in the browser.")?;
    let made = match &own {
        Some(old) => {
            let versions = library.history(name).unwrap_or(&empty);
            let parent = versions.parent_of(old)?;
            let message = message.map_or_else(|| message_since(versions, parent.as_deref(), &snapshot), str::to_owned);
            Made::from(library.amend(name, old, &snapshot, &message, &author, now)?)
        }
        None => {
            let message = message.map_or_else(|| save_message(library.history(name).unwrap_or(&empty), &snapshot), str::to_owned);
            library.record(name, &snapshot, &message, &author, now)?.map_or(Made::Nothing, Made::New)
        }
    };
    Ok(made.keep(s, automatic))
}

/// What making a version did.
enum Made {
    /// Nothing changed since the latest version.
    Nothing,
    New(String),
    /// The session's own version was replaced by this one ([`Amended`]).
    Amended(Amended),
}

impl From<Amended> for Made {
    fn from(a: Amended) -> Self {
        Made::Amended(a)
    }
}

impl Made {
    /// Remembers the version as the session's own to amend next when TeXCraft made it by itself
    /// (`automatic`); one asked for (named, a save, before a sync or a push), or one undone back
    /// to the version before, is never amended: the next version is a commit of its own.
    /// The version's id (none: nothing changed), and whether it amended one.
    fn keep(self, s: &mut Session, automatic: bool) -> (Option<String>, bool) {
        s.mark_versioned();
        let (id, amended, own) = match self {
            Made::Nothing => return (None, false),
            Made::Amended(Amended::Same) => return (None, false),
            Made::New(id) => (id, false, true),
            Made::Amended(Amended::Replaced(id)) => (id, true, true),
            Made::Amended(Amended::Undone(parent)) => (parent, true, false),
        };
        s.own_version = (own && automatic).then(|| id.clone());
        (Some(id), amended)
    }
}

fn has_history(s: &Session) -> Result<()> {
    match (&s.history, &s.library) {
        (Some(_), _) => Ok(()),
        (None, Some(_)) => library_history(s).map(drop),
        (None, None) => Err("The project has no history yet: save it in a folder first.".to_owned()),
    }
}

/// The history of the open project of the library.
fn library_history(s: &Session) -> Result<&texcraft_history::store::Versions> {
    let library = s.library.as_ref().ok_or("The project is not kept in the browser.")?;
    let name = s.dir.as_ref().map(|d| d.display().to_string()).ok_or("The project has no history yet: save it first.")?;
    library.history(&name).ok_or_else(|| "The project has no history yet: save it first.".to_owned())
}

fn can_commit(s: &Session) -> Result<()> {
    if s.library.is_some() {
        return Err("In the browser every save that changes the project is a version: there is nothing to commit.".to_owned());
    }
    has_history(s)?;
    if s.dirty { Err("Save the project first: a commit holds the files as saved.".to_owned()) } else { Ok(()) }
}

/// Whether the session may reach `history`'s repository for `access` (agents: inside the roots).
pub(crate) fn allowed(s: &Session, history: &History, access: Access) -> Result<()> {
    match &s.roots {
        Some(roots) => roots.check(history.git_dir(), access).map(|_| ()),
        None => Ok(()),
    }
}

/// Finds the repository the project's folder is in (opening a folder: none is made).
pub(super) fn find(s: &mut Session) {
    s.history = None;
    if s.library.is_some() {
        // A project of the library has a name, not a folder: its history is in the library.
        return;
    }
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
        // Someone else's repository: a save is not a commit there (versions are).
        return Ok(json!({"repository": "theirs", "commit": null}));
    }
    let (id, amended) = commit_now(s, None, false)?;
    Ok(json!({"repository": "ours", "commit": id, "amended": amended}))
}

/// After the project was saved in the library as `name`: records the save as a version of its
/// history (amending the session's own latest one, [`record_in_library`]). Says what was done.
pub(super) fn after_library_save(s: &mut Session, name: &str) -> Value {
    match record_in_library(s, name, None, false) {
        Ok((id, amended)) => json!({"repository": "ours", "commit": id, "amended": amended}),
        Err(e) => json!({"repository": "ours", "commit": null, "error": e}),
    }
}

/// Commits the session's files and review as they are with `message` (none: what changed,
/// [`crate::describe`]). The commit amends the session's own automatic one (made by this process,
/// [`Made::keep`]) while that is still the branch's tip and on no other ref: not pushed, not
/// someone else's; the amended commit's message says what changed since its parent. `automatic`:
/// the next commit may amend this one. The commit's id (none: nothing changed), and whether it
/// amended one.
fn commit_now(s: &mut Session, message: Option<&str>, automatic: bool) -> Result<(Option<String>, bool)> {
    let snapshot = snapshot(s)?;
    let history = s.history.as_ref().ok_or("The project has no history.")?;
    allowed(s, history, Access::Write)?;
    let author = s.author.as_ref().map(|a| Person::new(a.name.clone(), ""));
    let now = s.now();
    let own = s.own_version.clone().filter(|id| history.head().ok().flatten().as_deref() == Some(id.as_str()) && history.shared(id) == Ok(false));
    let amended = match &own {
        Some(old) => {
            let parent = history.parent_of(old)?;
            let message = message.map_or_else(|| message_since(history, parent.as_deref(), &snapshot), str::to_owned);
            // A branch moved meanwhile (git outside TeXCraft): a new commit instead.
            history.amend(old, &snapshot, &message, author.as_ref(), now).ok().map(Made::from)
        }
        None => None,
    };
    let made = match amended {
        Some(made) => made,
        None => {
            let message = message.map_or_else(|| save_message(history, &snapshot), str::to_owned);
            history.commit(&snapshot, &message, author.as_ref(), now)?.map_or(Made::Nothing, Made::New)
        }
    };
    Ok(made.keep(s, automatic))
}

/// The message of the version a save makes of `snapshot`: what changed since the latest one.
fn save_message(versions: &dyn Versioned, snapshot: &Snapshot) -> String {
    // A repository with no commit yet, or one that cannot be read: everything is new.
    let latest = versions.latest().ok().flatten();
    message_since(versions, latest.as_deref(), snapshot)
}

/// What changed in `snapshot` since version `base` (none: everything is new), as a version's
/// message.
fn message_since(versions: &dyn Versioned, base: Option<&str>, snapshot: &Snapshot) -> String {
    let before = base.and_then(|c| versions.ids_at(c).ok()).unwrap_or_default();
    let old_text = |path: &str| base.and_then(|c| versions.file_at(c, path).ok().flatten());
    crate::describe::message(&before, &snapshot.files, &old_text)
}

/// The project's files and its review threads' files, as the session holds them.
pub(crate) fn snapshot(s: &mut Session) -> Result<Snapshot> {
    let dir = texcraft_review::format::DIR;
    let mut files: Vec<(String, Vec<u8>)> = s.project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect();
    let review = s.review.files(&s.project).map_err(|e| format!("The review could not be written: {e}"))?;
    files.extend(review.into_iter().map(|(name, text)| (format!("{dir}/{name}"), text.into_bytes())));
    Ok(Snapshot { files, owned_dirs: vec![dir.to_owned()] })
}

/// The repository the project is in, for `project.open` and `history.status`.
pub(super) fn status_of(s: &Session) -> Value {
    if s.library.is_some() {
        let Ok(versions) = library_history(s) else { return json!({"repository": null}) };
        // Kept like a repository TeXCraft made: every save is a version.
        let name = s.dir.as_ref().map(|d| d.display().to_string());
        return json!({"repository": "ours", "kept": "browser", "folder": name, "prefix": "", "branch": null, "head": versions.head()});
    }
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

/// The project's history: its folder's repository (agents: inside their roots), or its versions
/// in the library.
fn history(s: &Session, access: Access) -> Result<&dyn Versioned> {
    if s.library.is_some() {
        return Ok(library_history(s)?);
    }
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

/// How a file changed in `commit`, line by line, against the commit's first parent: the changed
/// lines with three lines around them, in hunks.
fn diff(s: &mut Session, p: &Value) -> Result<Value> {
    let commit = string(p, "commit")?.to_owned();
    let path = path_param(p, "path")?;
    let history = history(s, Access::Read)?;
    let parent = history.parent_of(&commit)?;
    let new = history.file_at(&commit, path.as_str())?;
    let old = match &parent {
        Some(parent) => history.file_at(parent, path.as_str())?,
        None => None,
    };
    let change = match (&old, &new) {
        (None, None) => return Err(format!("`{path}` is not in the project at commit {commit} nor before it.")),
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        (Some(_), Some(_)) => "modified",
    };
    let mut result = json!({"path": path.as_str(), "commit": commit, "parent": parent, "change": change});
    let text = |bytes: Option<Vec<u8>>| match bytes.map(Contents::from_bytes) {
        None => Some(String::new()),
        Some(Contents::Text(t)) => Some(t),
        Some(Contents::Binary(_)) => None,
    };
    let (Some(old), Some(new)) = (text(old), text(new)) else {
        result["binary"] = json!(true);
        return Ok(result);
    };
    result["binary"] = json!(false);
    let (hunks, added, removed, truncated) = line_diff(&old, &new);
    result["hunks"] = json!(hunks);
    result["added"] = json!(added);
    result["removed"] = json!(removed);
    result["truncated"] = json!(truncated);
    Ok(result)
}

/// At most this many lines of a diff are given.
const MAX_DIFF_LINES: usize = 5000;

/// The hunks of a line diff from `old` to `new`, how many lines were added and removed, and
/// whether lines were left out (past [`MAX_DIFF_LINES`]).
fn line_diff(old: &str, new: &str) -> (Vec<Value>, usize, usize, bool) {
    let mut config = similar::TextDiff::configure();
    // A pathological pair of files gives a coarser diff rather than a stall (the browser has no
    // clock similar can read).
    if cfg!(not(target_arch = "wasm32")) {
        config.timeout(std::time::Duration::from_millis(500));
    }
    let diff = config.diff_lines(old, new);
    let (mut added, mut removed, mut shown, mut truncated) = (0, 0, 0, false);
    let mut hunks = Vec::new();
    for group in diff.grouped_ops(3) {
        let mut lines = Vec::new();
        for op in &group {
            for change in diff.iter_changes(op) {
                let kind = match change.tag() {
                    similar::ChangeTag::Insert => {
                        added += 1;
                        "added"
                    }
                    similar::ChangeTag::Delete => {
                        removed += 1;
                        "removed"
                    }
                    similar::ChangeTag::Equal => "same",
                };
                if shown >= MAX_DIFF_LINES {
                    truncated = true;
                    continue;
                }
                shown += 1;
                let text = change.value();
                lines.push(json!({
                    "kind": kind,
                    "old": change.old_index().map(|i| i + 1),
                    "new": change.new_index().map(|i| i + 1),
                    "text": text.strip_suffix('\n').unwrap_or(text),
                }));
            }
        }
        if !lines.is_empty() {
            hunks.push(json!({"lines": lines}));
        }
    }
    (hunks, added, removed, truncated)
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
    can_commit(s)?;
    let message = string(p, "message")?.trim().to_owned();
    if message.is_empty() {
        return Err("A commit needs a message.".to_owned());
    }
    let (id, _) = commit_now(s, Some(&message), false)?;
    let note = if id.is_none() { Some("Nothing changed since the last commit.") } else { None };
    Ok(json!({"commit": id, "note": note}))
}
