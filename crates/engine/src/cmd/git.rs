//! Push and pull for project folders (docs/forges.md, step 5): the project's git repository (C8)
//! and its remotes, over git's smart HTTP protocol (`texcraft-gitwire`), on the desktop. And a new
//! repository on GitLab or GitHub for a project that has none (Save Project to GitLab or GitHub):
//! made, set as the folder's `origin`, and the whole history pushed to it; in the browser, where
//! no git protocol reaches the forges, the files are its first commit and the project is linked
//! to it as one opened from it.
//!
//! The network is reached on an operation's own thread: with `background` the commands return
//! at once and [`Session::poll_forge`] gives the outcome; without it they wait. Credentials are
//! the sign-in or the token given for the remote's server, as for the forges' API, and go into
//! the git requests' basic authentication only.

use serde_json::{Value, json};
use texcraft_forge::create::{Created, Creator, NewRepository, Owner};
use texcraft_forge::task::Pending;
use texcraft_forge::{Link, Service};

use super::forge::{Key, Renewed, key_at, start};
use super::{CommandSpec, string};
use crate::{Result, Session};

/// The message of the commit made of changes not committed before a push to a new repository.
pub const SAVE_MESSAGE: &str = "Saved from TeXCraft";

/// A git or repository operation running in the background.
pub(crate) enum Task {
    #[cfg(not(target_arch = "wasm32"))]
    Push { pending: Pending<std::result::Result<texcraft_gitwire::Pushed, String>>, remote: String, branch: String, renewed: Renewed },
    #[cfg(not(target_arch = "wasm32"))]
    Pull { pending: Pending<std::result::Result<texcraft_gitwire::Fetched, String>>, remote: String, branch: String, renewed: Renewed },
    /// A repository made (and, on the desktop, the history pushed to it).
    Create { pending: Pending<Made>, renewed: Renewed, key: Key },
    /// Where the account can make repositories.
    Owners { pending: Pending<Vec<Owner>>, renewed: Renewed, key: Key, instance: String },
}

/// What making a repository did.
pub(crate) struct Made {
    created: Created,
    /// The desktop: the push of the history, or why it failed.
    #[cfg(not(target_arch = "wasm32"))]
    pushed: Option<std::result::Result<texcraft_gitwire::Pushed, String>>,
    /// The browser: the link to the first commit.
    linked: Option<Link>,
    branch: String,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "git.remotes",
            label: "Git Remotes",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: super::always,
            run: remotes,
            changes: false,
        },
        CommandSpec {
            id: "git.set_remote",
            label: "Set a Git Remote",
            menu: &[],
            shortcut: None,
            params: set_remote_schema,
            enabled: has_repository,
            run: set_remote,
            changes: true,
        },
        CommandSpec {
            id: "git.push",
            label: "Push",
            menu: &["File"],
            shortcut: None,
            params: transfer_schema,
            enabled: can_transfer,
            run: push,
            changes: true,
        },
        CommandSpec {
            id: "git.pull",
            label: "Pull",
            menu: &["File"],
            shortcut: None,
            params: transfer_schema,
            enabled: can_transfer,
            run: pull,
            changes: true,
        },
        CommandSpec {
            id: "forge.owners",
            label: "Where a Repository Can Be Made",
            menu: &[],
            shortcut: None,
            params: owners_schema,
            enabled: idle,
            run: owners,
            changes: false,
        },
        CommandSpec {
            id: "forge.create_repository",
            label: "Save Project to GitLab or GitHub…",
            menu: &["File"],
            shortcut: None,
            params: create_schema,
            enabled: can_create,
            run: create,
            changes: true,
        },
    ]
}

const BACKGROUND: &str = "Return at once; the outcome comes from `Session::poll_forge`. Default: wait for it (not in the browser).";
const TOKEN: &str = "A personal access token for the server (GitLab: the api scope to push, read_api or read_repository to pull; GitHub: Contents read and write); may be left out when one was given or a sign-in made for the server in this session. Kept for this session only.";

fn set_remote_schema() -> Value {
    json!({"type": "object", "properties": {
        "name": {"type": "string", "description": "The remote's name: origin, most often."},
        "url": {"type": "string", "description": "Its address: https://host/owner/name.git (an SSH address of the same server is reached over HTTPS)."}
    }, "required": ["name", "url"], "additionalProperties": false})
}

fn transfer_schema() -> Value {
    json!({"type": "object", "properties": {
        "remote": {"type": "string", "description": "The remote: the branch's upstream remote, else origin."},
        "branch": {"type": "string", "description": "The branch on the remote: the upstream's, else the branch checked out."},
        "token": {"type": "string", "description": TOKEN},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "additionalProperties": false})
}

fn owners_schema() -> Value {
    json!({"type": "object", "properties": {
        "service": {"enum": ["gitlab", "github"]},
        "instance": {"type": "string", "description": "The server: https://gitlab.com or https://github.com by default, or a server added."},
        "token": {"type": "string", "description": TOKEN},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "required": ["service"], "additionalProperties": false})
}

fn create_schema() -> Value {
    json!({"type": "object", "properties": {
        "service": {"enum": ["gitlab", "github"]},
        "instance": {"type": "string", "description": "The server: https://gitlab.com or https://github.com by default, or a server added."},
        "owner": {"type": "string", "description": "A GitHub organisation or a GitLab group (its path) to make it in, from forge.owners; the account itself when left out."},
        "name": {"type": "string", "description": "The repository's name: letters, digits, '.', '-' and '_'."},
        "private": {"type": "boolean", "description": "Only those given access see it (the default)."},
        "description": {"type": "string"},
        "push": {"type": "boolean", "description": "Desktop: set it as the folder's origin and push the whole history (the default; changes not committed are committed first). In the browser the files are always its first commit."},
        "message": {"type": "string", "description": "The message of the commit of changes not committed yet (\"Saved from TeXCraft\" by default)."},
        "token": {"type": "string", "description": "A personal access token that can make repositories (GitLab: api; GitHub: a classic token with repo, or a fine-grained one with Administration read and write)."},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "required": ["service", "name"], "additionalProperties": false})
}

fn idle(s: &Session) -> Result<()> {
    if s.git_task.is_some() {
        return Err("A push, a pull or a new repository is still under way: wait for it to end.".to_owned());
    }
    match s.forge_busy() {
        Some(_) => Err("The forge is still answering the last request: wait for it to end.".to_owned()),
        None => Ok(()),
    }
}

fn has_repository(s: &Session) -> Result<()> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Err("Git's remotes, push and pull are for project folders on the desktop: in the browser, use Save Project to GitLab or GitHub.".to_owned())
    }
    #[cfg(not(target_arch = "wasm32"))]
    match &s.history {
        Some(_) => Ok(()),
        None => Err("The project has no git repository yet: save it in a folder first.".to_owned()),
    }
}

fn can_transfer(s: &Session) -> Result<()> {
    has_repository(s)?;
    idle(s)
}

fn can_create(s: &Session) -> Result<()> {
    idle(s)?;
    if s.forge.is_some() {
        return Err("This project is on a forge already: commit to it instead.".to_owned());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if s.library.is_none() && s.dir.is_none() {
        return Err("Save the project in a folder first: its history is what goes to the new repository.".to_owned());
    }
    Ok(())
}

/// The `service` parameter.
fn service(p: &Value) -> Result<Service> {
    let id = string(p, "service")?;
    Service::parse(id).ok_or_else(|| format!("service: say \"gitlab\" or \"github\", not {id:?}."))
}

/// The `instance` parameter, or the service's public one: scheme and host, no `/` at the end.
fn instance(p: &Value, service: Service) -> Result<String> {
    match p.get("instance").and_then(Value::as_str).map(str::trim).filter(|i| !i.is_empty()) {
        None => Ok(texcraft_forge::oauth::instance(service).to_owned()),
        Some(i) => {
            let i = i.trim_end_matches('/');
            let host =
                i.strip_prefix("https://").or_else(|| i.strip_prefix("http://").filter(|h| h.starts_with("localhost") || h.starts_with("127.0.0.1")));
            match host {
                Some(h) if !h.is_empty() && !h.contains('/') => Ok(i.to_owned()),
                _ => Err(format!("instance: {i} is not a server's address (https://host).")),
            }
        }
    }
}

/// The key for a write: a sign-in that can only read is refused, with the way out.
fn write_key(s: &mut Session, p: &Value, instance: &str, service: Service) -> Result<Key> {
    let key = key_at(s, p, instance, service)?;
    if let Key::Grant(grant) = &key
        && !grant.can_commit()
    {
        s.forge_problem = Some(super::forge::Problem::NeedsWrite);
        return Err(format!(
            "TeXCraft is signed in to {} to read only: this needs permission to write. Sign in again allowing commits (forge.sign_in with write), then try again.",
            grant.service.name()
        ));
    }
    Ok(key)
}

fn owners(s: &mut Session, p: &Value) -> Result<Value> {
    let service = service(p)?;
    let instance = instance(p, service)?;
    let key = key_at(s, p, &instance, service)?;
    let (connect, sent, clients, slot) = (s.forge_transport.clone(), key.clone(), s.forge_auth_clients(), Renewed::default());
    let (filled, at) = (slot.clone(), instance.clone());
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        Creator::new(connect(), service, &at, token).owners().await
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| e.to_string())? {
        Ok(pending) => {
            s.git_task = Some(Task::Owners { pending, renewed: slot, key, instance });
            Ok(json!({"operation": "owners", "started": true}))
        }
        Err(found) => listed_owners(s, found, key, &instance),
    }
}

fn listed_owners(s: &mut Session, owners: Vec<Owner>, key: Key, instance: &str) -> Result<Value> {
    s.keep_key(instance, key);
    let list: Vec<Value> = owners.iter().map(|o| json!({"path": o.path, "personal": o.personal})).collect();
    Ok(json!({"operation": "owners", "owners": list}))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let service = service(p)?;
    let instance = instance(p, service)?;
    let name = string(p, "name")?.trim().to_owned();
    texcraft_forge::create::check_name(&name).map_err(|e| e.to_string())?;
    let owner = p.get("owner").and_then(Value::as_str).map(str::trim).filter(|o| !o.is_empty()).map(str::to_owned);
    let private = p.get("private").and_then(Value::as_bool).unwrap_or(true);
    let description = p.get("description").and_then(Value::as_str).map(str::to_owned);
    let key = write_key(s, p, &instance, service)?;
    let browser = s.library.is_some();
    let new = NewRepository { owner: None, name, private, description, initialize: browser && service == Service::GitHub };
    let (connect, sent, clients, slot) = (s.forge_transport.clone(), key.clone(), s.forge_auth_clients(), Renewed::default());
    let filled = slot.clone();
    let task = if browser { Prepared::Browser(s.forge_files(true)?) } else { prepare_push(s, p, service)? };
    let message = p.get("message").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()).unwrap_or(SAVE_MESSAGE).to_owned();
    #[cfg(not(target_arch = "wasm32"))]
    let http = s.git_http.clone();
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let creator = Creator::new(connect(), service, &instance, token.clone());
        let mut new = new;
        if let Some(path) = owner {
            let owners = creator.owners().await?;
            let found = owners.into_iter().find(|o| o.path.eq_ignore_ascii_case(&path));
            new.owner = Some(found.ok_or_else(|| {
                texcraft_forge::Error::Name(format!("The account cannot make repositories in {path}, or has no such organisation or group."))
            })?);
        }
        let created = creator.create(&new).await?;
        match task {
            Prepared::Browser(files) => {
                let branch = created.default_branch.clone();
                let link = Creator::new(connect(), service, &instance, token).first_commit(&created, &files, &message).await?;
                #[cfg(not(target_arch = "wasm32"))]
                return Ok(Made { created, pushed: None, linked: Some(link), branch });
                #[cfg(target_arch = "wasm32")]
                Ok(Made { created, linked: Some(link), branch })
            }
            #[cfg(not(target_arch = "wasm32"))]
            Prepared::Push { git_dir, head, branch } => {
                let pushed = match head {
                    Some(head) => {
                        let remote = texcraft_gitwire::Remote::new(&created.clone_url, Some(credentials(service, &token)));
                        let reference = format!("refs/heads/{branch}");
                        Some(texcraft_gitwire::push(&*http(), &remote, &git_dir, &head, &reference, false).map_err(|e| e.to_string()))
                    }
                    None => None,
                };
                Ok(Made { created, pushed, linked: None, branch })
            }
            #[cfg(not(target_arch = "wasm32"))]
            Prepared::NoPush { branch } => Ok(Made { created, pushed: None, linked: None, branch }),
        }
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| s.forge_failed(e))? {
        Ok(pending) => {
            s.git_task = Some(Task::Create { pending, renewed: slot, key });
            Ok(json!({"operation": "create_repository", "started": true}))
        }
        Err(made) => created(s, made, key),
    }
}

/// What a new repository's thread needs from the session.
enum Prepared {
    /// The browser: the files to commit.
    Browser(Vec<(String, Vec<u8>)>),
    /// The desktop: the repository and the commit to push to `branch`.
    #[cfg(not(target_arch = "wasm32"))]
    Push { git_dir: std::path::PathBuf, head: Option<String>, branch: String },
    #[cfg(not(target_arch = "wasm32"))]
    NoPush { branch: String },
}

#[cfg(target_arch = "wasm32")]
fn prepare_push(_: &mut Session, _: &Value, _: Service) -> Result<Prepared> {
    Err("Save the project first.".to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn prepare_push(s: &mut Session, p: &Value, _service: Service) -> Result<Prepared> {
    desktop::prepare_push(s, p)
}

/// The new repository's outcome: on the desktop set as `origin`, the branch following it once
/// pushed; in the browser the project linked to it.
fn created(s: &mut Session, made: Made, key: Key) -> Result<Value> {
    let Made { created, linked, branch, .. } = &made;
    s.keep_key(&created.address.instance, key);
    let mut result = json!({
        "operation": "create_repository",
        "service": created.address.service.id(),
        "instance": created.address.instance,
        "project": created.address.path,
        "web_url": created.web_url,
        "clone_url": created.clone_url,
        "branch": branch,
    });
    if let Some(link) = linked.clone() {
        result["linked"] = json!(true);
        result["commit"] = json!(link.base);
        result["note"] = json!("The files went as one commit: the project's history stays in this browser.");
        s.set_forge(Some(link));
        if let Some(dir) = s.dir.clone() {
            super::project::save_to(s, &dir)?;
        }
        return Ok(result);
    }
    #[cfg(not(target_arch = "wasm32"))]
    desktop::created(s, &made, &mut result)?;
    Ok(result)
}

/// The credentials git over HTTPS takes for a forge's token: GitLab's `oauth2` user (any name
/// works for its personal access tokens), GitHub's `x-access-token`.
#[cfg(not(target_arch = "wasm32"))]
fn credentials(service: Service, token: &texcraft_forge::Token) -> texcraft_gitwire::Credentials {
    let user = match service {
        Service::GitLab => "oauth2",
        Service::GitHub => "x-access-token",
    };
    texcraft_gitwire::Credentials::new(user, token.secret())
}

fn remotes(s: &mut Session, _: &Value) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::remotes(s);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Ok(
            json!({"repository": false, "remotes": [], "why": "In the browser a project has no git repository: Save Project to GitLab or GitHub links it to one."}),
        )
    }
}

fn set_remote(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::set_remote(s, p);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = p;
        has_repository(s).map(|()| Value::Null)
    }
}

fn push(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::push(s, p);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = p;
        has_repository(s).map(|()| Value::Null)
    }
}

fn pull(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::pull(s, p);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = p;
        has_repository(s).map(|()| Value::Null)
    }
}

impl Session {
    /// The push, pull or new repository under way: `push`, `pull`, `create_repository` or `owners`.
    pub fn git_busy(&self) -> Option<&'static str> {
        self.git_task.as_ref().map(|t| match t {
            #[cfg(not(target_arch = "wasm32"))]
            Task::Push { .. } => "push",
            #[cfg(not(target_arch = "wasm32"))]
            Task::Pull { .. } => "pull",
            Task::Create { .. } => "create_repository",
            Task::Owners { .. } => "owners",
        })
    }

    /// The outcome of the push, pull or new repository running in the background, once it has
    /// ended (as [`Session::poll_forge`] gives it).
    pub(crate) fn poll_git(&mut self) -> Option<Result<Value>> {
        let ended = match self.git_task.as_mut()? {
            #[cfg(not(target_arch = "wasm32"))]
            Task::Push { pending, .. } => Ended::Pushed(pending.poll()?),
            #[cfg(not(target_arch = "wasm32"))]
            Task::Pull { pending, .. } => Ended::Fetched(pending.poll()?),
            Task::Create { pending, .. } => Ended::Made(pending.poll()?.map(Box::new)),
            Task::Owners { pending, .. } => Ended::Owners(pending.poll()?),
        };
        let task = self.git_task.take()?;
        let renewed = match &task {
            #[cfg(not(target_arch = "wasm32"))]
            Task::Push { renewed, .. } | Task::Pull { renewed, .. } => renewed.clone(),
            Task::Create { renewed, .. } | Task::Owners { renewed, .. } => renewed.clone(),
        };
        self.keep_renewed(&renewed);
        Some(match (ended, task) {
            #[cfg(not(target_arch = "wasm32"))]
            (Ended::Pushed(done), Task::Push { remote, branch, .. }) => {
                done.map_err(|e| self.forge_failed(e)).and_then(|pushed| desktop::pushed(self, &remote, &branch, pushed))
            }
            #[cfg(not(target_arch = "wasm32"))]
            (Ended::Fetched(done), Task::Pull { remote, branch, .. }) => {
                done.map_err(|e| self.forge_failed(e)).and_then(|fetched| desktop::fetched(self, &remote, &branch, fetched))
            }
            (Ended::Made(done), Task::Create { key, .. }) => done.map_err(|e| self.forge_failed(e)).and_then(|made| created(self, *made, key)),
            (Ended::Owners(done), Task::Owners { key, instance, .. }) => {
                done.map_err(|e| self.forge_failed(e)).and_then(|owners| listed_owners(self, owners, key, &instance))
            }
            _ => Err(texcraft_forge::Error::Stopped.to_string()),
        })
    }
}

/// A background operation's outcome.
enum Ended {
    #[cfg(not(target_arch = "wasm32"))]
    Pushed(texcraft_forge::Result<std::result::Result<texcraft_gitwire::Pushed, String>>),
    #[cfg(not(target_arch = "wasm32"))]
    Fetched(texcraft_forge::Result<std::result::Result<texcraft_gitwire::Fetched, String>>),
    Made(texcraft_forge::Result<Box<Made>>),
    Owners(texcraft_forge::Result<Vec<Owner>>),
}

#[cfg(not(target_arch = "wasm32"))]
mod desktop;
#[cfg(not(target_arch = "wasm32"))]
pub use desktop::{GitHttp, default_http};
