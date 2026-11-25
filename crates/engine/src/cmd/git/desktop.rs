//! The desktop's push and pull: the folder's repository, its remotes, and git's protocol on an
//! operation's thread.

use std::sync::Arc;

use serde_json::{Value, json};
use texcraft_forge::{GITHUB_COM, GITLAB_COM, Service};
use texcraft_history::{ChangeKind, History, Merged, Person};
use texcraft_project::ProjectPath;

use super::super::forge::{Renewed, start};
use super::super::history::{allowed, snapshot};
use super::{Made, Prepared, SAVE_MESSAGE, Task, credentials, write_key};
use crate::file_access::Access;
use crate::{Result, Session};

/// Makes the HTTP client git's protocol goes over (on the operation's thread).
pub type GitHttp = Arc<dyn Fn() -> Box<dyn texcraft_gitwire::Http> + Send + Sync>;

/// Git's protocol over the forges' transport: the same HTTPS client and certificates.
struct OverTransport(Box<dyn texcraft_forge::Transport>);

impl OverTransport {
    fn send(
        &self,
        method: &'static str,
        url: &str,
        headers: &[(String, String)],
        body: Option<Vec<u8>>,
    ) -> std::result::Result<texcraft_gitwire::Reply, String> {
        let request =
            texcraft_forge::Request { method, url: url.to_owned(), headers: headers.to_vec(), body, max_body: texcraft_gitwire::MAX_ANSWER };
        let response = texcraft_forge::task::block_on(self.0.send(request))?;
        if response.too_large {
            return Err(format!("the server's answer is larger than {} MB", texcraft_gitwire::MAX_ANSWER / (1024 * 1024)));
        }
        let content_type = response.header("content-type").map(str::to_owned);
        Ok(texcraft_gitwire::Reply { status: response.status, content_type, body: response.body })
    }
}

impl texcraft_gitwire::Http for OverTransport {
    fn get(&self, url: &str, headers: &[(String, String)]) -> std::result::Result<texcraft_gitwire::Reply, String> {
        self.send("GET", url, headers, None)
    }

    fn post(&self, url: &str, headers: &[(String, String)], body: Vec<u8>) -> std::result::Result<texcraft_gitwire::Reply, String> {
        self.send("POST", url, headers, Some(body))
    }
}

/// This build's: over the forges' HTTPS client.
pub fn default_http() -> GitHttp {
    Arc::new(|| Box::new(OverTransport(texcraft_forge::transport())))
}

impl Session {
    /// Reaches git servers through the clients `make` makes, instead of the network (tests).
    pub fn set_git_http(&mut self, make: impl Fn() -> Box<dyn texcraft_gitwire::Http> + Send + Sync + 'static) {
        self.git_http = Arc::new(make);
    }
}

fn history(s: &Session) -> Result<&History> {
    s.history.as_ref().ok_or_else(|| "The project has no git repository yet: save it in a folder first.".to_owned())
}

/// The remote and its branch a push or a pull goes to: the parameters', else the upstream's,
/// else `origin` and the branch checked out.
fn which(h: &History, p: &Value) -> Result<(String, String)> {
    let given = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
    let upstream = h.upstream()?;
    let remote = given("remote").or_else(|| upstream.as_ref().map(|u| u.0.clone())).unwrap_or_else(|| "origin".to_owned());
    let branch = given("branch").or_else(|| upstream.map(|u| u.1)).or_else(|| h.branch()).unwrap_or_else(|| "main".to_owned());
    if texcraft_forge::check_branch_name(&branch).is_err() {
        return Err(format!("{branch:?} cannot be a branch's name."));
    }
    Ok((remote, branch))
}

/// `url` as an HTTPS address: an SSH address of a server (`git@host:owner/name.git`,
/// `ssh://git@host/owner/name.git`) is reached over HTTPS at the same path.
pub(crate) fn https_url(url: &str) -> Result<String> {
    let url = url.trim().trim_end_matches('/');
    let not = || format!("TeXCraft reaches remotes over HTTPS: {url} is not an address it can reach.");
    if url.starts_with("https://") {
        return Ok(url.to_owned());
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or_default();
        if matches!(host, "localhost" | "127.0.0.1") {
            return Ok(url.to_owned());
        }
        return Err(format!("{url} is plain HTTP, which would send the credentials in the clear: use its https:// address."));
    }
    let (host, path) = if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/').ok_or_else(not)?;
        (host.split(':').next().unwrap_or_default().to_owned(), path.to_owned())
    } else if let Some((user_host, path)) = url.split_once(':').filter(|(h, _)| h.contains('@') && !h.contains('/')) {
        (user_host.split_once('@').map_or(user_host, |(_, h)| h).to_owned(), path.to_owned())
    } else {
        return Err(not());
    };
    if host.is_empty() || path.is_empty() {
        return Err(not());
    }
    Ok(format!("https://{host}/{}", path.trim_start_matches('/')))
}

/// The forge an HTTPS address is on, and its instance: github.com's and gitlab.com's, a server
/// added, else a GitLab.
fn server_of(s: &Session, https: &str) -> (Service, String) {
    let (scheme, rest) = https.split_once("://").unwrap_or(("https", https));
    let host = rest.split('/').next().unwrap_or_default();
    let instance = format!("{scheme}://{host}");
    let service = match instance.as_str() {
        GITHUB_COM => Service::GitHub,
        GITLAB_COM => Service::GitLab,
        _ => s.forge_servers().into_iter().find(|v| v.instance.eq_ignore_ascii_case(&instance)).map_or(Service::GitLab, |v| v.service),
    };
    (service, instance)
}

/// The page of a forge's repository from its git address, when it reads as one.
fn web_url(https: &str) -> Option<String> {
    let address = texcraft_forge::Address::parse(https).ok()?;
    Some(address.web_url())
}

pub(super) fn remotes(s: &mut Session) -> Result<Value> {
    let Some(h) = &s.history else {
        return Ok(json!({"repository": false, "remotes": [], "why": "The project has no git repository yet: save it in a folder first."}));
    };
    let list = h.remotes()?;
    let upstream = h.upstream()?;
    let (remote, branch) = which(h, &json!({}))?;
    let head = h.head()?;
    let url = list.iter().find(|r| r.name == remote).map(|r| r.url.clone());
    let tracking = if url.is_some() { h.ref_id(&format!("refs/remotes/{remote}/{branch}"))? } else { None };
    let (ahead, behind) = match (&head, &tracking) {
        (Some(a), Some(b)) => {
            let (ahead, behind) = h.ahead_behind(a, b)?;
            (Some(ahead), Some(behind))
        }
        _ => (None, None),
    };
    Ok(json!({
        "repository": true,
        "branch": h.branch(),
        "head": head,
        "remotes": list.iter().map(|r| json!({"name": r.name, "url": r.url, "web_url": https_url(&r.url).ok().as_deref().and_then(web_url)})).collect::<Vec<_>>(),
        "upstream": upstream.map(|(remote, branch)| json!({"remote": remote, "branch": branch})),
        "remote": url.as_ref().map(|_| remote.clone()),
        "remote_branch": url.as_ref().map(|_| branch.clone()),
        "url": url,
        "web_url": url.as_deref().and_then(|u| https_url(u).ok()).as_deref().and_then(web_url),
        "tracking": tracking,
        "ahead": ahead,
        "behind": behind,
        "running": s.git_busy(),
    }))
}

pub(super) fn set_remote(s: &mut Session, p: &Value) -> Result<Value> {
    let name = super::string(p, "name")?.trim().to_owned();
    let url = super::string(p, "url")?.trim().to_owned();
    https_url(&url)?;
    let history = history(s)?;
    allowed(s, history, Access::Write)?;
    let history = s.history.as_mut().ok_or("The project has no git repository.")?;
    history.set_remote(&name, &url)?;
    Ok(json!({"name": name, "url": url}))
}

/// Refuses a push or a pull while the project has changes not saved or not committed: a push
/// sends what is committed, and a pull moves the files on disk.
fn check_committed(s: &mut Session, doing: &str) -> Result<()> {
    if s.dirty {
        return Err(format!("Save the project first: {doing}."));
    }
    let snapshot = snapshot(s)?;
    if history(s)?.uncommitted(&snapshot)? {
        return Err(format!("Some changes are not committed: commit them first (history.commit), then try again: {doing}."));
    }
    Ok(())
}

pub(super) fn push(s: &mut Session, p: &Value) -> Result<Value> {
    let h = history(s)?;
    allowed(s, h, Access::Read)?;
    let (remote, branch) = which(h, p)?;
    let url = h
        .remote_url(&remote)?
        .ok_or_else(|| format!("The project has no remote {remote}: set one (git.set_remote), or save the project to GitLab or GitHub."))?;
    let https = https_url(&url)?;
    check_committed(s, "a push sends what is committed")?;
    let h = history(s)?;
    let head = h.head()?.ok_or("The project has no commit yet: save it first.")?;
    let git_dir = h.git_dir().to_path_buf();
    let (service, instance) = server_of(s, &https);
    let key = write_key(s, p, &instance, service)?;
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let filled = slot.clone();
    let reference = format!("refs/heads/{branch}");
    let make = move || async move {
        let token = key.token(&*connect(), &clients, &filled).await?;
        let remote = texcraft_gitwire::Remote::new(&https, Some(credentials(service, &token)));
        Ok(texcraft_gitwire::push(&*http(), &remote, &git_dir, &head, &reference, false).map_err(|e| e.to_string()))
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| s.forge_failed(e))? {
        Ok(pending) => {
            s.git_task = Some(Task::Push { pending, remote: remote.clone(), branch: branch.clone(), renewed: slot });
            Ok(json!({"operation": "push", "started": true, "remote": remote, "branch": branch}))
        }
        Err(done) => pushed(s, &remote, &branch, done),
    }
}

/// A push's outcome: the remote's branch, as now known, moves; the branch follows it when it
/// followed none.
pub(super) fn pushed(s: &mut Session, remote: &str, branch: &str, done: std::result::Result<texcraft_gitwire::Pushed, String>) -> Result<Value> {
    let pushed = done?;
    let h = s.history.as_mut().ok_or("The project has no git repository.")?;
    h.set_ref(&format!("refs/remotes/{remote}/{branch}"), &pushed.new, "push")?;
    if h.upstream()?.is_none() {
        h.set_upstream(remote, branch)?;
    }
    Ok(json!({
        "operation": "push",
        "remote": remote,
        "branch": branch,
        "commit": pushed.new,
        "old": pushed.old,
        "objects": pushed.objects,
        "up_to_date": pushed.up_to_date,
    }))
}

pub(super) fn pull(s: &mut Session, p: &Value) -> Result<Value> {
    let h = history(s)?;
    allowed(s, h, Access::Write)?;
    let (remote, branch) = which(h, p)?;
    let url = h.remote_url(&remote)?.ok_or_else(|| format!("The project has no remote {remote}: set one (git.set_remote) first."))?;
    let https = https_url(&url)?;
    check_committed(s, "a pull changes the files on disk")?;
    let h = history(s)?;
    let tips: Vec<String> = [h.head()?, h.ref_id(&format!("refs/remotes/{remote}/{branch}"))?].into_iter().flatten().collect();
    let git_dir = h.git_dir().to_path_buf();
    let (service, instance) = server_of(s, &https);
    // A public repository is read without credentials.
    let given = p.get("token").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty());
    let key = if given { Some(super::key_at(s, p, &instance, service)?) } else { super::key_at(s, p, &instance, service).ok() };
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let filled = slot.clone();
    let reference = format!("refs/heads/{branch}");
    let make = move || async move {
        let credentials = match key {
            Some(key) => Some(credentials(service, &key.token(&*connect(), &clients, &filled).await?)),
            None => None,
        };
        let remote = texcraft_gitwire::Remote::new(&https, credentials);
        Ok(texcraft_gitwire::fetch(&*http(), &remote, &git_dir, &reference, &tips).map_err(|e| e.to_string()))
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| s.forge_failed(e))? {
        Ok(pending) => {
            s.git_task = Some(Task::Pull { pending, remote: remote.clone(), branch: branch.clone(), renewed: slot });
            Ok(json!({"operation": "pull", "started": true, "remote": remote, "branch": branch}))
        }
        Err(done) => fetched(s, &remote, &branch, done),
    }
}

/// A fetch's outcome: the remote's branch, as now known, moves, then the branch checked out
/// catches up with it: a fast-forward, or a merge when both have commits the other has not
/// (text files changed on both sides merged line by line; anything else that does not merge
/// refuses the pull, with nothing changed).
pub(super) fn fetched(s: &mut Session, remote: &str, branch: &str, done: std::result::Result<texcraft_gitwire::Fetched, String>) -> Result<Value> {
    let fetched = done?;
    let tip = fetched.tip.ok_or_else(|| format!("The remote {remote} has no branch {branch}: push it there first."))?;
    let author = s.author.as_ref().map(|a| Person::new(a.name.clone(), ""));
    let now = s.now();
    let h = s.history.as_mut().ok_or("The project has no git repository.")?;
    h.set_ref(&format!("refs/remotes/{remote}/{branch}"), &tip, "fetch")?;
    let head = h.head()?;
    let mut result = json!({"operation": "pull", "remote": remote, "branch": branch, "commit": tip, "received": fetched.received});
    let changed = match &head {
        Some(head) if *head == tip || h.is_ancestor(&tip, head)? => {
            let (ahead, _) = h.ahead_behind(head, &tip)?;
            result["up_to_date"] = json!(true);
            result["commit"] = json!(head);
            result["ahead"] = json!(ahead);
            Vec::new()
        }
        Some(head) if !h.is_ancestor(head, &tip)? => {
            let message = format!("Merge {remote}/{branch}");
            let merge = |b: &[u8], o: &[u8], t: &[u8]| texcraft_forge::merge::merge3(b, o, t);
            match h.merge(&tip, &message, author.as_ref(), now, &merge)? {
                Merged::Done { commit, changed } => {
                    result["merged"] = json!(true);
                    result["commit"] = json!(commit);
                    changed
                }
                Merged::Conflicts(files) => {
                    let list: Vec<String> = files.iter().map(|(p, why)| format!("{p} ({why})")).collect();
                    return Err(format!(
                        "The remote's branch {branch} and this project changed the same files in ways that do not merge: {}. Nothing was changed: settle them with git, or keep one side's version and commit it.",
                        list.join(", ")
                    ));
                }
            }
        }
        _ => {
            result["fast_forward"] = json!(true);
            h.fast_forward(&tip)?
        }
    };
    if h.upstream()?.is_none() {
        h.set_upstream(remote, branch)?;
    }
    result["changed"] = json!(changed.iter().map(|(p, k)| json!({"path": p, "change": k.as_str()})).collect::<Vec<_>>());
    apply(s, &changed)?;
    Ok(result)
}

/// Puts the files a pull changed on disk into the project (and its review threads), as saved.
fn apply(s: &mut Session, changed: &[(String, ChangeKind)]) -> Result<()> {
    if changed.is_empty() {
        return Ok(());
    }
    let dir = s.dir.clone().ok_or("The project has no folder.")?;
    for (path, kind) in changed {
        let review = texcraft_forge::is_review(path);
        if !review && texcraft_history::is_hidden(path) {
            continue;
        }
        let done = match kind {
            ChangeKind::Deleted => s.remove_forge_file(path),
            _ => match std::fs::read(dir.join(path)) {
                Ok(bytes) => s.put_forge_file(path, bytes),
                Err(e) => Err(format!("{path} cannot be read: {e}")),
            },
        };
        if let Err(e) = done {
            log::warn!("after a pull, {path}: {e}");
            continue;
        }
        if !review && let Ok(pp) = ProjectPath::new(path) {
            if *kind == ChangeKind::Deleted {
                s.on_disk.remove(&pp);
            } else {
                s.on_disk.insert(pp);
            }
        }
    }
    s.touch();
    if s.current.as_ref().is_some_and(|c| s.project.get(c).is_none()) {
        s.current = s.project.main().cloned();
    }
    s.open.retain(|p| s.project.get(p).is_some());
    // The project is as on disk, which is as committed.
    s.dirty = false;
    Ok(())
}

/// Before a new repository: the folder's history with everything committed (the project saved,
/// a `.gitignore` of build files written when it has none), to push to the new repository.
pub(super) fn prepare_push(s: &mut Session, p: &Value) -> Result<Prepared> {
    let dir = s.dir.clone().ok_or("Save the project in a folder first: its history is what goes to the new repository.")?;
    if let Some(h) = &s.history
        && let Some(url) = h.remote_url("origin")?
    {
        return Err(format!("The project has a remote already, origin ({url}): push to it (git.push), or change it (git.set_remote)."));
    }
    let push = p.get("push").and_then(Value::as_bool).unwrap_or(true);
    if !push {
        let branch = s.history.as_ref().and_then(History::branch).unwrap_or_else(|| "main".to_owned());
        return Ok(Prepared::NoPush { branch });
    }
    let message = p.get("message").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()).unwrap_or(SAVE_MESSAGE).to_owned();
    let ignore = dir.join(".gitignore");
    let wrote_ignore = if ignore.exists() {
        None
    } else {
        let text = crate::import::gitignore();
        std::fs::write(&ignore, &text).map_err(|e| format!("{} cannot be written: {e}", ignore.display()))?;
        Some(text)
    };
    if s.dirty || s.history.is_none() {
        super::super::project::save_to(s, &dir)?;
    }
    let mut snapshot = snapshot(s)?;
    if let Some(text) = wrote_ignore {
        snapshot.files.push((".gitignore".to_owned(), text.into_bytes()));
    }
    let author = s.author.as_ref().map(|a| Person::new(a.name.clone(), ""));
    let now = s.now();
    let h = history(s)?;
    allowed(s, h, Access::Write)?;
    if h.uncommitted(&snapshot)? {
        h.commit(&snapshot, &message, author.as_ref(), now)?;
    }
    let branch = h.branch().ok_or("No branch is checked out (HEAD is detached): check out a branch with git first.")?;
    Ok(Prepared::Push { git_dir: h.git_dir().to_path_buf(), head: h.head()?, branch })
}

/// The desktop's part of a new repository's outcome: `origin` set to it, and after the push the
/// remote's branch known and followed.
pub(super) fn created(s: &mut Session, made: &Made, result: &mut Value) -> Result<()> {
    let h = s.history.as_mut().ok_or("The project has no git repository.")?;
    h.set_remote("origin", &made.created.clone_url)?;
    result["remote"] = json!("origin");
    match &made.pushed {
        Some(Ok(pushed)) => {
            h.set_ref(&format!("refs/remotes/origin/{}", made.branch), &pushed.new, "push")?;
            h.set_upstream("origin", &made.branch)?;
            result["pushed"] = json!(true);
            result["commit"] = json!(pushed.new);
            result["objects"] = json!(pushed.objects);
        }
        Some(Err(e)) => {
            result["pushed"] = json!(false);
            result["push_error"] = json!(e);
        }
        None => result["pushed"] = json!(false),
    }
    Ok(())
}
