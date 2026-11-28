//! Importing a project (docs/design/import-dialog.md; docs/ROADMAP.md, C11): what the Import a
//! Project dialog offers here (`import.sources`), the check of a git address as it is typed
//! (`git.check_address`: the server's refs, read anonymously unless credentials are known or
//! given), and a clone of a git repository into a new folder (`project.clone`, stopped with
//! `project.cancel_clone`).
//!
//! A clone is real: an empty repository is made in the folder (marked as TeXCraft's, since
//! TeXCraft made it), the branch's whole history is fetched with `texcraft-gitwire` (it has no
//! shallow fetch), the branch is checked out, `origin` is set to the HTTPS address and the branch
//! follows it, so that Sync (`project.sync`) works at once. The credential used is kept for the
//! session, and in the store (the system's keychain) when asked to (`remember`), for the remote's
//! server (`texcraft.git.<host>`), where push, pull, Sync and the next import find it; the forges'
//! personal access tokens are kept so too (`forge.open`'s `remember` in the browser), listed by
//! `git.credentials`, replaced by `git.keep_credential`, removed by `git.forget_credential`. Then
//! the project opens (unless `open` is false: the window opens it itself). The history may be up to
//! [`texcraft_forge::MAX_PROJECT`] (200 MB), as a project opened from a forge.
//!
//! On the desktop only: in the browser no git protocol reaches the forges from a page, and the
//! GitLab and GitHub projects come in through their web APIs (`forge.open`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use texcraft_forge::{Service, oauth};

use super::{CommandSpec, always};
use crate::{Result, Session};

/// Why the last check or clone did not work, for the dialog to put the words under the field
/// they concern and offer the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportProblem {
    /// The server wants credentials and none were given or known: the fields for them appear.
    NeedsSignIn,
    /// The credentials given were refused.
    Unauthorized,
    /// The account cannot read the repository.
    Forbidden,
    /// No git repository at the address.
    NotFound,
    /// The server could not be reached: Try Again.
    Network,
    /// The history is larger than TeXCraft imports.
    TooLarge,
    /// The folder cannot be made or written, or is occupied.
    Folder,
    /// Anything else (the server's own message, a branch it does not have).
    Other,
}

/// Where a clone under way is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClonePhase {
    /// Asking the server for its branches.
    #[default]
    Connecting,
    /// Receiving the history.
    Receiving,
    /// Writing the files into the folder.
    Writing,
}

/// The session's import: the check or the clone running, and why the last one failed.
pub(crate) struct State {
    #[cfg(not(target_arch = "wasm32"))]
    task: Option<desktop::Task>,
    problem: Option<ImportProblem>,
    /// The largest history a clone takes, in bytes.
    limit: u64,
}

impl Default for State {
    fn default() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        return Self { task: None, problem: None, limit: texcraft_forge::MAX_PROJECT };
        #[cfg(target_arch = "wasm32")]
        Self { problem: None, limit: texcraft_forge::MAX_PROJECT }
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "import.sources",
            label: "Where a Project Can Be Imported From",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: always,
            run: sources,
            changes: false,
        },
        CommandSpec {
            id: "git.check_address",
            label: "Check a Git Address",
            menu: &[],
            shortcut: None,
            params: check_schema,
            enabled: idle,
            run: check_address,
            changes: false,
        },
        CommandSpec {
            id: "project.clone",
            label: "Import a Project from a Git Address",
            menu: &[],
            shortcut: None,
            params: clone_schema,
            enabled: can_clone,
            run: clone,
            changes: true,
        },
        CommandSpec {
            id: "project.cancel_clone",
            label: "Stop the Import",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: always,
            run: cancel,
            changes: true,
        },
        CommandSpec {
            id: "git.credentials",
            label: "Tokens Kept on This Computer",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: always,
            run: credentials,
            changes: false,
        },
        CommandSpec {
            id: "git.keep_credential",
            label: "Keep a Token for a Server",
            menu: &[],
            shortcut: None,
            params: keep_schema,
            enabled: always,
            run: keep,
            changes: true,
        },
        CommandSpec {
            id: "git.forget_credential",
            label: "Forget a Git Server's Credential",
            menu: &[],
            shortcut: None,
            params: forget_schema,
            enabled: always,
            run: forget,
            changes: true,
        },
    ]
}

const BACKGROUND: &str = "Return at once; the outcome arrives later, in the window's status line. Default: wait for it.";
const BROWSER: &str = "In the browser no git server can be reached (git servers do not answer web pages): import a GitLab or GitHub project with forge.open, or a zip file of the project with project.import.";

fn check_schema() -> Value {
    json!({"type": "object", "properties": {
        "url": {"type": "string", "description": "The repository's address: https://host/owner/name.git (a whole `git clone …` command is read too; an SSH address is answered with its HTTPS form)."},
        "username": {"type": "string", "description": "The user name, for a server that needs signing in."},
        "token": {"type": "string", "description": "A password or an access token for the server; kept for this session only (project.clone's remember keeps it longer). Without it, a token given earlier, one kept or a sign-in to the server is used, else none."},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "required": ["url"], "additionalProperties": false})
}

fn clone_schema() -> Value {
    json!({"type": "object", "properties": {
        "url": {"type": "string", "description": "The repository's address over HTTPS: https://host/owner/name.git, an Overleaf project's git address, a GitLab or GitHub project's address."},
        "folder": {"type": "string", "description": "The whole path of the new folder to clone into; it must not exist yet, or be empty."},
        "branch": {"type": "string", "description": "The branch to check out: the repository's default branch if left out."},
        "service": {"enum": ["gitlab", "github"], "description": "The forge the address is on, for a server not added (gitlab.com and github.com, and the servers added, are known by their address)."},
        "instance": {"type": "string", "description": "The forge's server (https://host), with service."},
        "username": {"type": "string", "description": "The user name for the server, when it needs one (Overleaf: git, which is the default there)."},
        "token": {"type": "string", "description": "A password or an access token for the server (Overleaf: a git authentication token; GitLab: read_repository; GitHub: Contents read-only). Without it, a token given earlier, one remembered or a sign-in to the server is used, else none (a public repository)."},
        "remember": {"type": "boolean", "description": "Keep the token for the server in the system's keychain, so that Sync, push, pull and later imports from it need none in later sessions too (default: this session only). git.credentials lists the tokens kept, git.forget_credential removes one."},
        "open": {"type": "boolean", "description": "Open the project once it is cloned (default: true). The window opens it itself, unless another project was opened meanwhile."},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "required": ["url", "folder"], "additionalProperties": false})
}

fn forget_schema() -> Value {
    json!({"type": "object", "properties": {
        "host": {"type": "string", "description": "The server whose token to forget, as git.credentials lists it (gitlab.com, github.com, git.overleaf.com…)."}
    }, "required": ["host"], "additionalProperties": false})
}

fn keep_schema() -> Value {
    json!({"type": "object", "properties": {
        "host": {"type": "string", "description": "The server the token is for, as git.credentials lists it (gitlab.com, github.com, git.overleaf.com, git.example.org)."},
        "token": {"type": "string", "description": "The token, in place of any kept for the server: a personal access token (GitLab, GitHub) or a git authentication token (Overleaf). Kept in the system's keychain (the browser: the tab's session storage); never in a result, a log or the preferences."}
    }, "required": ["host", "token"], "additionalProperties": false})
}

fn idle(s: &Session) -> Result<()> {
    match s.import_busy() {
        Some("clone") => Err("A project is being imported: wait for it to end, or stop it (project.cancel_clone).".to_owned()),
        Some(_) => Err("An address is being checked: wait for it to end.".to_owned()),
        None => Ok(()),
    }
}

fn can_clone(s: &Session) -> Result<()> {
    if s.library.is_some() || cfg!(target_arch = "wasm32") {
        return Err(BROWSER.to_owned());
    }
    match s.import_busy() {
        Some("clone") => Err("A project is being imported: wait for it to end, or stop it (project.cancel_clone).".to_owned()),
        _ => Ok(()),
    }
}

/// What the Import a Project dialog shows: each source, whether it can be used here and why not,
/// and the account or the servers it would use. Never a token.
fn sources(s: &mut Session, _: &Value) -> Result<Value> {
    let browser = s.library.is_some() || cfg!(target_arch = "wasm32");
    let account = |service: Service| {
        s.forge_grant(oauth::instance(service), service).and_then(|g| g.user.as_ref()).map(|u| json!({"name": u.name, "username": u.username}))
    };
    let servers: Vec<Value> = s
        .forge_servers()
        .into_iter()
        .filter(|v| v.service == Service::GitLab)
        .map(|v| json!({"host": v.host(), "instance": v.instance, "signed_in": s.forge_grant(&v.instance, Service::GitLab).is_some()}))
        .collect();
    let desktop_only = "browser: git servers do not answer web pages (no CORS), so a page cannot reach them";
    let unavailable = |id: &str| json!({"id": id, "available": !browser, "why": browser.then_some(desktop_only)});
    let (git_servers, _) = s.git_hosts_saved();
    Ok(json!({
        "browser": browser,
        "sources": [
            {"id": "gitlab", "available": true, "account": account(Service::GitLab)},
            {"id": "github", "available": true, "account": account(Service::GitHub), "token_only": browser},
            unavailable("overleaf"),
            {"id": "gitlab_server", "available": true, "servers": servers},
            unavailable("git"),
            {"id": "folder", "available": true},
            {"id": "zip", "available": true},
        ],
        "git_servers": git_servers,
        "clone_limit": texcraft_project::size_words(texcraft_forge::MAX_PROJECT),
    }))
}

/// The address in what was typed or pasted: trimmed, quotes and angle brackets around it dropped,
/// and from a whole `git clone [options] <address> [folder]` command, the address.
pub fn typed_address(text: &str) -> String {
    let text = text.trim();
    let words: Vec<&str> = text.split_whitespace().collect();
    let address = match words.as_slice() {
        ["git", "clone", rest @ ..] => {
            rest.iter().copied().find(|w| !w.starts_with('-') && (w.contains("://") || w.contains('@'))).unwrap_or_default()
        }
        _ => text,
    };
    address.trim_matches(|c| matches!(c, '"' | '\'' | '<' | '>' | '`')).to_owned()
}

/// Whether `address` is an SSH one (`git@host:owner/name.git`, `ssh://…`).
pub fn is_ssh(address: &str) -> bool {
    let a = address.trim();
    a.starts_with("ssh://") || (!a.contains("://") && a.split_once(':').is_some_and(|(h, _)| h.contains('@') && !h.contains('/')))
}

/// The name a repository at `address` suggests for its folder: the last part of its path, without
/// `.git`, kept to the characters a folder's name takes everywhere.
pub fn repository_name(address: &str) -> String {
    let a = typed_address(address);
    let path = match a.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, p)| p),
        // scp-like: user@host:path
        None => a.split_once(':').map_or(a.as_str(), |(_, p)| p),
    };
    let last = path.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let last = last.strip_suffix(".git").unwrap_or(last);
    let name: String = last.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect();
    let name = name.trim_matches(|c| c == '.' || c == '-').to_owned();
    if name.is_empty() { "project".to_owned() } else { name }
}

/// Whether `folder` can be cloned into: absent, or an empty folder.
pub fn folder_free(folder: &Path) -> bool {
    match std::fs::read_dir(folder) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => !folder.exists(),
    }
}

/// `parent/name` when it is free, else the first of `parent/name-2`, `parent/name-3`… that is.
pub fn free_folder(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if folder_free(&first) {
        return first;
    }
    (2..1000).map(|i| parent.join(format!("{name}-{i}"))).find(|p| folder_free(p)).unwrap_or(first)
}

fn check_address(s: &mut Session, p: &Value) -> Result<Value> {
    let typed = typed_address(super::string(p, "url")?);
    if typed.is_empty() {
        return Err("url: give the repository's address.".to_owned());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Err(BROWSER.to_owned())
    }
    #[cfg(not(target_arch = "wasm32"))]
    desktop::check(s, p, &typed)
}

fn clone(s: &mut Session, p: &Value) -> Result<Value> {
    can_clone(s)?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = p;
        Err(BROWSER.to_owned())
    }
    #[cfg(not(target_arch = "wasm32"))]
    desktop::clone(s, p)
}

fn cancel(s: &mut Session, _: &Value) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(desktop::cancel(s));
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Ok(json!({"cancelled": false}))
    }
}

fn forget(s: &mut Session, p: &Value) -> Result<Value> {
    let host = super::string(p, "host")?.trim().to_ascii_lowercase();
    let known = s.forget_git_token(&host).map_err(|e| format!("The token for {host} could not be removed from where it is kept: {e}"))?;
    s.forge_tokens.remove(&format!("https://{host}"));
    Ok(json!({"host": host, "forgotten": known}))
}

/// What kind of server `host` is, as Settings › Accounts names it: `gitlab`, `github`,
/// `overleaf`, or `git` (any other).
fn host_service(s: &Session, host: &str) -> &'static str {
    if super::git::hosting(&format!("https://{host}/")).is_some() {
        return "overleaf";
    }
    match host {
        "gitlab.com" => "gitlab",
        "github.com" => "github",
        _ => match s.forge_servers().into_iter().find(|v| v.host().eq_ignore_ascii_case(host)).map(|v| v.service) {
            Some(Service::GitLab) => "gitlab",
            Some(Service::GitHub) => "github",
            None => "git",
        },
    }
}

/// The tokens kept (Remember on this computer): by host, with the kind of server. Never a token.
fn credentials(s: &mut Session, _: &Value) -> Result<Value> {
    let hosts = s.git_hosts_saved().0.to_vec();
    let listed: Vec<Value> = hosts.iter().map(|h| json!({"host": h, "service": host_service(s, h)})).collect();
    Ok(json!({"credentials": listed, "kept": s.git_token_kept()}))
}

/// Keeps a token for a server, in place of the one kept: Settings › Accounts' Replace….
fn keep(s: &mut Session, p: &Value) -> Result<Value> {
    let host = super::string(p, "host")?.trim().to_ascii_lowercase();
    let token = texcraft_forge::Token::new(super::string(p, "token")?).map_err(|e| e.to_string())?;
    s.keep_git_token(&host, &token).map_err(|e| format!("The token for {host} could not be kept: {e}"))?;
    // The session's copy follows, so that what runs next uses the new one.
    let instance = format!("https://{host}");
    if s.forge_tokens.contains_key(&instance) {
        s.forge_tokens.insert(instance, token);
    }
    Ok(json!({"host": host, "kept": s.git_token_kept()}))
}

impl Session {
    /// The import running: `clone` or `check_address`.
    pub fn import_busy(&self) -> Option<&'static str> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.import.task.as_ref().map(desktop::Task::name);
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// Where the clone under way is.
    pub fn clone_phase(&self) -> Option<ClonePhase> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.import.task.as_ref().and_then(desktop::Task::phase);
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// Makes clones refuse histories larger than `bytes` instead of 200 MB (tests).
    pub fn set_clone_limit(&mut self, bytes: u64) {
        self.import.limit = bytes;
    }

    /// Why the last check or clone did not work; cleared by the next.
    pub fn import_problem(&self) -> Option<ImportProblem> {
        self.import.problem
    }

    /// The outcome of the check or the clone that ended, if one did (as [`Session::poll_forge`]
    /// gives it).
    pub(crate) fn poll_import(&mut self) -> Option<Result<Value>> {
        #[cfg(not(target_arch = "wasm32"))]
        return desktop::poll(self);
        #[cfg(target_arch = "wasm32")]
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod desktop;

#[cfg(test)]
mod tests {
    use super::{free_folder, is_ssh, repository_name, typed_address};

    #[test]
    fn a_pasted_clone_command_gives_its_address() {
        assert_eq!(typed_address("  git clone https://git.overleaf.com/6f2c1a9e4b7d  "), "https://git.overleaf.com/6f2c1a9e4b7d");
        assert_eq!(typed_address("git clone --branch main <https://example.org/lab/notes.git> notes"), "https://example.org/lab/notes.git");
        assert_eq!(typed_address("\"https://example.org/a.git\""), "https://example.org/a.git");
        assert!(is_ssh("git@gitlab.com:ada/paper.git") && is_ssh("ssh://git@host/x.git") && !is_ssh("https://host/x.git"));
    }

    #[test]
    fn the_folder_is_named_after_the_repository() {
        assert_eq!(repository_name("https://gitlab.com/ada/paper-2026.git"), "paper-2026");
        assert_eq!(repository_name("git@github.com:ada/thesis.git"), "thesis");
        assert_eq!(repository_name("https://git.overleaf.com/6f2c1a9e4b7d/"), "6f2c1a9e4b7d");
        assert_eq!(repository_name("https://example.org/"), "project");
    }

    #[test]
    fn an_occupied_folder_gets_a_free_name_beside_it() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("texcraft-free-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("paper"))?;
        // An empty folder is free.
        assert_eq!(free_folder(&dir, "paper"), dir.join("paper"));
        std::fs::write(dir.join("paper/main.tex"), "x")?;
        assert_eq!(free_folder(&dir, "paper"), dir.join("paper-2"));
        std::fs::remove_dir_all(&dir)
    }
}
