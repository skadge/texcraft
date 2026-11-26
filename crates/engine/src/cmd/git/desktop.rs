//! The desktop's push and pull: the folder's repository, its remotes, and git's protocol on an
//! operation's thread.

use std::sync::Arc;

use serde_json::{Value, json};
use texcraft_forge::{GITHUB_COM, GITLAB_COM, Service};
use texcraft_history::{ChangeKind, History, Merged, Person};
use texcraft_project::ProjectPath;

use super::super::forge::{Renewed, start};
use super::super::history::{allowed, snapshot};
use super::{Made, Prepared, SAVE_MESSAGE, Syncing, Task, hosting, remote_credentials, write_key};
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
    if let Some(rest) = url.strip_prefix("https://") {
        // A user name in the address (`https://git@host/…`) goes in the credentials, not the URL.
        let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        return Ok(if path.is_empty() { format!("https://{host}") } else { format!("https://{host}/{path}") });
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
pub(crate) fn server_of(s: &Session, https: &str) -> (Service, String) {
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

/// The page of a forge's repository from its git address, when it reads as one (not a hosting
/// service's git address, whose page is elsewhere).
fn web_url(https: &str) -> Option<String> {
    if hosting(https).is_some() {
        return None;
    }
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
        "hosting": url.as_deref().and_then(hosting).map(|h| h.id()),
        "hosting_name": url.as_deref().and_then(hosting).map(|h| h.name()),
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

/// Whether the folder's repository has the remote a push or a pull would reach.
pub(super) fn has_remote(s: &Session) -> bool {
    let Some(h) = &s.history else { return false };
    which(h, &json!({})).ok().and_then(|(remote, _)| h.remote_url(&remote).ok().flatten()).is_some()
}

/// Where a push or a pull goes: the remote, its branch, its address as configured and over HTTPS,
/// and its server.
struct Target {
    remote: String,
    branch: String,
    url: String,
    https: String,
    service: Service,
    instance: String,
}

/// `push`: the target of a push (its refusal says how to get a remote).
fn target(s: &Session, p: &Value, push: bool) -> Result<Target> {
    let h = history(s)?;
    let (remote, branch) = which(h, p)?;
    let url = h.remote_url(&remote)?.ok_or_else(|| {
        if push {
            format!("The project has no remote {remote}: set one (git.set_remote), or save the project to GitLab or GitHub.")
        } else {
            format!("The project has no remote {remote}: set one (git.set_remote) first.")
        }
    })?;
    let https = https_url(&url)?;
    let (service, instance) = server_of(s, &https);
    Ok(Target { remote, branch, url, https, service, instance })
}

/// The key for `target`'s server: the token given, the one given earlier in this session, or the
/// sign-in to it. A hosting service's git access (Overleaf) takes its own token, never a forge's
/// sign-in. `required`: a key is needed (a push; a pull from a hosting service); otherwise a
/// public repository is read without one. With none, the session asks for a token (rule 15:
/// said in words, and [`super::super::forge::Problem::NeedsToken`]).
fn git_key(s: &mut Session, p: &Value, target: &Target, write: bool) -> Result<Option<super::super::forge::Key>> {
    use super::super::forge::{Key, Problem};
    let hosted = hosting(&target.url);
    let given = p.get("token").and_then(Value::as_str).filter(|t| !t.trim().is_empty());
    let needs = |s: &mut Session| s.forge_problem = Some(Problem::NeedsToken { instance: target.instance.clone(), hosting: hosted });
    if let Some(h) = hosted {
        if let Some(t) = given {
            return texcraft_forge::Token::new(t).map(|t| Some(Key::Token(t))).map_err(|e| e.to_string());
        }
        if let Some(token) = s.forge_tokens.get(&target.instance) {
            return Ok(Some(Key::Token(token.clone())));
        }
        if let Some(token) = s.kept_git_token(texcraft_forge::servers::host_of(&target.instance)) {
            return Ok(Some(Key::Token(token)));
        }
        needs(s);
        return Err(format!(
            "Give your {} git authentication token for this project (token): TeXCraft keeps it only while it runs, never on disk.",
            h.name()
        ));
    }
    let host = texcraft_forge::servers::host_of(&target.instance).to_owned();
    if write {
        return match write_key(s, p, &target.instance, target.service) {
            Ok(key) => Ok(Some(key)),
            Err(_)
                if given.is_none()
                    && let Some(token) = s.kept_git_token(&host) =>
            {
                Ok(Some(Key::Token(token)))
            }
            Err(e) => {
                if s.forge_problem.is_none() {
                    needs(s);
                }
                Err(e)
            }
        };
    }
    // A public repository is read without credentials.
    if given.is_some() {
        super::super::forge::key_at(s, p, &target.instance, target.service).map(Some)
    } else {
        Ok(super::super::forge::key_at(s, p, &target.instance, target.service).ok().or_else(|| s.kept_git_token(&host).map(Key::Token)))
    }
}

/// The key a transfer was given, kept for the session once the server took it.
pub(crate) struct Kept {
    instance: String,
    url: String,
    key: Option<super::super::forge::Key>,
}

/// A transfer ended: the key the server took is kept for the session; credentials it refused are
/// forgotten, and a new token asked for.
pub(super) fn settle<T>(s: &mut Session, kept: Kept, done: std::result::Result<T, String>) -> std::result::Result<T, String> {
    match &done {
        Ok(_) => {
            if let Some(key) = kept.key {
                s.keep_key(&kept.instance, key);
            }
        }
        Err(e) if *e == texcraft_gitwire::Error::Unauthorized.to_string() => {
            s.forge_tokens.remove(&kept.instance);
            s.forge_problem = Some(super::super::forge::Problem::NeedsToken { instance: kept.instance.clone(), hosting: hosting(&kept.url) });
        }
        Err(_) => {}
    }
    done
}

pub(super) fn push(s: &mut Session, p: &Value) -> Result<Value> {
    start_push(s, p, None).map(|done| done.unwrap_or_else(|| json!({"operation": "git_push", "started": true})))
}

/// Starts a push: `Ok(None)` while it runs in the background, else its outcome (a Sync's, with
/// `sync`).
fn start_push(s: &mut Session, p: &Value, sync: Option<Box<Syncing>>) -> Result<Option<Value>> {
    let h = history(s)?;
    allowed(s, h, Access::Read)?;
    let target = target(s, p, true)?;
    check_committed(s, "a push sends what is committed")?;
    let h = history(s)?;
    let head = h.head()?.ok_or("The project has no commit yet: save it first.")?;
    let git_dir = h.git_dir().to_path_buf();
    let key = git_key(s, p, &target, true)?.ok_or("No credentials for the remote's server.")?;
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let filled = slot.clone();
    let reference = format!("refs/heads/{}", target.branch);
    let sent = key.clone();
    let (url, https, service) = (target.url.clone(), target.https.clone(), target.service);
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let remote = texcraft_gitwire::Remote::new(&https, Some(remote_credentials(&url, service, &token)));
        Ok(texcraft_gitwire::push(&*http(), &remote, &git_dir, &head, &reference, false).map_err(|e| e.to_string()))
    };
    let background = super::super::forge::background(p) || sync.as_ref().is_some_and(|s| super::super::forge::background(&s.params));
    let started = start(&json!({"background": background}), make);
    s.keep_renewed(&slot);
    let Target { remote, branch, instance, url, .. } = target;
    let kept = Kept { instance, url, key: Some(key) };
    match started.map_err(|e| s.forge_failed(e))? {
        Ok(pending) => {
            s.git_task = Some(Task::Push { pending, remote, branch, renewed: slot, kept, sync });
            Ok(None)
        }
        Err(done) => {
            let done = settle(s, kept, done);
            let pushed = pushed(s, &remote, &branch, done)?;
            Ok(Some(match sync {
                Some(sync) => synced(*sync, pushed),
                None => pushed,
            }))
        }
    }
}

/// A push's outcome: the remote's branch, as now known, moves; the branch follows it when it
/// followed none.
pub(super) fn pushed(s: &mut Session, remote: &str, branch: &str, done: std::result::Result<texcraft_gitwire::Pushed, String>) -> Result<Value> {
    // Sent, or maybe sent: the session's versions are no longer its own to amend.
    s.versions_sent();
    let pushed = done?;
    let h = s.history.as_mut().ok_or("The project has no git repository.")?;
    h.set_ref(&format!("refs/remotes/{remote}/{branch}"), &pushed.new, "push")?;
    if h.upstream()?.is_none() {
        h.set_upstream(remote, branch)?;
    }
    Ok(json!({
        "operation": "git_push",
        "remote": remote,
        "branch": branch,
        "commit": pushed.new,
        "old": pushed.old,
        "objects": pushed.objects,
        "up_to_date": pushed.up_to_date,
    }))
}

pub(super) fn pull(s: &mut Session, p: &Value) -> Result<Value> {
    start_pull(s, p, None).map(|done| done.unwrap_or_else(|| json!({"operation": "git_pull", "started": true})))
}

/// Starts a pull: `Ok(None)` while it runs in the background, else its outcome (for a Sync, the
/// push that follows it started or done).
fn start_pull(s: &mut Session, p: &Value, sync: Option<Box<Syncing>>) -> Result<Option<Value>> {
    let h = history(s)?;
    allowed(s, h, Access::Write)?;
    let target = target(s, p, false)?;
    let choices = super::super::forge::choices(p)?;
    check_committed(s, "a pull changes the files on disk")?;
    let h = history(s)?;
    let tips: Vec<String> = [h.head()?, h.ref_id(&format!("refs/remotes/{}/{}", target.remote, target.branch))?].into_iter().flatten().collect();
    let git_dir = h.git_dir().to_path_buf();
    let key = git_key(s, p, &target, false)?;
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let filled = slot.clone();
    let reference = format!("refs/heads/{}", target.branch);
    let sent = key.clone();
    let (url, https, service) = (target.url.clone(), target.https.clone(), target.service);
    let make = move || async move {
        let credentials = match sent {
            Some(key) => Some(remote_credentials(&url, service, &key.token(&*connect(), &clients, &filled).await?)),
            None => None,
        };
        let remote = texcraft_gitwire::Remote::new(&https, credentials);
        Ok(texcraft_gitwire::fetch(&*http(), &remote, &git_dir, &reference, &tips).map_err(|e| e.to_string()))
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    let Target { remote, branch, instance, url, .. } = target;
    let kept = Kept { instance, url, key };
    match started.map_err(|e| s.forge_failed(e))? {
        Ok(pending) => {
            s.git_task = Some(Task::Pull { pending, remote, branch, renewed: slot, kept, choices, sync });
            Ok(None)
        }
        Err(done) => {
            let done = settle(s, kept, done);
            let pulled = fetched(s, &remote, &branch, done, &choices, sync.is_some())?;
            match sync {
                Some(mut sync) => {
                    sync.pulled = Some(pulled);
                    push_for_sync(s, sync)
                }
                None => Ok(Some(pulled)),
            }
        }
    }
}

/// Sync on the desktop: a version of the changes, then a pull, then a push.
pub(super) fn sync(s: &mut Session, p: &Value) -> Result<Value> {
    s.forge_problem = None;
    let version = super::super::history::make_version(s, None, false)?;
    let sync = Box::new(Syncing { params: p.clone(), version, pulled: None });
    Ok(start_pull(s, p, Some(sync))?.unwrap_or_else(|| json!({"operation": "sync", "started": true})))
}

/// A Sync's pull is done: its push starts (`Ok(None)` while it runs), or is done.
pub(super) fn push_for_sync(s: &mut Session, sync: Box<Syncing>) -> Result<Option<Value>> {
    if sync.pulled.as_ref().is_some_and(|p| p["up_to_date"] == true && p["ahead"] == 0) {
        // Nothing to send either.
        return Ok(Some(synced(*sync, json!({"operation": "git_push", "up_to_date": true}))));
    }
    let params = sync.params.clone();
    start_push(s, &params, Some(sync))
}

/// A Sync's outcome: the version it made, what the pull took in and what the push sent.
pub(super) fn synced(sync: Syncing, pushed: Value) -> Value {
    json!({"operation": "sync", "version": sync.version, "pulled": sync.pulled, "pushed": pushed})
}

/// A fetch's outcome: the remote's branch, as now known, moves, then the branch checked out
/// catches up with it: a fast-forward, or a merge when both have commits the other has not
/// (text files changed on both sides merged line by line, files in conflict kept as `choices`
/// says; any other that does not merge refuses the pull, with nothing changed, and is listed by
/// `forge.status`). `empty_ok`: a remote with no such branch yet is fine (a Sync then pushes it).
pub(super) fn fetched(
    s: &mut Session,
    remote: &str,
    branch: &str,
    done: std::result::Result<texcraft_gitwire::Fetched, String>,
    choices: &texcraft_forge::Choices,
    empty_ok: bool,
) -> Result<Value> {
    let fetched = done?;
    let Some(tip) = fetched.tip else {
        if empty_ok {
            return Ok(json!({"operation": "git_pull", "remote": remote, "branch": branch, "empty": true, "changed": []}));
        }
        return Err(format!("The remote {remote} has no branch {branch}: push it there first."));
    };
    let author = s.author.as_ref().map(|a| Person::new(a.name.clone(), ""));
    let now = s.now();
    let h = s.history.as_mut().ok_or("The project has no git repository.")?;
    h.set_ref(&format!("refs/remotes/{remote}/{branch}"), &tip, "fetch")?;
    let head = h.head()?;
    let mut result = json!({"operation": "git_pull", "remote": remote, "branch": branch, "commit": tip, "received": fetched.received});
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
            let choose = |path: &str| {
                choices.get(path).map(|c| match c {
                    texcraft_forge::Choice::Mine => texcraft_history::Side::Ours,
                    texcraft_forge::Choice::Theirs => texcraft_history::Side::Theirs,
                })
            };
            match h.merge_choosing(&tip, &message, author.as_ref(), now, &merge, &choose)? {
                Merged::Done { commit, changed } => {
                    result["merged"] = json!(true);
                    result["commit"] = json!(commit);
                    changed
                }
                Merged::Conflicts(files) => {
                    let list: Vec<String> = files.iter().map(|(p, why)| format!("{p} ({why})")).collect();
                    let conflicts =
                        files.iter().map(|(path, why)| texcraft_forge::Conflict { path: path.clone(), kind: conflict_kind(why) }).collect();
                    s.forge_problem = Some(super::super::forge::Problem::Conflicts(conflicts));
                    return Err(format!(
                        "The remote's branch {branch} and this project changed the same files in ways that do not merge: {}. Nothing was changed: choose which version of each to keep, then try again.",
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
    s.forge_problem = None;
    Ok(result)
}

/// A conflict's reason, as the merge words it, as the forges' conflicts have it.
fn conflict_kind(why: &str) -> texcraft_forge::ConflictKind {
    use texcraft_forge::ConflictKind;
    match why {
        "deleted here, changed on the remote" => ConflictKind::DeletedHere,
        "changed here, deleted on the remote" => ConflictKind::DeletedThere,
        "added on both sides" => ConflictKind::AddedBoth,
        "changed on both sides, and not text" => ConflictKind::Binary,
        _ => ConflictKind::Overlap,
    }
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
    s.mark_versioned();
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
    // Agents write only inside their roots.
    if let Some(roots) = &s.roots {
        roots.check(&ignore, Access::Write)?;
    }
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
    s.versions_sent();
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
