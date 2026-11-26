//! The desktop's check and clone: git's protocol on the operation's own thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use texcraft_forge::task::Pending;
use texcraft_forge::{Service, Token};
use texcraft_gitwire::{Credentials, Error as WireError, Http, Remote, Reply};
use texcraft_history::History;

use super::super::forge::{Key, Renewed, key_at, start};
use super::super::git::desktop::{https_url, server_of};
use super::super::git::{hosting, remote_credentials};
use super::{ClonePhase, ImportProblem, folder_free, is_ssh, repository_name};
use crate::file_access::Access;
use crate::{Result, Session};

/// A check or a clone running in the background.
pub(crate) enum Task {
    Check { pending: Pending<Checked>, renewed: Renewed, key: Option<Key>, sent: bool, https: String, instance: String, kind: &'static str },
    Clone(Box<Cloning>),
}

impl Task {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Task::Check { .. } => "check_address",
            Task::Clone(_) => "clone",
        }
    }

    pub(super) fn phase(&self) -> Option<ClonePhase> {
        match self {
            Task::Clone(c) => Some(*c.phase.lock().unwrap_or_else(PoisonError::into_inner)),
            Task::Check { .. } => None,
        }
    }
}

/// A clone under way, and what its end needs.
pub(crate) struct Cloning {
    pending: Pending<std::result::Result<Cloned, Failed>>,
    renewed: Renewed,
    stop: Arc<AtomicBool>,
    phase: Arc<Mutex<ClonePhase>>,
    target: Target,
}

/// Where a clone goes, and the credential it was given.
struct Target {
    folder: PathBuf,
    /// TeXCraft made the folder (else it was there, empty).
    made: bool,
    https: String,
    instance: String,
    key: Option<Key>,
    remember: bool,
    open: bool,
}

/// The server's answer to a check: its refs (or why there are none).
pub(crate) type Checked = std::result::Result<texcraft_gitwire::Advertised, WireError>;

/// What a clone did.
pub(crate) struct Cloned {
    branch: String,
    tip: String,
}

/// Why a clone failed, in words, and what kind of failure it is.
pub(crate) struct Failed {
    problem: ImportProblem,
    message: String,
}

/// The kind of server a repository's HTTPS address is on, as the dialog names it: `gitlab`
/// (gitlab.com and the GitLab servers added), `github`, `overleaf`, or `other`.
fn host_kind(s: &Session, https: &str) -> &'static str {
    if super::super::git::hosting(https).is_some() {
        return "overleaf";
    }
    let host = super::super::git::remote_host(https).unwrap_or_default();
    match host.as_str() {
        "github.com" | "www.github.com" => "github",
        "gitlab.com" => "gitlab",
        _ if s.forge_servers().iter().any(|v| v.service == Service::GitLab && v.host().eq_ignore_ascii_case(&host)) => "gitlab",
        _ => "other",
    }
}

/// The `token` parameter, checked.
fn given_token(p: &Value) -> Result<Option<Token>> {
    match p.get("token").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => Token::new(t).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// The `username` parameter, checked: it goes into the remote's address.
fn given_user(p: &Value) -> Result<Option<String>> {
    match p.get("username").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) if u.len() <= 128 && u.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '@' | ':' | '/' | '?' | '#')) => Ok(Some(u.to_owned())),
        Some(_) => Err("username: a user name has no spaces, @, :, / or ?.".to_owned()),
        None => Ok(None),
    }
}

/// The credential known for `instance` without one given: a token given earlier in this session,
/// one remembered, or (not for a hosting service's git access) the sign-in to the forge.
fn known_key(s: &Session, instance: &str, service: Service, hosted: bool) -> Option<Key> {
    let host = texcraft_forge::servers::host_of(instance);
    if hosted {
        return s.forge_tokens.get(instance).cloned().or_else(|| s.kept_git_token(host)).map(Key::Token);
    }
    key_at(s, &json!({}), instance, service).ok().or_else(|| s.kept_git_token(host).map(Key::Token))
}

/// The credentials git's requests carry: the user name given, else the address's, the hosting
/// service's or the forge's.
fn credentials(user: Option<&str>, url: &str, service: Service, token: &Token) -> Credentials {
    match user {
        Some(u) => Credentials::new(u, token.secret()),
        None => remote_credentials(url, service, token),
    }
}

/// The repository's HTTPS address from what was typed: an SSH one is refused with its HTTPS form,
/// an address with no repository's path too.
fn repository_url(typed: &str) -> Result<String> {
    let https = https_url(typed)?;
    if is_ssh(typed) {
        return Err(format!("TeXCraft uses git over HTTPS: use {https}."));
    }
    let path = https.split_once("://").and_then(|(_, rest)| rest.split_once('/')).map_or("", |(_, p)| p);
    if path.trim_matches('/').is_empty() {
        return Err("The address names a server but no repository: copy the whole address from the project's page.".to_owned());
    }
    Ok(https)
}

/// A refusal or a failure of the server, in the words the dialog shows (design: Errors).
fn words(e: &WireError, host: &str, https: &str, sent: bool) -> (ImportProblem, String) {
    let project = https.split_once("://").and_then(|(_, r)| r.split_once('/')).map_or(https, |(_, p)| p).trim_end_matches(".git");
    match e {
        WireError::Unauthorized if sent => (
            ImportProblem::Unauthorized,
            format!("{host} did not accept the token. Check it is a token, not your password, and that it has not expired."),
        ),
        WireError::Unauthorized => {
            (ImportProblem::NeedsSignIn, "This repository needs signing in: give a user name and a password or token.".to_owned())
        }
        WireError::Forbidden => (ImportProblem::Forbidden, format!("This account cannot read {project}: it needs read access to the repository.")),
        WireError::NotFound | WireError::Protocol(_) => {
            (ImportProblem::NotFound, "No git repository at this address. Copy the address again from the project's page.".to_owned())
        }
        WireError::Network(_) => (ImportProblem::Network, format!("Could not reach {host}. Check the connection, then try again.")),
        other => (ImportProblem::Other, other.to_string()),
    }
}

pub(super) fn check(s: &mut Session, p: &Value, typed: &str) -> Result<Value> {
    s.import.problem = None;
    if is_ssh(typed) {
        let https = https_url(typed)?;
        return Ok(json!({
            "operation": "check_address", "reachable": false, "ssh": true, "https_url": https, "needs_sign_in": false,
            "branches": [], "host": host_kind(s, &https), "name": repository_name(&https),
        }));
    }
    let https = repository_url(typed)?;
    let (service, instance) = server_of(s, &https);
    let kind = host_kind(s, &https);
    let user = given_user(p)?;
    let key = match given_token(p)? {
        Some(t) => Some(Key::Token(t)),
        None => known_key(s, &instance, service, hosting(&https).is_some()),
    };
    let sent = key.is_some();
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let (filled, url, sent_key) = (slot.clone(), https.clone(), key.clone());
    let make = move || async move {
        let credentials = match sent_key {
            Some(key) => Some(credentials(user.as_deref(), &url, service, &key.token(&*connect(), &clients, &filled).await?)),
            None => None,
        };
        let remote = Remote::new(&url, credentials);
        Ok(texcraft_gitwire::advertised(&*http(), &remote, texcraft_gitwire::Service::UploadPack))
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| e.to_string())? {
        Ok(pending) => {
            s.import.task = Some(Task::Check { pending, renewed: slot, key, sent, https, instance, kind });
            Ok(json!({"operation": "check_address", "started": true}))
        }
        Err(answer) => checked(s, answer, key, sent, &https, &instance, kind),
    }
}

/// A check's outcome: the branches, or that the server needs signing in; any other refusal is an
/// error, in words.
fn checked(s: &mut Session, answer: Checked, key: Option<Key>, sent: bool, https: &str, instance: &str, kind: &'static str) -> Result<Value> {
    let host = super::super::git::remote_host(https).unwrap_or_default();
    let base = json!({"operation": "check_address", "https_url": https, "host": kind, "ssh": false, "name": repository_name(https)});
    match answer {
        Ok(advertised) => {
            s.import.problem = None;
            if let Some(key) = key {
                s.keep_key(instance, key);
            }
            let branches: Vec<&str> = advertised.refs.iter().filter_map(|(name, _)| name.strip_prefix("refs/heads/")).collect();
            let default = advertised.head().and_then(|h| h.strip_prefix("refs/heads/")).filter(|h| branches.contains(h));
            let mut result = base;
            result["reachable"] = json!(true);
            result["needs_sign_in"] = json!(false);
            result["signed_in"] = json!(sent);
            result["default_branch"] = json!(default.or(branches.first().copied()));
            result["branches"] = json!(branches);
            result["empty"] = json!(branches.is_empty());
            Ok(result)
        }
        Err(WireError::Unauthorized) if !sent => {
            s.import.problem = Some(ImportProblem::NeedsSignIn);
            let mut result = base;
            result["reachable"] = json!(true);
            result["needs_sign_in"] = json!(true);
            result["branches"] = json!([]);
            Ok(result)
        }
        Err(e) => {
            let (problem, message) = words(&e, &host, https, sent);
            s.import.problem = Some(problem);
            Err(message)
        }
    }
}

pub(super) fn clone(s: &mut Session, p: &Value) -> Result<Value> {
    s.import.problem = None;
    let typed = super::typed_address(super::super::string(p, "url")?);
    let https = repository_url(&typed)?;
    let folder = PathBuf::from(super::super::string(p, "folder")?.trim());
    if !folder.is_absolute() {
        return Err("folder: give the whole path of the new folder.".to_owned());
    }
    if let Some(roots) = &s.roots {
        roots.check(&folder, Access::Write)?;
    }
    let branch = p.get("branch").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned);
    if let Some(b) = &branch
        && texcraft_forge::check_branch_name(b).is_err()
    {
        return Err(format!("{b:?} cannot be a branch's name."));
    }
    let user = given_user(p)?;
    let (mut service, instance) = server_of(s, &https);
    if let Some(named) = p.get("service").and_then(Value::as_str) {
        service = Service::parse(named).ok_or_else(|| format!("service: say \"gitlab\" or \"github\", not {named:?}."))?;
    }
    let hosted = hosting(&https);
    let key = match given_token(p)? {
        Some(t) => Some(Key::Token(t)),
        None => known_key(s, &instance, service, hosted.is_some()),
    };
    let made = make_folder(&folder).inspect_err(|_| s.import.problem = Some(ImportProblem::Folder))?;
    // The remote as kept in the repository: the user name given goes in its address (git's way),
    // so that later pushes and pulls send it.
    let origin = match (&user, hosted) {
        (Some(u), None) => https.replacen("://", &format!("://{u}@"), 1),
        _ => https.clone(),
    };
    let remember = p.get("remember").and_then(Value::as_bool).unwrap_or(false);
    let open = p.get("open").and_then(Value::as_bool).unwrap_or(true);
    let (stop, phase) = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(ClonePhase::Connecting)));
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let (filled, url, sent_key, at) = (slot.clone(), https.clone(), key.clone(), folder.clone());
    let (stopped, phased, sent) = (stop.clone(), phase.clone(), key.is_some());
    let host = super::super::git::remote_host(&https).unwrap_or_default();
    let limit = s.import.limit;
    let make = move || async move {
        let credentials = match sent_key {
            Some(key) => Some(credentials(user.as_deref(), &url, service, &key.token(&*connect(), &clients, &filled).await?)),
            None => None,
        };
        let remote = Remote::new(&url, credentials);
        let done = run(&*http(), &remote, &at, (branch, &origin, limit), &stopped, &phased).map_err(|e| match e {
            Run::Wire(e) => {
                let (problem, message) = words(&e, &host, &url, sent);
                Failed { problem, message }
            }
            Run::Other(problem, message) => Failed { problem, message },
        });
        if stopped.load(Ordering::Relaxed) {
            clear(&at, made);
        }
        Ok(done)
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    let target = Target { folder, made, https, instance, key, remember, open };
    match started {
        Ok(Ok(pending)) => {
            s.import.task = Some(Task::Clone(Box::new(Cloning { pending, renewed: slot, stop, phase, target })));
            Ok(json!({"operation": "clone", "started": true}))
        }
        Ok(Err(done)) => cloned(s, done, target),
        Err(e) => {
            clear(&target.folder, target.made);
            Err(e.to_string())
        }
    }
}

/// Makes the clone's folder, or checks it is empty; whether TeXCraft made it.
fn make_folder(folder: &Path) -> Result<bool> {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if folder.is_file() {
        return Err(format!("A file named {name} is there already: choose another name."));
    }
    if folder.is_dir() {
        if !folder_free(folder) {
            let parent = folder.parent().unwrap_or(folder);
            let free = super::free_folder(parent, &name);
            let free = free.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(format!("A folder named {name} is there already: choose another, such as {free}."));
        }
        return Ok(false);
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("Could not write to {}: {e}.", folder.display()))?;
    Ok(true)
}

/// Removes what the clone put in `folder`: the folder itself when TeXCraft made it, else what is
/// in it (it was empty).
fn clear(folder: &Path, made: bool) {
    if made {
        if let Err(e) = std::fs::remove_dir_all(folder)
            && folder.exists()
        {
            log::warn!("the folder of a stopped import, {}, could not be removed: {e}", folder.display());
        }
        return;
    }
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = removed {
            log::warn!("{} could not be removed after a stopped import: {e}", path.display());
        }
    }
}

/// Why the clone's thread stopped.
enum Run {
    Wire(WireError),
    Other(ImportProblem, String),
}

impl From<WireError> for Run {
    fn from(e: WireError) -> Self {
        Run::Wire(e)
    }
}

/// An HTTP client that refuses answers larger than `limit`: the pack of a history TeXCraft does
/// not import.
struct Limited<'a> {
    http: &'a dyn Http,
    limit: u64,
    over: std::cell::Cell<Option<u64>>,
}

impl Http for Limited<'_> {
    fn get(&self, url: &str, headers: &[(String, String)]) -> std::result::Result<Reply, String> {
        self.http.get(url, headers)
    }

    fn post(&self, url: &str, headers: &[(String, String)], body: Vec<u8>) -> std::result::Result<Reply, String> {
        let reply = self.http.post(url, headers, body)?;
        if reply.body.len() as u64 > self.limit {
            self.over.set(Some(reply.body.len() as u64));
            return Err("the repository is too large".to_owned());
        }
        Ok(reply)
    }
}

/// The clone itself, on its thread: the branch chosen from the server's, an empty repository,
/// the history fetched, the branch checked out and following `origin`.
fn run(
    http: &dyn Http,
    remote: &Remote,
    folder: &Path,
    (branch, origin, limit): (Option<String>, &str, u64),
    stop: &AtomicBool,
    phase: &Mutex<ClonePhase>,
) -> std::result::Result<Cloned, Run> {
    let set = |p: ClonePhase| *phase.lock().unwrap_or_else(PoisonError::into_inner) = p;
    let stopped = || Run::Other(ImportProblem::Other, "The import was stopped.".to_owned());
    let advertised = texcraft_gitwire::advertised(http, remote, texcraft_gitwire::Service::UploadPack)?;
    if stop.load(Ordering::Relaxed) {
        return Err(stopped());
    }
    let heads: Vec<&str> = advertised.refs.iter().filter_map(|(n, _)| n.strip_prefix("refs/heads/")).collect();
    if heads.is_empty() {
        return Err(Run::Other(ImportProblem::Other, "This repository is empty: there is nothing to import yet.".to_owned()));
    }
    let branch = branch
        .or_else(|| advertised.head().and_then(|h| h.strip_prefix("refs/heads/")).filter(|h| heads.contains(h)).map(str::to_owned))
        .or_else(|| heads.first().map(|h| (*h).to_owned()))
        .unwrap_or_else(|| "main".to_owned());
    if !heads.contains(&branch.as_str()) {
        return Err(Run::Other(ImportProblem::Other, format!("The repository has no branch {branch}.")));
    }
    let local = |e: String| Run::Other(ImportProblem::Folder, format!("Could not write to {}: {e}", folder.display()));
    let mut history = History::init_for_clone(folder, &branch).map_err(local)?;
    set(ClonePhase::Receiving);
    let limited = Limited { http, limit, over: std::cell::Cell::new(None) };
    let reference = format!("refs/heads/{branch}");
    let fetched = texcraft_gitwire::fetch(&limited, remote, history.git_dir(), &reference, &[]);
    if let Some(size) = limited.over.get() {
        let limit = texcraft_project::size_words(limit);
        return Err(Run::Other(
            ImportProblem::TooLarge,
            format!("This repository is {}; TeXCraft imports up to {limit}.", texcraft_project::size_words(size)),
        ));
    }
    let fetched = fetched?;
    if stop.load(Ordering::Relaxed) {
        return Err(stopped());
    }
    let tip = fetched.tip.ok_or_else(|| Run::Other(ImportProblem::Other, format!("The repository has no branch {branch}.")))?;
    set(ClonePhase::Writing);
    history.set_ref(&format!("refs/remotes/origin/{branch}"), &tip, "clone").map_err(local)?;
    history.fast_forward(&tip).map_err(local)?;
    history.set_remote("origin", origin).map_err(local)?;
    history.set_upstream("origin", &branch).map_err(local)?;
    Ok(Cloned { branch, tip })
}

/// A clone's end: on success the credential kept (for the session, and in the keychain when
/// asked) and the project opened; on failure the folder cleared, and why said.
fn cloned(s: &mut Session, done: std::result::Result<Cloned, Failed>, target: Target) -> Result<Value> {
    let Target { folder, made, https, instance, key, remember, open } = target;
    let cloned = match done {
        Ok(c) => c,
        Err(failed) => {
            clear(&folder, made);
            s.import.problem = Some(failed.problem);
            return Err(failed.message);
        }
    };
    let mut result = json!({"operation": "clone", "remote": "origin", "url": https, "branch": cloned.branch, "commit": cloned.tip});
    if let Some(key) = key {
        if remember && let Key::Token(token) = &key {
            let host = texcraft_forge::servers::host_of(&instance).to_owned();
            match s.keep_git_token(&host, token) {
                Ok(()) => result["kept"] = json!(s.git_token_kept()),
                Err(e) => result["warning"] = json!(format!("The token is kept for this session only: the keychain could not keep it ({e}).")),
            }
        }
        s.keep_key(&instance, key);
    }
    result["folder"] = json!(folder.display().to_string());
    result["name"] = json!(folder.file_name().map(|n| n.to_string_lossy().into_owned()));
    if !open {
        result["opened"] = json!(false);
        return Ok(result);
    }
    let opened = super::super::project::open(s, &json!({"path": folder.display().to_string()}))?;
    result["opened"] = json!(true);
    result["files"] = opened["files"].clone();
    result["main"] = opened["main"].clone();
    result["skipped"] = opened.get("not_loaded").cloned().unwrap_or_else(|| json!([]));
    if let Some(note) = opened.get("note") {
        result["note"] = note.clone();
    }
    Ok(result)
}

/// Stops the clone under way and removes what it made.
pub(super) fn cancel(s: &mut Session) -> Value {
    match s.import.task.take() {
        Some(Task::Clone(cloning)) => {
            cloning.stop.store(true, Ordering::Relaxed);
            clear(&cloning.target.folder, cloning.target.made);
            s.import.problem = None;
            json!({"cancelled": true, "removed": cloning.target.made, "folder": cloning.target.folder.display().to_string()})
        }
        Some(check @ Task::Check { .. }) => {
            // A check is not stopped by this; it ends by itself.
            s.import.task = Some(check);
            json!({"cancelled": false})
        }
        None => json!({"cancelled": false}),
    }
}

/// The outcome of the check or the clone that ended, if one did.
pub(super) fn poll(s: &mut Session) -> Option<Result<Value>> {
    let ready = match s.import.task.as_mut()? {
        Task::Check { pending, .. } => Ended::Checked(pending.poll()?),
        Task::Clone(c) => Ended::Cloned(c.pending.poll()?),
    };
    let task = s.import.task.take()?;
    Some(match (ready, task) {
        (Ended::Checked(answer), Task::Check { renewed, key, sent, https, instance, kind, .. }) => {
            s.keep_renewed(&renewed);
            answer.map_err(|e| e.to_string()).and_then(|a| checked(s, a, key, sent, &https, &instance, kind))
        }
        (Ended::Cloned(done), Task::Clone(cloning)) => {
            let Cloning { renewed, target, .. } = *cloning;
            s.keep_renewed(&renewed);
            match done {
                Ok(done) => cloned(s, done, target),
                Err(e) => {
                    clear(&target.folder, target.made);
                    Err(e.to_string())
                }
            }
        }
        _ => Err(texcraft_forge::Error::Stopped.to_string()),
    })
}

/// A background operation's outcome.
enum Ended {
    Checked(texcraft_forge::Result<Checked>),
    Cloned(texcraft_forge::Result<std::result::Result<Cloned, Failed>>),
}
