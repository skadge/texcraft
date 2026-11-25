//! Projects from a git forge (docs/forges.md; docs/ROADMAP.md, C5), GitLab or GitHub: open a
//! project's branch with a personal access token, see what changed, commit it back to the branch.
//!
//! On the desktop the project opens in memory, linked to its branch and in no folder: commits go
//! to the forge, and Save Project As can still copy it into a folder. In the browser it is kept in
//! the library with its link, so it can be committed from after a reload. Tokens are kept for the
//! session only, by instance: never in the library, the preferences or a log.
//!
//! `forge.pull` takes in the branch's new commits, merging files changed on both sides line by
//! line; a file that does not merge is a conflict, listed by `forge.status`, which a pull or a
//! commit with `resolve` (keep mine, take theirs) settles, or a commit to a new branch with a merge
//! request (a pull request on GitHub; `new_branch`) avoids. The review threads (`.texcraft/review/`) go with the files.
//!
//! The forge's API is reached over the network, so `forge.open`, `forge.pull` and `forge.commit`
//! take `background`: they return at once and [`Session::poll_forge`] gives their outcome (the
//! browser always works so). Without it they wait (the command line, agents).

use std::path::PathBuf;

use serde_json::{Value, json};
use texcraft_forge::oauth::{self, Clients, Grant};
use texcraft_forge::task::Pending;
use texcraft_forge::{Address, Choice, Choices, Client, Committed, Link, Loaded, Pulled, Service, Skipped, Token, Update};
use texcraft_project::{Contents, Project, ProjectPath};

use super::{CommandSpec, always, no_params, string};
use crate::{Result, Session};

/// A forge operation running in the background, with the key it was given and the slot its
/// sign-in, renewed on the way, is put in.
pub(crate) enum Task {
    Open { pending: Pending<Loaded>, key: Key, renewed: Renewed },
    Commit { pending: Pending<Committed>, key: Key, renewed: Renewed, files: Vec<(String, Vec<u8>)> },
    Pull { pending: Pending<Pulled>, key: Key, renewed: Renewed, files: Vec<(String, Vec<u8>)> },
}

/// What an operation reaches the forge with: a personal access token, or a sign-in.
#[derive(Clone)]
pub(crate) enum Key {
    Token(Token),
    Grant(Box<Grant>),
}

/// A sign-in renewed while an operation ran (its access token had expired): kept whatever the
/// operation's outcome, since GitLab's old refresh token no longer works.
pub(crate) type Renewed = std::sync::Arc<std::sync::Mutex<Option<Grant>>>;

impl Key {
    /// The token to send, renewing the sign-in first when it has expired (into `renewed`).
    pub(super) async fn token(
        self,
        transport: &dyn texcraft_forge::Transport,
        clients: &Clients,
        renewed: &Renewed,
    ) -> texcraft_forge::Result<Token> {
        match self {
            Key::Token(t) => Ok(t),
            Key::Grant(grant) => {
                let (token, new) = oauth::fresh(transport, clients, &grant, oauth::now()).await?;
                if let Some(new) = new {
                    *renewed.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(new);
                }
                Ok(token)
            }
        }
    }
}

/// The last forge operation's refusal the interface offers a way out of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Files that do not merge with the branch's new commits: keep mine or take theirs for each,
    /// or commit to a new branch with a merge request.
    Conflicts(Vec<texcraft_forge::Conflict>),
    /// The branch does not take commits from this user: a new branch with a merge request.
    CannotPush,
    /// Signed in to GitLab to read only: committing needs signing in again with the api scope
    /// (`forge.sign_in {write: true}`).
    NeedsWrite,
}

/// Makes the transport a forge operation sends its requests with (on the operation's thread).
pub type Connect = std::sync::Arc<dyn Fn() -> Box<dyn texcraft_forge::Transport> + Send + Sync>;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "forge.open",
            label: "Open from GitLab or GitHub…",
            menu: &["File"],
            shortcut: None,
            params: open_schema,
            enabled: idle,
            run: open,
            changes: true,
        },
        CommandSpec {
            id: "forge.commit",
            label: "Commit to GitLab or GitHub…",
            menu: &["File"],
            shortcut: None,
            params: commit_schema,
            enabled: can_commit,
            run: commit,
            changes: true,
        },
        CommandSpec {
            id: "forge.pull",
            label: "Update from GitLab or GitHub",
            menu: &["File"],
            shortcut: None,
            params: pull_schema,
            enabled: can_commit,
            run: pull,
            changes: true,
        },
        CommandSpec {
            id: "forge.status",
            label: "GitLab or GitHub Status",
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
        "url": {"type": "string", "description": "The project's address: https://gitlab.com/group/project or https://github.com/owner/repository, a page under it, its clone address, or group/project (on gitlab.com, or github.com with service github)."},
        "service": {"enum": ["gitlab", "github"], "description": "The forge's kind, for an address that does not say it (github.com's are GitHub's, gitlab.com's GitLab's): a bare owner/repository, or a GitHub Enterprise Server. Default: gitlab."},
        "token": {"type": "string", "description": "A personal access token. GitLab: the read_api scope (api to commit). GitHub: a fine-grained token with Contents read-only (read and write to commit) and Pull requests read and write for new_branch, or a classic token with the repo scope. Kept for this session only. May be left out when one was given for the same instance in this session."},
        "ref": {"type": "string", "description": "The branch; the project's default branch if left out."},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_forge` (the window's status line). Default: wait for it (not in the browser, which always works so)."}
    }, "required": ["url"], "additionalProperties": false})
}

const RESOLVE: &str =
    "For files in conflict (forge.status lists them): \"mine\" keeps the project's version, \"theirs\" takes the branch's, by path.";

fn commit_schema() -> Value {
    json!({"type": "object", "properties": {
        "message": {"type": "string", "description": "The commit's message."},
        "token": {"type": "string", "description": "A personal access token that can commit (GitLab: the api scope; GitHub: Contents read and write); may be left out when one was given for the project's instance in this session."},
        "resolve": {"type": "object", "additionalProperties": {"enum": ["mine", "theirs"]}, "description": RESOLVE},
        "new_branch": {"type": "string", "description": "Commit to this new branch, from the base (no merge needed), and open a merge request (GitHub: a pull request) of it into the project's branch; the project then follows the new branch. For a protected branch, one the user cannot push to, or conflicts left to the forge."},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_forge`. Default: wait for it (not in the browser)."}
    }, "required": ["message"], "additionalProperties": false})
}

fn pull_schema() -> Value {
    json!({"type": "object", "properties": {
        "token": {"type": "string", "description": "A personal access token that can read the project (GitLab: read_api; GitHub: Contents read-only); may be left out when one was given for the project's instance in this session."},
        "resolve": {"type": "object", "additionalProperties": {"enum": ["mine", "theirs"]}, "description": RESOLVE},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_forge`. Default: wait for it (not in the browser)."}
    }, "additionalProperties": false})
}

/// The `resolve` parameter.
fn choices(p: &Value) -> Result<Choices> {
    let Some(map) = p.get("resolve") else { return Ok(Choices::new()) };
    let map = map.as_object().ok_or("resolve: give an object of paths and \"mine\" or \"theirs\".")?;
    map.iter()
        .map(|(path, c)| {
            let choice = c.as_str().and_then(Choice::parse).ok_or_else(|| format!("resolve: {path}: say \"mine\" or \"theirs\"."))?;
            Ok((path.clone(), choice))
        })
        .collect()
}

fn idle(s: &Session) -> Result<()> {
    match &s.forge_task {
        Some(_) => Err("The forge is still answering the last request: wait for it to end.".to_owned()),
        None => Ok(()),
    }
}

fn can_commit(s: &Session) -> Result<()> {
    if s.forge.is_none() {
        return Err("This project was not opened from GitLab or GitHub: use Open from GitLab or GitHub… first.".to_owned());
    }
    idle(s)
}

pub(super) fn background(p: &Value) -> bool {
    cfg!(target_arch = "wasm32") || p.get("background").and_then(Value::as_bool).unwrap_or(false)
}

/// The token given, the one given earlier in this session for `address`'s instance, or the
/// sign-in to it.
fn key_for(s: &Session, p: &Value, address: &Address) -> Result<Key> {
    key_at(s, p, &address.instance, address.service)
}

/// The token given, the one given earlier in this session for `instance`, or the sign-in to it.
pub(super) fn key_at(s: &Session, p: &Value, instance: &str, service: Service) -> Result<Key> {
    if let Some(t) = p.get("token").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
        return Token::new(t).map(Key::Token).map_err(|e| e.to_string());
    }
    if let Some(token) = s.forge_tokens.get(instance) {
        return Ok(Key::Token(token.clone()));
    }
    if let Some(grant) = s.forge_grant(instance, service) {
        return Ok(Key::Grant(Box::new(grant.clone())));
    }
    let name = service.name();
    Err(match s.forge_sign_in_unavailable_at(service, instance) {
        None if instance == oauth::instance(service) => format!(
            "Sign in to {name} (forge.sign_in), or give a personal access token for {instance}: TeXCraft keeps a token only while it runs, never on disk."
        ),
        None => format!(
            "Sign in to {instance} (forge.sign_in with its instance), or give a personal access token for it: TeXCraft keeps a token only while it runs, never on disk."
        ),
        _ => format!("Give a personal access token for {instance}: TeXCraft keeps it only while it runs, never on disk."),
    })
}

/// Runs the operation `make` returns: in the background (then `None`: [`Session::poll_forge`]
/// gives its outcome), or at once.
pub(super) fn start<T, F, Fut>(p: &Value, make: F) -> texcraft_forge::Result<std::result::Result<Pending<T>, T>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = texcraft_forge::Result<T>> + 'static,
    T: Send + 'static,
{
    let spawn = |make| texcraft_forge::task::spawn(make).map(Ok).map_err(|_| texcraft_forge::Error::Stopped);
    if background(p) {
        return spawn(make);
    }
    #[cfg(not(target_arch = "wasm32"))]
    return texcraft_forge::task::block_on(make()).map(Err);
    #[cfg(target_arch = "wasm32")]
    spawn(make)
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let service = match p.get("service").and_then(Value::as_str) {
        Some(s) => Some(Service::parse(s).ok_or_else(|| format!("service: say \"gitlab\" or \"github\", not {s:?}."))?),
        None => None,
    };
    let address = Address::parse_as(string(p, "url")?, service).map_err(|e| e.to_string())?;
    let key = key_for(s, p, &address)?;
    let branch = p.get("ref").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned);
    let (connect, path, sent, service_id) = (s.forge_transport.clone(), address.path.clone(), key.clone(), address.service.id());
    let (clients, slot) = (s.forge_auth_clients(), Renewed::default());
    let filled = slot.clone();
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let client = Client::new(connect(), address, token);
        texcraft_forge::open(&client, branch.as_deref()).await
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| e.to_string())? {
        Ok(pending) => {
            s.forge_task = Some(Task::Open { pending, key, renewed: slot });
            Ok(json!({"operation": "open", "started": true, "project": path, "service": service_id}))
        }
        Err(loaded) => opened(s, loaded, key),
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
fn unused_name(library: &crate::library::Library, name: &str, service: Service) -> String {
    let name = crate::library::check_name(name).unwrap_or_else(|_| format!("{} project", service.name()));
    (1..).map(|i| if i == 1 { name.clone() } else { format!("{name} ({i})") }).find(|n| !library.contains(n)).unwrap_or(name)
}

/// The project loaded: it becomes the session's, linked to its branch.
fn opened(s: &mut Session, loaded: Loaded, key: Key) -> Result<Value> {
    let Loaded { mut link, files } = loaded;
    let mut project = Project::new();
    let (review, files): (Vec<_>, Vec<_>) = files.into_iter().partition(|(path, _)| texcraft_forge::is_review(path));
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
    let name = s.library.as_ref().map(|library| unused_name(library, link.address.name(), link.address.service));
    s.replace_project(project, name.as_ref().map(PathBuf::from));
    s.load_forge_review(review);
    s.keep_key(&link.address.instance, key);
    let result = json!({
        "operation": "open",
        "service": link.address.service.id(),
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
    let link = s.forge.clone().ok_or("This project was not opened from GitLab or GitHub.")?;
    let message = string(p, "message")?.trim().to_owned();
    if message.is_empty() {
        return Err("Say what the commit changes: its message cannot be empty.".to_owned());
    }
    let key = key_for(s, p, &link.address)?;
    if let Key::Grant(grant) = &key
        && !grant.can_commit()
    {
        s.forge_problem = Some(Problem::NeedsWrite);
        return Err(format!(
            "TeXCraft is signed in to {} to read only: committing needs permission to write. Sign in again allowing commits (forge.sign_in with write), then commit.",
            grant.service.name()
        ));
    }
    let choices = choices(p)?;
    let new_branch = p.get("new_branch").and_then(Value::as_str).map(str::trim).map(str::to_owned);
    if let Some(name) = &new_branch {
        texcraft_forge::check_branch_name(name).map_err(|e| e.to_string())?;
    }
    // The review's quotes are renewed from the text, as when the project is saved.
    let files = s.forge_files(true)?;
    let (connect, sent, snapshot) = (s.forge_transport.clone(), key.clone(), files.clone());
    let (clients, slot) = (s.forge_auth_clients(), Renewed::default());
    let filled = slot.clone();
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let client = Client::new(connect(), link.address.clone(), token);
        match new_branch {
            Some(name) => texcraft_forge::commit_to_new_branch(&client, &link, &snapshot, &message, &name).await,
            None => texcraft_forge::commit(&client, &link, &snapshot, &message, &choices).await,
        }
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started {
        Ok(Ok(pending)) => {
            s.forge_task = Some(Task::Commit { pending, key, renewed: slot, files });
            Ok(json!({"operation": "commit", "started": true}))
        }
        Ok(Err(done)) => committed(s, done, key, &files),
        Err(e) => Err(s.forge_failed(e)),
    }
}

fn pull(s: &mut Session, p: &Value) -> Result<Value> {
    let link = s.forge.clone().ok_or("This project was not opened from GitLab or GitHub.")?;
    let key = key_for(s, p, &link.address)?;
    let choices = choices(p)?;
    let files = s.forge_files(false)?;
    let (connect, sent, snapshot) = (s.forge_transport.clone(), key.clone(), files.clone());
    let (clients, slot) = (s.forge_auth_clients(), Renewed::default());
    let filled = slot.clone();
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let client = Client::new(connect(), link.address.clone(), token);
        texcraft_forge::pull(&client, &link, &snapshot, &choices).await
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started {
        Ok(Ok(pending)) => {
            s.forge_task = Some(Task::Pull { pending, key, renewed: slot, files });
            Ok(json!({"operation": "pull", "started": true}))
        }
        Ok(Err(done)) => pulled(s, done, key, &files),
        Err(e) => Err(s.forge_failed(e)),
    }
}

/// What taking in a forge operation's updates did: files taken, removed, merged with edits made
/// while it ran, or kept as edited.
#[derive(Default)]
struct TakenIn {
    taken: Vec<String>,
    removed: Vec<String>,
    kept: Vec<String>,
}

/// Puts `updates` (computed from the project's files as they were, `snapshot`) into the project
/// and its review. A file edited since the snapshot gets the update merged into the edits, or,
/// when they do not merge, keeps the edits (`kept`).
fn take_in(s: &mut Session, updates: &[Update], snapshot: &[(String, Vec<u8>)]) -> Result<TakenIn> {
    let then: std::collections::BTreeMap<&str, &[u8]> = snapshot.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let now: std::collections::BTreeMap<String, Vec<u8>> = s.forge_files(false)?.into_iter().collect();
    let mut out = TakenIn::default();
    let mut touched = false;
    for (path, new) in updates {
        let (was, is) = (then.get(path.as_str()).copied(), now.get(path).map(Vec::as_slice));
        let put: Option<Vec<u8>> = match (was == is, was, is, new) {
            (true, ..) => new.clone(),
            (false, _, is, new) if is == new.as_deref() => continue,
            (false, Some(was), Some(is), Some(new)) => match texcraft_forge::merge::merge3(was, is, new) {
                Some(merged) => Some(merged),
                None => {
                    out.kept.push(path.clone());
                    continue;
                }
            },
            _ => {
                out.kept.push(path.clone());
                continue;
            }
        };
        let done = match &put {
            Some(bytes) => s.put_forge_file(path, bytes.clone()),
            None => s.remove_forge_file(path),
        };
        match done {
            Ok(()) if put.is_some() => out.taken.push(path.clone()),
            Ok(()) => out.removed.push(path.clone()),
            Err(e) => out.kept.push(format!("{path} ({e})")),
        }
        touched |= !texcraft_forge::is_review(path);
    }
    if touched {
        s.touch();
        if s.current.as_ref().is_some_and(|c| s.project.get(c).is_none()) {
            s.current = s.project.main().cloned();
        }
        s.open.retain(|p| s.project.get(p).is_some());
    }
    Ok(out)
}

/// The link moved: the browser keeps the project with its new base, so a reload counts changes
/// from it; a project in memory only has nothing unsaved once the branch has it all.
fn keep_link(s: &mut Session, link: Link) -> Result<()> {
    s.set_forge(Some(link));
    match s.dir.clone() {
        Some(dir) if s.library.is_some() => {
            super::project::save_to(s, &dir)?;
        }
        None => s.dirty = !s.forge_changes()?.is_empty(),
        Some(_) => {}
    }
    Ok(())
}

/// The branch's new commits taken in.
fn pulled(s: &mut Session, done: Pulled, key: Key, files: &[(String, Vec<u8>)]) -> Result<Value> {
    s.keep_key(&done.link.address.instance, key);
    let TakenIn { taken, removed, kept } = take_in(s, &done.updates, files)?;
    let result = json!({
        "operation": "pull",
        "moved": done.moved,
        "branch": done.link.branch,
        "commit": done.link.base,
        "taken": taken,
        "removed": removed,
        "merged": done.merged,
        "kept": kept,
    });
    keep_link(s, done.link)?;
    Ok(result)
}

/// The commit made: the link moves to it, and what others committed meanwhile is taken in.
fn committed(s: &mut Session, done: Committed, key: Key, files: &[(String, Vec<u8>)]) -> Result<Value> {
    s.keep_key(&done.link.address.instance, key);
    let TakenIn { taken, removed, kept } = take_in(s, &done.updates, files)?;
    let web_url = done.link.web_url.clone();
    let changes: Vec<Value> = done.changes.iter().map(change_json).collect();
    let result = json!({
        "operation": "commit",
        "service": done.link.address.service.id(),
        "commit": done.commit,
        "web_url": done.commit.as_ref().map(|id| done.link.address.commit_url(&web_url, id)),
        "branch": done.link.branch,
        "changes": changes,
        "taken": taken,
        "removed": removed,
        "merged": done.merged,
        "kept": kept,
        "raced": done.raced,
        "merge_request": done.merge_request.as_ref().map(|mr| json!({"iid": mr.iid, "web_url": mr.web_url})),
        "merge_request_error": done.merge_request_error,
    });
    keep_link(s, done.link)?;
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
        "service": link.address.service.id(),
        "instance": link.address.instance,
        "project": link.address.path,
        "web_url": link.web_url,
        "branch": link.branch,
        "base": link.base,
        "changes": changes,
        "skipped": skipped_json(&link.skipped),
        "token": s.has_forge_token(),
        "signed_in": s.forge_grant(&link.address.instance, link.address.service).map(|g| json!({
            "name": g.user.as_ref().map(|u| u.name.clone()),
            "username": g.user.as_ref().map(|u| u.username.clone()),
            "can_commit": g.can_commit(),
        })),
        "running": running,
        "conflicts": s.forge_conflicts().iter().map(|c| json!({"path": c.path, "why": c.kind.as_str()})).collect::<Vec<_>>(),
        "cannot_push": s.forge_problem == Some(Problem::CannotPush),
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
        self.forge_problem = None;
    }

    /// The forge operation running in the background: `open`, `pull` or `commit`.
    pub fn forge_busy(&self) -> Option<&'static str> {
        self.forge_task.as_ref().map(|t| match t {
            Task::Open { .. } => "open",
            Task::Commit { .. } => "commit",
            Task::Pull { .. } => "pull",
        })
    }

    /// Why the last pull or commit was refused, when there is a way out: files in conflict, or a
    /// branch that does not take this user's commits. Cleared by the next one that succeeds.
    pub fn forge_problem(&self) -> Option<&Problem> {
        self.forge_problem.as_ref()
    }

    /// The files in conflict with the branch's new commits, as the last pull or commit found them.
    pub fn forge_conflicts(&self) -> &[texcraft_forge::Conflict] {
        match &self.forge_problem {
            Some(Problem::Conflicts(c)) => c,
            _ => &[],
        }
    }

    /// Keeps what the interface offers for `e` and puts it in words.
    fn forge_failed(&mut self, e: texcraft_forge::Error) -> String {
        match &e {
            texcraft_forge::Error::Conflict { files, .. } => self.forge_problem = Some(Problem::Conflicts(files.clone())),
            texcraft_forge::Error::CannotPush { .. } => self.forge_problem = Some(Problem::CannotPush),
            _ => {}
        }
        e.to_string()
    }

    /// The project's files and its review threads' files (in `.texcraft/review/`), as the forge
    /// has them; with `requote`, the threads' quotes renewed from the text first (a commit).
    pub(crate) fn forge_files(&mut self, requote: bool) -> Result<Vec<(String, Vec<u8>)>> {
        let mut files: Vec<(String, Vec<u8>)> = self.project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect();
        let review = if requote { self.review.files(&self.project)? } else { self.review.texts()? };
        files.extend(review.into_iter().map(|(name, text)| (format!("{}/{name}", texcraft_forge::REVIEW_DIR), text.into_bytes())));
        Ok(files)
    }

    /// The review threads loaded from a forge branch (paths in `.texcraft/review/` and bytes).
    fn load_forge_review(&mut self, files: Vec<(String, Vec<u8>)>) {
        let files = files.into_iter().map(|(path, bytes)| {
            let name = path.rsplit('/').next().unwrap_or_default().to_owned();
            (name, String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned()))
        });
        self.review = crate::review_store::State::read(files, &self.project);
    }

    /// Puts a file from the forge into the project, or into the review when it is a thread's.
    fn put_forge_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        if texcraft_forge::is_review(path) {
            let text = String::from_utf8(bytes).map_err(|_| "a review thread that is not UTF-8 text".to_owned())?;
            let id = self.review.put(&text, &self.project)?;
            return self.review_changed(id);
        }
        let pp = ProjectPath::new(path).map_err(|e| e.to_string())?;
        let contents = Contents::from_bytes(bytes);
        let put = if self.project.get(&pp).is_some() { self.project.set(&pp, contents) } else { self.project.add(pp, contents) };
        put.map_err(|e| e.to_string())
    }

    /// Removes a file the forge's branch deleted from the project, or from the review.
    fn remove_forge_file(&mut self, path: &str) -> Result<()> {
        if texcraft_forge::is_review(path) {
            let name = path.rsplit('/').next().unwrap_or_default();
            return match self.review.drop_file(name) {
                Some(id) => self.review_changed(id),
                None => Ok(()),
            };
        }
        let pp = ProjectPath::new(path).map_err(|e| e.to_string())?;
        self.project.remove(&pp).map(|_| ()).map_err(|e| e.to_string())
    }

    /// Whether a token for the linked project's instance was given in this session, or the
    /// session is signed in to it.
    pub fn has_forge_token(&self) -> bool {
        self.forge.as_ref().is_some_and(|l| {
            self.forge_tokens.contains_key(&l.address.instance) || self.forge_grant(&l.address.instance, l.address.service).is_some()
        })
    }

    /// Whether the session reaches the linked project with a sign-in that can only read (GitLab's
    /// read_api): committing then asks to sign in again, allowing commits.
    pub fn forge_needs_write(&self) -> bool {
        self.forge.as_ref().is_some_and(|l| {
            !self.forge_tokens.contains_key(&l.address.instance)
                && self.forge_grant(&l.address.instance, l.address.service).is_some_and(|g| !g.can_commit())
        })
    }

    /// The applications this session signs in as.
    pub(super) fn forge_auth_clients(&self) -> Clients {
        self.forge_auth_clients_ref().clone()
    }

    /// A personal access token given is kept for the session; a sign-in is kept already.
    pub(super) fn keep_key(&mut self, instance: &str, key: Key) {
        if let Key::Token(token) = key {
            self.forge_tokens.insert(instance.to_owned(), token);
        }
    }

    /// Keeps the sign-in an operation renewed, if it did.
    pub(super) fn keep_renewed(&mut self, slot: &Renewed) {
        let renewed = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(grant) = renewed
            && let Some(warning) = self.keep_grant(grant)
        {
            log::warn!("{warning}");
        }
    }

    /// The project's files changed since the forge branch's base, sorted by path; none for a
    /// project with no link. Kept until the files or the link change.
    pub fn forge_changes(&mut self) -> Result<Vec<texcraft_forge::FileChange>> {
        let Some(link) = &self.forge else { return Ok(Vec::new()) };
        let at = (self.revision, self.review.revision);
        if let Some((revision, changes)) = &self.forge_changes
            && *revision == at
        {
            return Ok(changes.clone());
        }
        let link = link.clone();
        let files = self.forge_files(false)?;
        let changes = texcraft_forge::changes(&link, &files).map_err(|e| e.to_string())?;
        self.forge_changes = Some((at, changes.clone()));
        Ok(changes)
    }

    /// The outcome of the forge operation running in the background, once it has ended: what
    /// `forge.open`, `forge.pull` or `forge.commit` returns when it waits (with `operation`), or
    /// why it failed.
    pub fn poll_forge(&mut self) -> Option<Result<Value>> {
        if let Some(done) = self.poll_forge_auth() {
            return Some(done);
        }
        let outcome = match self.forge_task.as_mut()? {
            Task::Open { pending, .. } => pending.poll()?.map(Ended::Opened),
            Task::Commit { pending, .. } => pending.poll()?.map(Ended::Committed),
            Task::Pull { pending, .. } => pending.poll()?.map(Ended::Pulled),
        };
        let task = self.forge_task.take()?;
        let (Task::Open { renewed, .. } | Task::Commit { renewed, .. } | Task::Pull { renewed, .. }) = &task;
        let renewed = renewed.clone();
        self.keep_renewed(&renewed);
        Some(match (outcome, task) {
            (Ok(Ended::Opened(loaded)), Task::Open { key, .. }) => opened(self, loaded, key),
            (Ok(Ended::Committed(done)), Task::Commit { key, files, .. }) => committed(self, done, key, &files),
            (Ok(Ended::Pulled(done)), Task::Pull { key, files, .. }) => pulled(self, done, key, &files),
            (Err(e), _) => Err(self.forge_failed(e)),
            _ => Err(texcraft_forge::Error::Stopped.to_string()),
        })
    }
}

/// A background operation's outcome.
enum Ended {
    Opened(Loaded),
    Committed(Committed),
    Pulled(Pulled),
}
