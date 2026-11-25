//! Projects from a git forge (docs/forges.md; docs/ROADMAP.md, C5), GitLab first: open a
//! project's branch with a personal access token, see what changed, commit it back to the branch.
//!
//! On the desktop the project opens in memory, linked to its branch and in no folder: commits go
//! to the forge, and Save Project As can still copy it into a folder. In the browser it is kept in
//! the library with its link, so it can be committed from after a reload. Tokens are kept for the
//! session only, by instance: never in the library, the preferences or a log.
//!
//! The forge's API is reached over the network, so `forge.open` and `forge.commit` take
//! `background`: they return at once and [`Session::poll_forge`] gives their outcome (the browser
//! always works so). Without it they wait (the command line, agents).

use std::path::PathBuf;

use serde_json::{Value, json};
use texcraft_forge::task::Pending;
use texcraft_forge::{Address, Committed, GitLab, Link, Loaded, Skipped, Token};
use texcraft_project::{Contents, Project, ProjectPath};

use super::{CommandSpec, always, no_params, string};
use crate::{Result, Session};

/// A forge operation running in the background.
pub(crate) enum Task {
    Open { pending: Pending<Loaded>, token: Token },
    Commit { pending: Pending<Committed>, token: Token, files: Vec<(String, Vec<u8>)> },
}

/// Makes the transport a forge operation sends its requests with (on the operation's thread).
pub type Connect = std::sync::Arc<dyn Fn() -> Box<dyn texcraft_forge::Transport> + Send + Sync>;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "forge.open",
            label: "Open from GitLab…",
            menu: &["File"],
            shortcut: None,
            params: open_schema,
            enabled: idle,
            run: open,
            changes: true,
        },
        CommandSpec {
            id: "forge.commit",
            label: "Commit to GitLab…",
            menu: &["File"],
            shortcut: None,
            params: commit_schema,
            enabled: can_commit,
            run: commit,
            changes: true,
        },
        CommandSpec {
            id: "forge.status",
            label: "GitLab Status",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: status,
            changes: false,
        },
    ]
}

fn open_schema() -> Value {
    json!({"type": "object", "properties": {
        "url": {"type": "string", "description": "The project's address: https://gitlab.com/group/project, a page under it, its clone address, or group/project on gitlab.com."},
        "token": {"type": "string", "description": "A personal access token with the read_api scope (api to commit). Kept for this session only. May be left out when one was given for the same instance in this session."},
        "ref": {"type": "string", "description": "The branch; the project's default branch if left out."},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_forge` (the window's status line). Default: wait for it (not in the browser, which always works so)."}
    }, "required": ["url"], "additionalProperties": false})
}

fn commit_schema() -> Value {
    json!({"type": "object", "properties": {
        "message": {"type": "string", "description": "The commit's message."},
        "token": {"type": "string", "description": "A personal access token with the api scope; may be left out when one was given for the project's instance in this session."},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_forge`. Default: wait for it (not in the browser)."}
    }, "required": ["message"], "additionalProperties": false})
}

fn idle(s: &Session) -> Result<()> {
    match &s.forge_task {
        Some(_) => Err("GitLab is still answering the last request: wait for it to end.".to_owned()),
        None => Ok(()),
    }
}

fn can_commit(s: &Session) -> Result<()> {
    if s.forge.is_none() {
        return Err("This project was not opened from GitLab: use Open from GitLab… first.".to_owned());
    }
    idle(s)
}

fn background(p: &Value) -> bool {
    cfg!(target_arch = "wasm32") || p.get("background").and_then(Value::as_bool).unwrap_or(false)
}

/// The token given, or the one given earlier in this session for `address`'s instance.
fn token_for(s: &Session, p: &Value, address: &Address) -> Result<Token> {
    match p.get("token").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
        Some(t) => Token::new(t).map_err(|e| e.to_string()),
        None => s
            .forge_tokens
            .get(&address.instance)
            .cloned()
            .ok_or_else(|| format!("Give a personal access token for {}: TeXCraft keeps it only while it runs, never on disk.", address.instance)),
    }
}

/// Runs the operation `make` returns: in the background (then `None`: [`Session::poll_forge`]
/// gives its outcome), or at once.
fn start<T, F, Fut>(p: &Value, make: F) -> Result<std::result::Result<Pending<T>, T>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = texcraft_forge::Result<T>> + 'static,
    T: Send + 'static,
{
    if background(p) {
        return texcraft_forge::task::spawn(make).map(Ok);
    }
    #[cfg(not(target_arch = "wasm32"))]
    return texcraft_forge::task::block_on(make()).map(Err).map_err(|e| e.to_string());
    #[cfg(target_arch = "wasm32")]
    texcraft_forge::task::spawn(make).map(Ok)
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let address = Address::parse(string(p, "url")?).map_err(|e| e.to_string())?;
    let token = token_for(s, p, &address)?;
    let branch = p.get("ref").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned);
    let (connect, path, sent) = (s.forge_transport.clone(), address.path.clone(), token.clone());
    let make = move || async move {
        let client = GitLab::new(connect(), address, sent);
        texcraft_forge::open(&client, branch.as_deref()).await
    };
    match start(p, make)? {
        Ok(pending) => {
            s.forge_task = Some(Task::Open { pending, token });
            Ok(json!({"operation": "open", "started": true, "project": path}))
        }
        Err(loaded) => opened(s, loaded, token),
    }
}

/// The main document of files loaded from a forge: `main.tex` at the top, else a top-level
/// `.tex` file with `\documentclass`, else any `.tex` file with it.
fn pick_main(project: &Project) -> Option<ProjectPath> {
    let tex: Vec<&ProjectPath> = project.paths().filter(|p| p.extension() == "tex").collect();
    let has_class = |p: &&&ProjectPath| matches!(project.get(p), Some(Contents::Text(t)) if t.contains("\\documentclass"));
    tex.iter()
        .find(|p| p.as_str() == "main.tex")
        .or_else(|| tex.iter().filter(|p| !p.as_str().contains('/')).find(has_class))
        .or_else(|| tex.iter().find(has_class))
        .map(|p| (*p).clone())
}

/// A name for a project the library does not have yet: `name`, or `name (2)`…
fn unused_name(library: &crate::library::Library, name: &str) -> String {
    let name = crate::library::check_name(name).unwrap_or_else(|_| "GitLab project".to_owned());
    (1..).map(|i| if i == 1 { name.clone() } else { format!("{name} ({i})") }).find(|n| !library.contains(n)).unwrap_or(name)
}

/// The project loaded: it becomes the session's, linked to its branch.
fn opened(s: &mut Session, loaded: Loaded, token: Token) -> Result<Value> {
    let Loaded { mut link, files } = loaded;
    let mut project = Project::new();
    for (path, bytes) in files {
        let added = ProjectPath::new(&path).and_then(|pp| project.add(pp, Contents::from_bytes(bytes)));
        if let Err(e) = added {
            // It stays on the branch as it is.
            link.files.remove(&path);
            link.skipped.push(Skipped { path, why: e.to_string() });
        }
    }
    if let Some(main) = pick_main(&project) {
        project.set_main(&main).map_err(|e| e.to_string())?;
    }
    let count = project.len();
    let name = s.library.as_ref().map(|library| unused_name(library, link.address.name()));
    s.replace_project(project, name.as_ref().map(PathBuf::from));
    s.forge_tokens.insert(link.address.instance.clone(), token);
    let result = json!({
        "operation": "open",
        "project": link.address.path,
        "web_url": link.web_url,
        "branch": link.branch,
        "commit": link.base,
        "files": count,
        "skipped": skipped_json(&link.skipped),
        "name": name,
    });
    s.set_forge(Some(link));
    if let Some(name) = name {
        // Kept in the browser at once, with its link.
        super::project::save_to(s, &PathBuf::from(name))?;
    }
    Ok(result)
}

fn skipped_json(skipped: &[Skipped]) -> Value {
    skipped.iter().map(|k| json!({"path": k.path, "why": k.why})).collect()
}

fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    let link = s.forge.clone().ok_or("This project was not opened from GitLab.")?;
    let message = string(p, "message")?.trim().to_owned();
    if message.is_empty() {
        return Err("Say what the commit changes: its message cannot be empty.".to_owned());
    }
    let token = token_for(s, p, &link.address)?;
    let files: Vec<(String, Vec<u8>)> = s.project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect();
    let (connect, sent, snapshot) = (s.forge_transport.clone(), token.clone(), files.clone());
    let make = move || async move {
        let client = GitLab::new(connect(), link.address.clone(), sent);
        texcraft_forge::commit(&client, &link, &snapshot, &message, &texcraft_forge::Choices::new()).await
    };
    match start(p, make)? {
        Ok(pending) => {
            s.forge_task = Some(Task::Commit { pending, token, files });
            Ok(json!({"operation": "commit", "started": true}))
        }
        Err(done) => committed(s, done, token, &files),
    }
}

/// The commit made: the link moves to it, and what others committed meanwhile is taken in, for
/// each file the user has not changed since the commit started.
fn committed(s: &mut Session, done: Committed, token: Token, files: &[(String, Vec<u8>)]) -> Result<Value> {
    s.forge_tokens.insert(done.link.address.instance.clone(), token);
    let then: std::collections::BTreeMap<&str, &[u8]> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let (mut taken, mut removed, mut kept) = (Vec::new(), Vec::new(), Vec::new());
    for (path, bytes) in done.updates.iter().filter_map(|(p, b)| Some((p, b.as_ref()?))) {
        let Ok(pp) = ProjectPath::new(path) else { continue };
        if s.project.get(&pp).map(Contents::as_bytes) != then.get(path.as_str()).copied() {
            kept.push(path.clone());
            continue;
        }
        let contents = Contents::from_bytes(bytes.clone());
        let put = if s.project.get(&pp).is_some() { s.project.set(&pp, contents) } else { s.project.add(pp, contents) };
        match put {
            Ok(()) => taken.push(path.clone()),
            Err(e) => kept.push(format!("{path} ({e})")),
        }
    }
    for path in done.updates.iter().filter(|(_, b)| b.is_none()).map(|(p, _)| p) {
        let Ok(pp) = ProjectPath::new(path) else { continue };
        if s.project.get(&pp).map(Contents::as_bytes) == then.get(path.as_str()).copied() && s.project.remove(&pp).is_ok() {
            removed.push(path.clone());
        } else {
            kept.push(path.clone());
        }
    }
    if !taken.is_empty() || !removed.is_empty() {
        s.touch();
        if s.current.as_ref().is_some_and(|c| s.project.get(c).is_none()) {
            s.current = s.project.main().cloned();
        }
        s.open.retain(|p| s.project.get(p).is_some());
    }
    let web_url = done.link.web_url.clone();
    let branch = done.link.branch.clone();
    s.set_forge(Some(done.link));
    let changes: Vec<Value> = done.changes.iter().map(change_json).collect();
    let result = json!({
        "operation": "commit",
        "commit": done.commit,
        "web_url": done.commit.as_ref().map(|id| format!("{web_url}/-/commit/{id}")),
        "branch": branch,
        "changes": changes,
        "taken": taken,
        "removed": removed,
        "kept": kept,
        "raced": done.raced,
    });
    match s.dir.clone() {
        // The browser keeps the project with its new base, so a reload counts changes from it.
        Some(dir) if s.library.is_some() => {
            super::project::save_to(s, &dir)?;
        }
        // In memory only: nothing is unsaved once the branch has it all.
        None => s.dirty = !s.forge_changes()?.is_empty(),
        Some(_) => {}
    }
    Ok(result)
}

fn change_json(c: &texcraft_forge::FileChange) -> Value {
    json!({"path": c.path, "change": c.kind.as_str(), "from": c.from})
}

fn status(s: &mut Session, _: &Value) -> Result<Value> {
    let running = s.forge_busy();
    let Some(link) = s.forge.clone() else { return Ok(json!({"linked": false, "running": running})) };
    let changes: Vec<Value> = s.forge_changes()?.iter().map(change_json).collect();
    Ok(json!({
        "linked": true,
        "instance": link.address.instance,
        "project": link.address.path,
        "web_url": link.web_url,
        "branch": link.branch,
        "base": link.base,
        "changes": changes,
        "skipped": skipped_json(&link.skipped),
        "token": s.forge_tokens.contains_key(&link.address.instance),
        "running": running,
    }))
}

impl Session {
    /// Reaches forges through the transports `connect` makes, instead of the network (tests).
    pub fn set_forge_transport(&mut self, connect: impl Fn() -> Box<dyn texcraft_forge::Transport> + Send + Sync + 'static) {
        self.forge_transport = std::sync::Arc::new(connect);
    }

    /// The forge branch the project is linked to (C5).
    pub fn forge(&self) -> Option<&Link> {
        self.forge.as_ref()
    }

    pub(crate) fn set_forge(&mut self, link: Option<Link>) {
        self.forge = link;
        self.forge_changes = None;
    }

    /// The forge operation running in the background: `open` or `commit`.
    pub fn forge_busy(&self) -> Option<&'static str> {
        self.forge_task.as_ref().map(|t| match t {
            Task::Open { .. } => "open",
            Task::Commit { .. } => "commit",
        })
    }

    /// Whether a token for the linked project's instance was given in this session.
    pub fn has_forge_token(&self) -> bool {
        self.forge.as_ref().is_some_and(|l| self.forge_tokens.contains_key(&l.address.instance))
    }

    /// The project's files changed since the forge branch's base, sorted by path; none for a
    /// project with no link. Kept until the files or the link change.
    pub fn forge_changes(&mut self) -> Result<Vec<texcraft_forge::FileChange>> {
        let Some(link) = &self.forge else { return Ok(Vec::new()) };
        if let Some((revision, changes)) = &self.forge_changes
            && *revision == self.revision
        {
            return Ok(changes.clone());
        }
        let files: Vec<(String, Vec<u8>)> = self.project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect();
        let changes = texcraft_forge::changes(link, &files).map_err(|e| e.to_string())?;
        self.forge_changes = Some((self.revision, changes.clone()));
        Ok(changes)
    }

    /// The outcome of the forge operation running in the background, once it has ended: what
    /// `forge.open` or `forge.commit` returns when it waits (with `operation`), or why it failed.
    pub fn poll_forge(&mut self) -> Option<Result<Value>> {
        let outcome = match self.forge_task.as_mut()? {
            Task::Open { pending, .. } => pending.poll()?.map(Ok),
            Task::Commit { pending, .. } => pending.poll()?.map(Err),
        };
        let task = self.forge_task.take()?;
        Some(match (outcome, task) {
            (Ok(Ok(loaded)), Task::Open { token, .. }) => opened(self, loaded, token),
            (Ok(Err(done)), Task::Commit { token, files, .. }) => committed(self, done, token, &files),
            (Err(e), _) => Err(e.to_string()),
            _ => Err(texcraft_forge::Error::Stopped.to_string()),
        })
    }
}
