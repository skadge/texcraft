//! Link to a Repository (docs/design/add-remote.md; docs/ROADMAP.md, C10): an open project linked
//! to a git repository that already exists, and the two histories brought together once.
//!
//! `git.check_remote` says what linking would do and changes nothing; `git.add_remote` links and
//! brings them together; `git.remove_remote` unlinks, touching neither side.
//!
//! On the desktop the project's folder gets the remote (`origin`, or the name given), its branch
//! follows the remote's, and a Sync runs: a version of the changes, a fetch, then a push into an
//! empty repository, or a merge of the two histories (with none in common, each file is kept from
//! the side that has it, and a name on both sides with other contents is a file in conflict,
//! settled with `resolve`), and the push of it. Never a forced push.
//!
//! In the browser, where no git protocol reaches the forges, GitLab and GitHub are read through
//! their web APIs: the branch's files and the project's are brought together the same way, per
//! file, before anything changes; the project is linked to the branch, and Sync sends the rest.

use serde_json::{Value, json};
use texcraft_forge::task::Pending;
use texcraft_forge::{Address, Choice, Client, Forge, Link, Loaded, Service};

use super::forge::{Key, Renewed, start};
use super::{CommandSpec, string};
use crate::{Result, Session};

/// A check or a browser link running in the background.
pub(crate) enum Task {
    /// The desktop's check: the remote's branches, and its branch fetched (no ref moved).
    #[cfg(not(target_arch = "wasm32"))]
    Check(Box<desktop::Checking>),
    /// The browser: the branch read, to compare (`add` none) or to link with the choices given.
    Read { pending: Pending<Read>, renewed: Renewed, key: Key, add: Option<texcraft_forge::Choices> },
}

impl Task {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Task::Check(_) => "check_remote",
            Task::Read { add: None, .. } => "check_remote",
            Task::Read { add: Some(_), .. } => "add_remote",
        }
    }
}

/// What the browser read of a repository's branch.
pub(crate) enum Read {
    /// The repository has no commit yet: its page and id.
    Empty {
        address: Address,
        info: texcraft_forge::ProjectInfo,
    },
    Branch(Loaded),
    /// The project's files went as the empty repository's first commit (GitLab).
    Committed(Link),
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "git.check_remote",
            label: "What Linking to a Repository Does",
            menu: &[],
            shortcut: None,
            params: check_schema,
            enabled: can_link,
            run: check,
            changes: false,
        },
        CommandSpec {
            id: "git.add_remote",
            label: "Link to a Repository…",
            menu: &["File"],
            shortcut: None,
            params: add_schema,
            enabled: can_link,
            run: add,
            changes: true,
        },
        CommandSpec {
            id: "git.remove_remote",
            label: "Unlink from the Repository…",
            menu: &[],
            shortcut: None,
            params: remove_schema,
            enabled: can_unlink,
            run: remove,
            changes: true,
        },
    ]
}

const URL: &str = "The repository's address: https://host/owner/name.git, a forge project's page (https://gitlab.com/ada/paper), or for GitLab and GitHub its path (ada/paper) with service. An SSH address is refused with its HTTPS form.";
const BACKGROUND: &str = "Return at once; the outcome arrives later, in the window's status line. Default: wait for it (not in the browser).";
const TOKEN: &str = "A token for the repository's server (a personal access token, or Overleaf's git token); may be left out when one is kept (git.credentials) or a sign-in made for the server.";

fn common() -> Value {
    json!({
        "url": {"type": "string", "description": URL},
        "service": {"enum": ["gitlab", "github"], "description": "The forge the address is on, when it is a path or a server added."},
        "branch": {"type": "string", "description": "The repository's branch to bring together with the project's: its default branch when left out."},
        "username": {"type": "string", "description": "The user name for a git server that asks for one (it goes into the remote's address)."},
        "token": {"type": "string", "description": TOKEN},
        "name": {"type": "string", "description": "The remote's name in the project's git repository: origin by default (desktop)."},
        "background": {"type": "boolean", "description": BACKGROUND},
    })
}

fn check_schema() -> Value {
    json!({"type": "object", "properties": common(), "required": ["url"], "additionalProperties": false})
}

fn add_schema() -> Value {
    let mut properties = common();
    properties["remember"] = json!({"type": "boolean", "description": "Keep the token given for the server (the keychain, or the tab's storage)."});
    properties["resolve"] = json!({"type": "object", "additionalProperties": {"enum": ["mine", "theirs"]}, "description": "For files on both sides with other contents (git.check_remote's clashes, or forge.status's conflicts after a refusal): \"mine\" keeps the project's, \"theirs\" takes the repository's, by path."});
    json!({"type": "object", "properties": properties, "required": ["url"], "additionalProperties": false})
}

fn remove_schema() -> Value {
    json!({"type": "object", "properties": {
        "name": {"type": "string", "description": "The remote to remove: the one the branch follows by default (desktop). In the browser, the link to the forge branch goes."}
    }, "additionalProperties": false})
}

fn idle(s: &Session) -> Result<()> {
    if s.git_task.is_some() {
        return Err("A push, a pull or a link is still under way: wait for it to end.".to_owned());
    }
    match s.forge_busy() {
        Some(_) => Err("The forge is still answering the last request: wait for it to end.".to_owned()),
        None => Ok(()),
    }
}

/// A project can be linked when it has its history somewhere: a folder's repository on the
/// desktop, the browser's library; and, in the browser, no link yet.
fn can_link(s: &Session) -> Result<()> {
    idle(s)?;
    if s.library.is_some() {
        if s.forge.is_some() {
            return Err("The project is linked to a repository already: unlink it first (git.remove_remote).".to_owned());
        }
        if s.dir.is_none() {
            return Err("Keep the project in the browser first (Save), then link it.".to_owned());
        }
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    return match &s.history {
        Some(h) if !h.prefix().is_empty() => Err(
            "The project is a folder inside a larger git repository: link that repository with git itself, so that its other files are not left out."
                .to_owned(),
        ),
        Some(_) => Ok(()),
        None if s.forge.is_some() => Err("This project is linked to its forge branch already: use Sync.".to_owned()),
        None => Err("Save the project in a folder first: its history is what goes to the repository.".to_owned()),
    };
    #[cfg(target_arch = "wasm32")]
    Err("Keep the project in the browser first.".to_owned())
}

fn can_unlink(s: &Session) -> Result<()> {
    idle(s)?;
    if s.forge.is_some() {
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if s.history.as_ref().is_some_and(|h| h.remotes().is_ok_and(|r| !r.is_empty())) {
        return Ok(());
    }
    Err("The project is linked to no repository.".to_owned())
}

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    s.forge_problem = None;
    if s.library.is_some() {
        return read_branch(s, p, None);
    }
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::check(s, p);
    #[cfg(target_arch = "wasm32")]
    Err("Keep the project in the browser first.".to_owned())
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    s.forge_problem = None;
    if s.library.is_some() {
        let choices = super::forge::choices(p)?;
        return read_branch(s, p, Some(choices));
    }
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::add(s, p);
    #[cfg(target_arch = "wasm32")]
    Err("Keep the project in the browser first.".to_owned())
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(link) = s.forge.clone() {
        s.set_forge(None);
        if let Some(dir) = s.dir.clone()
            && s.library.is_some()
        {
            super::project::save_to(s, &dir)?;
        }
        return Ok(json!({"operation": "remove_remote", "removed": true, "project": link.address.path, "branch": link.branch}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    return desktop::remove(s, p);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = p;
        Err("The project is linked to no repository.".to_owned())
    }
}

/// The forge project an address names, for the browser.
fn forge_address(p: &Value) -> Result<Address> {
    let service = match p.get("service").and_then(Value::as_str) {
        Some(id) => Some(Service::parse(id).ok_or_else(|| format!("service: say \"gitlab\" or \"github\", not {id:?}."))?),
        None => None,
    };
    let typed = super::clone::typed_address(string(p, "url")?);
    Address::parse_as(&typed, service).map_err(|e| e.to_string())
}

/// The browser: reads the repository's branch through the forge's API (an empty GitLab
/// repository gets the project's files as its first commit when linking), then compares or links.
fn read_branch(s: &mut Session, p: &Value, add: Option<texcraft_forge::Choices>) -> Result<Value> {
    let address = forge_address(p)?;
    let key = super::forge::key_at(s, p, &address.instance, address.service)?;
    if add.is_some()
        && let Key::Grant(grant) = &key
        && !grant.can_commit()
    {
        s.forge_problem = Some(super::forge::Problem::NeedsWrite);
        return Err(format!(
            "TeXCraft is signed in to {} to read only: linking sends the project's files, which needs permission to write. Sign in again allowing commits (forge.sign_in with write), then link.",
            grant.service.name()
        ));
    }
    let branch = p.get("branch").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned);
    let files = if add.is_some() { Some(s.forge_files(true)?) } else { None };
    let (connect, clients, slot, sent) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default(), key.clone());
    let filled = slot.clone();
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let client = Client::new(connect(), address.clone(), token.clone());
        let info = client.project().await?;
        if branch.is_none() && info.default_branch.is_none() {
            let Some(files) = files else { return Ok(Read::Empty { address, info }) };
            if address.service == Service::GitHub {
                return Err(texcraft_forge::Error::Name(
                    "GitHub's web API cannot write into an empty repository: add a file to it on GitHub first (a README), then link again, or use the desktop app."
                        .to_owned(),
                ));
            }
            let created = texcraft_forge::create::Created {
                address: address.clone(),
                project_id: info.id,
                web_url: info.web_url.clone(),
                clone_url: address.clone_url(),
                default_branch: "main".to_owned(),
            };
            let creator = texcraft_forge::create::Creator::new(connect(), address.service, &address.instance, token);
            let link = creator.first_commit(&created, &files, super::git::SAVE_MESSAGE).await?;
            return Ok(Read::Committed(link));
        }
        texcraft_forge::open(&client, branch.as_deref()).await.map(Read::Branch)
    };
    let started = start(p, make);
    s.keep_renewed(&slot);
    match started.map_err(|e| s.forge_failed(e))? {
        Ok(pending) => {
            let adding = add.is_some();
            s.git_task = Some(super::git::Task::Link(Box::new(Task::Read { pending, renewed: slot, key, add })));
            Ok(json!({"operation": if adding { "add_remote" } else { "check_remote" }, "started": true}))
        }
        Err(read) => read_done(s, read, key, add),
    }
}

/// How the project's files compare with the branch's, by path.
struct Compared {
    clashes: Vec<String>,
    same: usize,
    only_here: usize,
    only_there: Vec<String>,
}

fn compare(here: &[(String, Vec<u8>)], there: &[(String, Vec<u8>)]) -> Compared {
    let mine: std::collections::BTreeMap<&str, &[u8]> = here.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let theirs: std::collections::BTreeMap<&str, &[u8]> = there.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let mut out = Compared { clashes: Vec::new(), same: 0, only_here: 0, only_there: Vec::new() };
    for (path, bytes) in &mine {
        match theirs.get(path) {
            Some(b) if b == bytes => out.same += 1,
            Some(_) => out.clashes.push((*path).to_owned()),
            None => out.only_here += 1,
        }
    }
    out.only_there = theirs.keys().filter(|p| !mine.contains_key(*p)).map(|p| (*p).to_owned()).collect();
    out
}

/// The browser's read ended: what linking would do, or the link made.
fn read_done(s: &mut Session, read: Read, key: Key, add: Option<texcraft_forge::Choices>) -> Result<Value> {
    match read {
        Read::Empty { address, info } => {
            s.keep_key(&address.instance, key);
            let writable = address.service == Service::GitLab;
            Ok(json!({
                "operation": "check_remote", "empty": true, "project": info.path, "web_url": info.web_url,
                "branch": Value::Null, "branches": [], "can_link": writable,
                "why": (!writable).then_some("GitHub's web API cannot write into an empty repository: add a file to it on GitHub first (a README), then link again, or use the desktop app."),
                "clashes": [], "only_there": 0, "same": 0, "only_here": s.forge_files(false)?.len(), "versions": 0,
            }))
        }
        Read::Committed(link) => {
            s.keep_key(&link.address.instance, key);
            let result = json!({
                "operation": "add_remote", "linked": true, "empty": true, "project": link.address.path, "web_url": link.web_url,
                "branch": link.branch, "commit": link.base, "taken": [], "to_send": 0,
            });
            super::git::link_project(s, link)?;
            Ok(result)
        }
        Read::Branch(loaded) => {
            s.keep_key(&loaded.link.address.instance, key);
            let here = s.forge_files(false)?;
            let compared = compare(&here, &loaded.files);
            let Some(choices) = add else {
                return Ok(json!({
                    "operation": "check_remote", "empty": false, "can_link": true,
                    "project": loaded.link.address.path, "web_url": loaded.link.web_url, "branch": loaded.link.branch,
                    "related": false, "clashes": compared.clashes, "same": compared.same,
                    "only_here": compared.only_here, "only_there": compared.only_there.len(),
                }));
            };
            let open: Vec<texcraft_forge::Conflict> = compared
                .clashes
                .iter()
                .filter(|p| !choices.contains_key(*p))
                .map(|p| texcraft_forge::Conflict { path: p.clone(), kind: texcraft_forge::ConflictKind::AddedBoth })
                .collect();
            if !open.is_empty() {
                let names: Vec<&str> = open.iter().map(|c| c.path.as_str()).collect();
                let said = format!(
                    "The project and the branch {} both have {}, with other contents. Nothing was changed: choose which version of each to keep (resolve), then link again.",
                    loaded.link.branch,
                    names.join(", ")
                );
                s.forge_problem = Some(super::forge::Problem::Conflicts(open));
                return Err(said);
            }
            let theirs: std::collections::BTreeMap<&str, &Vec<u8>> = loaded.files.iter().map(|(p, b)| (p.as_str(), b)).collect();
            let mut taken = Vec::new();
            let wanted = compared.only_there.iter().chain(compared.clashes.iter().filter(|p| choices.get(*p) == Some(&Choice::Theirs)));
            for path in wanted {
                if let Some(bytes) = theirs.get(path.as_str()) {
                    s.put_forge_file(path, (*bytes).clone())?;
                    taken.push(path.clone());
                }
            }
            if !taken.is_empty() {
                s.touch();
            }
            let link = loaded.link;
            let mut result = json!({
                "operation": "add_remote", "linked": true, "empty": false, "project": link.address.path, "web_url": link.web_url,
                "branch": link.branch, "commit": link.base, "taken": taken,
            });
            super::git::link_project(s, link)?;
            result["to_send"] = json!(s.forge_changes()?.len());
            Ok(result)
        }
    }
}

/// The outcome of the check or link that ended, if one did (as [`Session::poll_forge`] gives it).
pub(crate) fn poll(s: &mut Session) -> Option<Result<Value>> {
    let Some(super::git::Task::Link(task)) = s.git_task.as_mut() else { return None };
    let ended = match task.as_mut() {
        #[cfg(not(target_arch = "wasm32"))]
        Task::Check(c) => Ended::Checked(c.pending.poll()?),
        Task::Read { pending, .. } => Ended::Read(pending.poll()?),
    };
    let Some(super::git::Task::Link(task)) = s.git_task.take() else { return None };
    Some(match (ended, *task) {
        #[cfg(not(target_arch = "wasm32"))]
        (Ended::Checked(done), Task::Check(checking)) => {
            s.keep_renewed(&checking.renewed);
            done.map_err(|e| e.to_string()).and_then(|d| desktop::checked(s, d, checking.answer()))
        }
        (Ended::Read(done), Task::Read { renewed, key, add, .. }) => {
            s.keep_renewed(&renewed);
            done.map_err(|e| s.forge_failed(e)).and_then(|read| read_done(s, read, key, add))
        }
        #[cfg(not(target_arch = "wasm32"))]
        _ => Err(texcraft_forge::Error::Stopped.to_string()),
    })
}

enum Ended {
    #[cfg(not(target_arch = "wasm32"))]
    Checked(texcraft_forge::Result<desktop::Looked>),
    Read(texcraft_forge::Result<Read>),
}

/// Whether `name` can be a remote's name in git's configuration: letters, digits, `-`, `_`, `.`,
/// not starting with `-` or `.`.
pub fn remote_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with(['-', '.'])
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A remote's name for a repository at `https` that is not one of `taken`: `origin`, else the
/// service's (`gitlab`, `github`, `overleaf`) or the host's first word, with `-2`… while taken.
pub fn suggested_name(https: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == "origin") {
        return "origin".to_owned();
    }
    let host = super::git::remote_host(https).unwrap_or_default();
    let base = if super::git::hosting(https).is_some() {
        "overleaf".to_owned()
    } else if host == "github.com" || host.starts_with("github.") {
        "github".to_owned()
    } else if host == "gitlab.com" || host.starts_with("gitlab.") {
        "gitlab".to_owned()
    } else {
        let word: String = host
            .split('.')
            .find(|w| !w.is_empty() && *w != "www" && *w != "git")
            .unwrap_or("remote")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if word.is_empty() { "remote".to_owned() } else { word }
    };
    (1..).map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") }).find(|n| !taken.contains(n)).unwrap_or(base)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod desktop;

#[cfg(test)]
mod tests {
    use super::suggested_name;

    #[test]
    fn a_name_is_suggested_from_the_host_when_origin_is_taken() {
        assert_eq!(suggested_name("https://gitlab.com/ada/paper.git", &[]), "origin");
        let taken = vec!["origin".to_owned()];
        assert_eq!(suggested_name("https://gitlab.com/ada/paper.git", &taken), "gitlab");
        assert_eq!(suggested_name("https://git.example.org/lab/paper.git", &taken), "example");
        let both = vec!["origin".to_owned(), "github".to_owned()];
        assert_ne!(suggested_name("https://github.com/ada/paper.git", &both), "github");
    }
}
