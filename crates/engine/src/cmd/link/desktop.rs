//! The desktop's link: the folder's repository gets the remote, and a Sync brings the two
//! histories together; the check fetches the remote's branch without moving a ref.

use serde_json::{Value, json};
use texcraft_forge::Service;
use texcraft_forge::task::Pending;
use texcraft_gitwire::{Error as WireError, Remote};
use texcraft_history::History;

use super::super::clone::desktop::{credentials, given_token, given_user, host_kind, known_key, repository_url, words};
use super::super::forge::{Key, Problem, Renewed, start};
use super::super::git::desktop::{https_url, server_of};
use super::super::git::hosting;
use super::super::history::allowed;
use super::super::string;
use super::suggested_name;
use crate::file_access::Access;
use crate::{Result, Session};

/// A check under way, and what its end needs.
pub(crate) struct Checking {
    pub(super) pending: Pending<Looked>,
    pub(super) renewed: Renewed,
    key: Option<Key>,
    sent: bool,
    https: String,
    instance: String,
    name: String,
}

/// What the check found on the server: its branches, the one chosen, and that branch's tip (its
/// objects now in the project's repository, no ref moved), or why it could not.
pub(crate) type Looked = std::result::Result<Seen, WireError>;

pub(crate) struct Seen {
    branches: Vec<String>,
    default: Option<String>,
    branch: String,
    tip: Option<String>,
}

/// The repository asked for: its HTTPS address, the address the remote keeps (with the user name
/// given), its server, and the credential to reach it with.
struct Asked {
    https: String,
    stored: String,
    service: Service,
    instance: String,
    user: Option<String>,
    key: Option<Key>,
}

fn history(s: &Session) -> Result<&History> {
    s.history.as_ref().ok_or_else(|| "The project has no git repository yet: save it in a folder first.".to_owned())
}

fn asked(s: &Session, p: &Value) -> Result<Asked> {
    let typed = super::super::clone::typed_address(string(p, "url")?);
    let https = repository_url(&typed)?;
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
    let stored = match (&user, hosted) {
        (Some(u), None) => https.replacen("://", &format!("://{u}@"), 1),
        _ => https.clone(),
    };
    Ok(Asked { https, stored, service, instance, user, key })
}

/// The `name` parameter: `origin` by default.
fn name(p: &Value) -> String {
    p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("origin").to_owned()
}

/// The `branch` parameter, checked.
fn given_branch(p: &Value) -> Result<Option<String>> {
    match p.get("branch").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) if texcraft_forge::check_branch_name(b).is_err() => Err(format!("{b:?} cannot be a branch's name.")),
        Some(b) => Ok(Some(b.to_owned())),
        None => Ok(None),
    }
}

/// The remote's branches, and its default one (the one `HEAD` names, else the first).
fn branches(advertised: &texcraft_gitwire::Advertised) -> (Vec<String>, Option<String>) {
    let list: Vec<String> = advertised.refs.iter().filter_map(|(n, _)| n.strip_prefix("refs/heads/").map(str::to_owned)).collect();
    let default = advertised.head().and_then(|h| h.strip_prefix("refs/heads/")).filter(|h| list.iter().any(|b| b == h)).map(str::to_owned);
    let default = default.or_else(|| list.first().cloned());
    (list, default)
}

pub(super) fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let h = history(s)?;
    allowed(s, h, Access::Read)?;
    let a = asked(s, p)?;
    let wanted = given_branch(p)?;
    let h = history(s)?;
    let local = h.branch().unwrap_or_else(|| "main".to_owned());
    let tips: Vec<String> = h.head()?.into_iter().collect();
    let git_dir = h.git_dir().to_path_buf();
    let sent = a.key.is_some();
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let (filled, sent_key, https, user, service) = (slot.clone(), a.key.clone(), a.https.clone(), a.user.clone(), a.service);
    let make = move || async move {
        let credentials = match sent_key {
            Some(key) => Some(credentials(user.as_deref(), &https, service, &key.token(&*connect(), &clients, &filled).await?)),
            None => None,
        };
        let remote = Remote::new(&https, credentials);
        let http = http();
        let look = || -> Looked {
            let advertised = texcraft_gitwire::advertised(&*http, &remote, texcraft_gitwire::Service::UploadPack)?;
            let (branches, default) = branches(&advertised);
            let branch = wanted.or_else(|| default.clone()).unwrap_or(local);
            let fetched = texcraft_gitwire::fetch(&*http, &remote, &git_dir, &format!("refs/heads/{branch}"), &tips)?;
            Ok(Seen { branches, default, branch, tip: fetched.tip })
        };
        Ok(look())
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    let (key, https, instance, name) = (a.key, a.https, a.instance, name(p));
    match started.map_err(|e| e.to_string())? {
        Ok(pending) => {
            let checking = Checking { pending, renewed: slot, key, sent, https, instance, name };
            s.git_task = Some(super::super::git::Task::Link(Box::new(super::Task::Check(Box::new(checking)))));
            Ok(json!({"operation": "check_remote", "started": true}))
        }
        Err(looked) => checked(s, looked, Answer { key, sent, https, instance, name }),
    }
}

/// What a check's end needs.
pub(super) struct Answer {
    key: Option<Key>,
    sent: bool,
    https: String,
    instance: String,
    name: String,
}

impl Checking {
    pub(super) fn answer(self) -> Answer {
        let Checking { key, sent, https, instance, name, .. } = self;
        Answer { key, sent, https, instance, name }
    }
}

/// A check's outcome: what linking would do, in counts and paths; nothing changed.
pub(super) fn checked(s: &mut Session, looked: Looked, answer: Answer) -> Result<Value> {
    let Answer { key, sent, https, instance, name } = answer;
    let host = super::super::git::remote_host(&https).unwrap_or_default();
    let kind = host_kind(s, &https);
    let seen = match looked {
        Ok(seen) => seen,
        Err(WireError::Unauthorized) if !sent => {
            return Ok(
                json!({"operation": "check_remote", "reachable": true, "needs_sign_in": true, "https_url": https, "host": kind, "branches": []}),
            );
        }
        Err(e) => {
            if matches!(e, WireError::Unauthorized) {
                s.forge_tokens.remove(&instance);
            }
            return Err(words(&e, &host, &https, sent).1);
        }
    };
    if let Some(key) = key {
        s.keep_key(&instance, key);
    }
    let h = history(s)?;
    let names: Vec<String> = h.remotes()?.into_iter().map(|r| r.name).collect();
    let name_taken = h.remote_url(&name)?.is_some_and(|u| https_url(&u).ok().as_deref() != Some(https.as_str()));
    let suggested = if name_taken { suggested_name(&https, &names) } else { name.clone() };
    let mut out = json!({
        "operation": "check_remote", "reachable": true, "needs_sign_in": false, "signed_in": sent, "can_link": true,
        "https_url": https, "host": kind, "branch": seen.branch, "branches": seen.branches, "default_branch": seen.default,
        "empty": seen.tip.is_none(), "name": name, "name_taken": name_taken, "suggested_name": suggested, "remotes": names,
        "versions": 0, "related": false, "up_to_date": false, "clashes": [], "same": 0, "only_here": 0, "only_there": 0,
    });
    if let (Some(tip), Some(head)) = (&seen.tip, h.head()?) {
        let (_, behind) = h.ahead_behind(&head, tip)?;
        let compared = h.compare(&head, tip)?;
        out["versions"] = json!(behind);
        out["related"] = json!(compared.related);
        out["up_to_date"] = json!(h.is_ancestor(tip, &head)?);
        out["clashes"] = json!(compared.clashes);
        out["same"] = json!(compared.same);
        out["only_here"] = json!(compared.only_ours);
        out["only_there"] = json!(compared.only_theirs);
    }
    Ok(out)
}

/// The remote's default branch, asked of the server now (`git.add_remote` without `branch`):
/// the one its `HEAD` names, else its first, else (an empty repository) the project's.
fn default_branch(s: &mut Session, a: &Asked) -> Result<String> {
    let local = history(s)?.branch().unwrap_or_else(|| "main".to_owned());
    let (connect, clients, slot, http) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), s.git_http.clone());
    let (key, https, user, service) = (a.key.clone(), a.https.clone(), a.user.clone(), a.service);
    let filled = slot.clone();
    let answer = texcraft_forge::task::block_on(async move {
        let credentials = match key {
            Some(key) => Some(credentials(user.as_deref(), &https, service, &key.token(&*connect(), &clients, &filled).await?)),
            None => None,
        };
        let remote = Remote::new(&https, credentials);
        Ok::<_, texcraft_forge::Error>(texcraft_gitwire::advertised(&*http(), &remote, texcraft_gitwire::Service::UploadPack))
    });
    s.keep_renewed(&slot);
    match answer.map_err(|e| e.to_string())? {
        Ok(advertised) => Ok(branches(&advertised).1.unwrap_or(local)),
        Err(e) => {
            let host = super::super::git::remote_host(&a.https).unwrap_or_default();
            Err(words(&e, &host, &a.https, a.key.is_some()).1)
        }
    }
}

pub(super) fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let h = history(s)?;
    allowed(s, h, Access::Write)?;
    let a = asked(s, p)?;
    let name = name(p);
    let h = history(s)?;
    let before = h.remote_url(&name)?;
    if let Some(url) = &before
        && https_url(url).ok().as_deref() != Some(a.https.as_str())
    {
        return Err(format!(
            "The project has a remote named {name} already ({url}): give this one another name (name), or unlink that one first (git.remove_remote)."
        ));
    }
    let upstream = h.upstream()?;
    let branch = match given_branch(p)? {
        Some(b) => b,
        None => default_branch(s, &a)?,
    };
    let mut warning = None;
    if p.get("remember").and_then(Value::as_bool) == Some(true)
        && let Some(token) = given_token(p)?
    {
        let host = texcraft_forge::servers::host_of(&a.instance).to_owned();
        if let Err(e) = s.keep_git_token(&host, &token) {
            warning = Some(format!("The token is kept for this session only: the keychain could not keep it ({e})."));
        }
    }
    let h = s.history.as_mut().ok_or("The project has no git repository.")?;
    h.set_remote(&name, &a.stored)?;
    h.set_upstream(&name, &branch)?;
    let mut params = json!({"remote": name, "branch": branch});
    for k in ["token", "resolve", "background"] {
        if let Some(v) = p.get(k) {
            params[k] = v.clone();
        }
    }
    let added = json!({"name": name, "url": a.stored, "branch": branch, "warning": warning});
    match super::super::git::desktop::sync_adding(s, &params, added) {
        Ok(done) => Ok(done),
        // Files in conflict: the link stays, and a Sync with the choices made gets through.
        Err(e) if matches!(s.forge_problem, Some(Problem::Conflicts(_))) => Err(e),
        Err(e) => {
            undo(s, &name, before.as_deref(), upstream);
            Err(e)
        }
    }
}

/// Puts the remote and the upstream back as they were before a link that did not get through.
fn undo(s: &mut Session, name: &str, before: Option<&str>, upstream: Option<(String, String)>) {
    let Some(h) = s.history.as_mut() else { return };
    let removed = match before {
        Some(url) => h.set_remote(name, url),
        None => h.remove_remote(name),
    };
    let restored = match upstream {
        Some((remote, branch)) => h.set_upstream(&remote, &branch),
        None => h.clear_upstream(),
    };
    for e in [removed.err(), restored.err()].into_iter().flatten() {
        log::warn!("a link that did not get through could not be undone: {e}");
    }
}

pub(super) fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let h = history(s)?;
    allowed(s, h, Access::Write)?;
    let remotes = h.remotes()?;
    let given = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
    let name = given
        .or_else(|| h.upstream().ok().flatten().map(|u| u.0))
        .or_else(|| remotes.iter().find(|r| r.name == "origin").map(|r| r.name.clone()))
        .or_else(|| remotes.first().map(|r| r.name.clone()))
        .ok_or("The project is linked to no repository.")?;
    let url = h.remote_url(&name)?.ok_or_else(|| format!("The project has no remote {name}."))?;
    s.history.as_mut().ok_or("The project has no git repository.")?.remove_remote(&name)?;
    Ok(json!({"operation": "remove_remote", "removed": true, "name": name, "url": url}))
}
