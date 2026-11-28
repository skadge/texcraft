//! Connections (docs/design/connections.md): one list of every place TeXCraft reaches for the user
//! and how, for Settings › Accounts, Import a Project, Save Project to GitLab/GitHub… and agents.
//!
//! A connection is a forge (gitlab.com, github.com, a self-managed GitLab or GitHub Enterprise
//! Server), reached by a sign-in or a personal access token, or another git server (Overleaf's,
//! any HTTPS host), reached by a token. Nothing new is stored: the rows are read from the sign-ins
//! (`forge_auth`), the tokens kept for hosts (`git.credentials`), the servers added and the tokens
//! given in this session. One of the forges is the default (`accounts.set_default`), kept in the
//! preferences by the shell.
//!
//! - `accounts.list`: the rows, never a token.
//! - `accounts.add`: a token connection; a forge's token is checked (who it is for) before it is
//!   kept. Signing in stays `forge.sign_in`, servers stay `forge.check_server`/`forge.add_server`.
//! - `accounts.disconnect`: signs out (revoking where the service allows) and forgets the tokens.
//! - `accounts.set_default`: the default connection.
//! - `accounts.remember_in_browser`: the browser's "Remember sign-ins in this browser" (every tab
//!   and later visits, or this tab only; [`crate::browser_secrets`]).
//! - `accounts.reload`: reads the kept sign-ins and tokens again, when another of the browser's
//!   tabs changed them.

use serde_json::{Value, json};
use texcraft_forge::oauth::{self, User};
use texcraft_forge::servers;
use texcraft_forge::task::Pending;
use texcraft_forge::{Service, Token};

use super::forge_auth::{forget, is_host, stop_sign_in, unavailable};
use super::{CommandSpec, always, no_params};
use crate::{Result, Session};

/// Why git servers other than the forges are not reached from the browser.
const DESKTOP_ONLY: &str = "browser: git servers do not answer web pages (no CORS), so a page cannot reach them";

/// Overleaf's git server.
const OVERLEAF_HOST: &str = "git.overleaf.com";

/// A token being checked before it is kept.
pub(crate) struct Adding {
    instance: String,
    service: Service,
    token: Token,
    pending: Pending<User>,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "accounts.list",
            label: "Connections",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: list,
            changes: false,
        },
        CommandSpec {
            id: "accounts.add",
            label: "Add a Connection with a Token",
            menu: &[],
            shortcut: None,
            params: add_schema,
            enabled: always,
            run: add,
            changes: false,
        },
        CommandSpec {
            id: "accounts.disconnect",
            label: "Disconnect",
            menu: &[],
            shortcut: None,
            params: disconnect_schema,
            enabled: always,
            run: disconnect,
            changes: false,
        },
        CommandSpec {
            id: "accounts.set_default",
            label: "Set as Default Connection",
            menu: &[],
            shortcut: None,
            params: default_schema,
            enabled: always,
            run: set_default,
            changes: false,
        },
        CommandSpec {
            id: "accounts.remember_in_browser",
            label: "Remember Sign-ins in This Browser",
            menu: &[],
            shortcut: None,
            params: remember_schema,
            enabled: remember_offered,
            run: remember_in_browser,
            changes: false,
        },
        CommandSpec {
            id: "accounts.reload",
            label: "Read the Kept Sign-ins Again",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: reload,
            changes: false,
        },
    ]
}

fn remember_schema() -> Value {
    json!({"type": "object", "properties": {
        "on": {"type": "boolean", "description": "On: sign-ins and tokens are kept in the browser's storage for this site, for every tab and later visits. Off: in this tab only, forgotten when it is closed (a shared computer); what was kept moves into this tab, and the other tabs forget it. The browser only."}
    }, "required": ["on"], "additionalProperties": false})
}

/// The setting is the browser's: elsewhere sign-ins are kept as the system keeps them.
fn remember_offered(s: &Session) -> Result<()> {
    match s.remember_in_browser_saved().0 {
        Some(_) => Ok(()),
        None => Err("Only the browser keeps sign-ins either for this tab or for the whole browser.".to_owned()),
    }
}

fn remember_in_browser(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).ok_or("on: give true or false.")?;
    remember_offered(s)?;
    s.set_remember_in_browser(on).map_err(|e| format!("The sign-ins kept could not all be moved: {e}"))?;
    Ok(json!({"operation": "remember_in_browser", "on": on, "kept": s.forge_sign_in_kept()}))
}

fn reload(s: &mut Session, _: &Value) -> Result<Value> {
    s.reload_kept();
    Ok(json!({"operation": "reload", "kept": s.forge_sign_in_kept(), "connections": rows(s).len()}))
}

fn add_schema() -> Value {
    json!({"type": "object", "properties": {
        "kind": {"enum": ["gitlab", "github", "server", "overleaf", "git"], "description": "gitlab.com, github.com, a server added with forge.add_server (give its host), Overleaf's git server, or another git server over HTTPS (give its host)."},
        "host": {"type": "string", "description": "The server's host (git.example.org): for server and git."},
        "token": {"type": "string", "description": "A personal access token (GitLab, GitHub, a server) or a git token (Overleaf, another git server). A forge's is checked first (who it is for); then it is kept in the system's keychain (the browser: its storage for this site, or this tab's when accounts.remember_in_browser is off). Never in a result, a log or the preferences."},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes as the forge operation's (add_token). The browser always works so."}
    }, "required": ["kind", "token"], "additionalProperties": false})
}

fn disconnect_schema() -> Value {
    json!({"type": "object", "properties": {
        "instance": {"type": "string", "description": "The connection's id, as accounts.list gives it (https://gitlab.com)."},
        "host": {"type": "string", "description": "Or its host (gitlab.com, git.overleaf.com)."}
    }, "additionalProperties": false})
}

fn default_schema() -> Value {
    json!({"type": "object", "properties": {
        "instance": {"type": ["string", "null"], "description": "A forge connection's id (https://gitlab.com, https://github.com, a server added); null for none."}
    }, "required": ["instance"], "additionalProperties": false})
}

/// The forges a row can be about: gitlab.com, github.com, then the servers.
fn forges(s: &Session) -> Vec<(String, Service, Option<servers::Server>)> {
    let mut out: Vec<(String, Service, Option<servers::Server>)> =
        [Service::GitLab, Service::GitHub].into_iter().map(|service| (oauth::instance(service).to_owned(), service, None)).collect();
    out.extend(s.forge_servers().into_iter().map(|server| (server.instance.clone(), server.service, Some(server))));
    out
}

/// What kind of git server `host` is: `overleaf`, or `git`.
fn git_kind(host: &str) -> &'static str {
    if host.eq_ignore_ascii_case(OVERLEAF_HOST) { "overleaf" } else { "git" }
}

fn user_json(user: &User) -> Value {
    json!({"name": user.name, "username": user.username})
}

/// A forge's row: none when it is a public forge with nothing kept, given or under way (and not
/// the default).
fn forge_row(s: &Session, instance: &str, service: Service, server: Option<&servers::Server>) -> Option<Value> {
    let host = servers::host_of(instance).to_owned();
    let grant = s.forge_grants().get(instance).filter(|g| g.service == service);
    let kept_token = s.git_hosts_saved().0.iter().any(|h| h.eq_ignore_ascii_case(&host));
    let session_token = s.forge_tokens.contains_key(instance);
    let signing = s.forge_signing_in_instance() == Some(instance);
    let default = s.forge_auth.default.as_deref() == Some(instance);
    if server.is_none() && grant.is_none() && !kept_token && !session_token && !signing && !default {
        return None;
    }
    let (state, how, kept, user) = match grant {
        _ if signing => ("signing_in", None, None, None),
        Some(g) if g.expired(oauth::now()) && !g.renewable() => {
            ("expired", Some("sign_in"), Some(s.forge_sign_in_kept()), g.user.as_ref().map(user_json))
        }
        Some(g) => ("signed_in", Some("sign_in"), Some(s.forge_sign_in_kept()), g.user.as_ref().map(user_json)),
        None if kept_token => ("token", Some("token"), Some(s.git_token_kept()), s.forge_auth.token_users.get(&host).map(user_json)),
        None if session_token => ("token", Some("token"), Some("session"), s.forge_auth.token_users.get(&host).map(user_json)),
        None => ("signed_out", None, None, None),
    };
    let sign_in = s.forge_sign_in_unavailable_at(service, instance).map_or(json!(true), |why| json!(unavailable(service, why)));
    Some(json!({
        "id": instance,
        "kind": "forge",
        "service": service.id(),
        "server": server.is_some(),
        "instance": instance,
        "host": host,
        "state": state,
        "how": how,
        "user": user,
        "can_commit": grant.map(|g| g.can_commit()),
        "scopes": grant.map(|g| g.scopes.clone()),
        "kept": kept,
        "default": default,
        "can_default": true,
        "can_create": true,
        "available": true,
        "provided": server.is_some_and(|v| v.provided),
        "removable": server.is_some_and(|v| !v.provided),
        "sign_in": sign_in,
        "connected": matches!(state, "signed_in" | "token" | "expired"),
    }))
}

/// A git server's row.
fn git_row(host: &str, kept: &str, browser: bool) -> Value {
    json!({
        "id": format!("https://{host}"),
        "kind": "git",
        "service": git_kind(host),
        "server": false,
        "instance": format!("https://{host}"),
        "host": host,
        "state": "token",
        "how": "token",
        "user": null,
        "can_commit": null,
        "scopes": null,
        "kept": kept,
        "default": false,
        "can_default": false,
        "can_create": false,
        "available": !browser,
        "why": browser.then_some(DESKTOP_ONLY),
        "provided": false,
        "removable": false,
        "sign_in": false,
        "connected": true,
    })
}

/// Every connection, forges first. Never a token.
pub(super) fn rows(s: &Session) -> Vec<Value> {
    let browser = s.accounts_in_browser();
    let forges = forges(s);
    let mut rows: Vec<Value> = forges.iter().filter_map(|(i, service, server)| forge_row(s, i, *service, server.as_ref())).collect();
    let forge_host = |h: &str| forges.iter().any(|(i, ..)| servers::host_of(i).eq_ignore_ascii_case(h));
    let mut hosts: Vec<(String, &str)> = s.git_hosts_saved().0.iter().filter(|h| !forge_host(h)).map(|h| (h.clone(), s.git_token_kept())).collect();
    for instance in s.forge_tokens.keys() {
        let host = servers::host_of(instance).to_ascii_lowercase();
        if !forge_host(&host) && !hosts.iter().any(|(h, _)| *h == host) {
            hosts.push((host, "session"));
        }
    }
    // Overleaf's first, then by host.
    hosts.sort_by(|a, b| (git_kind(&b.0) == "overleaf").cmp(&(git_kind(&a.0) == "overleaf")).then(a.0.cmp(&b.0)));
    rows.extend(hosts.iter().map(|(h, kept)| git_row(h, kept, browser)));
    rows
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let browser = s.accounts_in_browser();
    let sign_in = |service: Service| s.forge_sign_in_unavailable(service).map_or(json!(true), |why| json!(unavailable(service, why)));
    let signing_in = s.forge_signing_in().map(|(service, _)| json!({"service": service.id(), "instance": s.forge_signing_in_instance()}));
    let ghes_why = "browser: GitHub's sign-in does not answer web pages, so a GitHub Enterprise Server needs the desktop app";
    let add = json!([
        {"id": "gitlab", "available": true, "sign_in": sign_in(Service::GitLab), "commands": ["forge.sign_in", "accounts.add"]},
        {"id": "github", "available": true, "sign_in": sign_in(Service::GitHub), "commands": ["forge.sign_in", "accounts.add"]},
        {"id": "gitlab_server", "available": true, "commands": ["forge.check_server", "forge.add_server", "forge.sign_in", "accounts.add"]},
        {"id": "github_server", "available": !browser, "why": browser.then_some(ghes_why), "commands": ["forge.check_server", "forge.add_server", "forge.sign_in", "accounts.add"]},
        {"id": "overleaf", "available": !browser, "why": browser.then_some(DESKTOP_ONLY), "commands": ["accounts.add"]},
        {"id": "git", "available": !browser, "why": browser.then_some(DESKTOP_ONLY), "commands": ["accounts.add"]},
    ]);
    Ok(json!({
        "connections": rows(s),
        "default": s.forge_auth.default,
        "kept": s.forge_sign_in_kept(),
        "remember_in_browser": s.remember_in_browser_saved().0,
        "browser": browser,
        "signing_in": signing_in,
        "adding": s.forge_auth.adding.as_ref().map(|a| a.instance.clone()),
        "store_error": s.forge_store_error(),
        "server_problems": s.forge_server_problems(),
        "add": add,
    }))
}

/// The instance and service a token for `kind` (and `host`) is for; none for a git server.
fn target(s: &Session, kind: &str, host: Option<&str>) -> Result<(String, Option<Service>)> {
    let need_host = || {
        host.map(str::trim).filter(|h| !h.is_empty()).map(str::to_ascii_lowercase).ok_or_else(|| format!("host: give the server's host for {kind}."))
    };
    match kind {
        "gitlab" => Ok((oauth::instance(Service::GitLab).to_owned(), Some(Service::GitLab))),
        "github" => Ok((oauth::instance(Service::GitHub).to_owned(), Some(Service::GitHub))),
        "overleaf" => Ok((format!("https://{OVERLEAF_HOST}"), None)),
        "server" => {
            let host = need_host()?;
            let server = s.forge_servers().into_iter().find(|v| v.host().eq_ignore_ascii_case(&host));
            match server {
                Some(v) => Ok((v.instance.clone(), Some(v.service))),
                None => Err(format!(
                    "{host} is not among the servers added: add it first (forge.add_server), or connect it as another git server (kind git)."
                )),
            }
        }
        "git" => {
            let host = need_host()?;
            let host = host.split_once("://").map_or(host.as_str(), |(_, h)| h).trim_end_matches('/').to_owned();
            if !is_host(&host) {
                return Err(format!("{host} is not a server's name."));
            }
            Ok((format!("https://{host}"), None))
        }
        other => Err(format!("kind: say gitlab, github, server, overleaf or git, not {other:?}.")),
    }
}

/// Keeps a token connection: a forge's after checking who it is for.
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = super::string(p, "kind")?;
    let token = Token::new(super::string(p, "token")?).map_err(|e| e.to_string())?;
    let (instance, service) = target(s, kind, p.get("host").and_then(Value::as_str))?;
    let host = servers::host_of(&instance).to_owned();
    let Some(service) = service else {
        if s.accounts_in_browser() {
            return Err(format!("{host}: {DESKTOP_ONLY}."));
        }
        s.keep_git_token(&host, &token).map_err(|e| format!("The token for {host} could not be kept: {e}"))?;
        return Ok(json!({"operation": "add_token", "instance": instance, "host": host, "service": git_kind(&host), "kept": s.git_token_kept()}));
    };
    let connect = s.forge_transport.clone();
    let (sent, at) = (token.clone(), instance.clone());
    let make = move || async move { oauth::token_user(&*connect(), service, &at, &sent).await };
    if super::forge::background(p) {
        let pending = texcraft_forge::task::spawn(make).map_err(|e| e.to_string())?;
        s.forge_auth.adding = Some(Adding { instance: instance.clone(), service, token, pending });
        return Ok(json!({"operation": "add_token", "instance": instance, "started": true}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let checked = texcraft_forge::task::block_on(make()).map_err(|e| e.to_string());
        added(s, &instance, service, &token, checked)
    }
    #[cfg(target_arch = "wasm32")]
    Err("A token is checked in the background here.".to_owned())
}

/// The token for `instance` was checked: kept, with who it is for, or refused.
fn added(s: &mut Session, instance: &str, service: Service, token: &Token, checked: Result<User>) -> Result<Value> {
    let user = checked?;
    let host = servers::host_of(instance).to_owned();
    s.keep_git_token(&host, token).map_err(|e| format!("The token for {host} could not be kept: {e}"))?;
    if s.forge_tokens.contains_key(instance) {
        s.forge_tokens.insert(instance.to_owned(), token.clone());
    }
    s.forge_auth.token_users.insert(host.clone(), user.clone());
    Ok(json!({
        "operation": "add_token",
        "instance": instance,
        "host": host,
        "service": service.id(),
        "name": user.name,
        "username": user.username,
        "kept": s.git_token_kept(),
    }))
}

/// The outcome of a token check that ended, if one did.
pub(super) fn poll(s: &mut Session) -> Option<Result<Value>> {
    let outcome = s.forge_auth.adding.as_mut()?.pending.poll()?;
    let adding = s.forge_auth.adding.take()?;
    Some(added(s, &adding.instance, adding.service, &adding.token, outcome.map_err(|e| e.to_string())))
}

/// The instance `p` names, by `instance` or `host`.
fn named(p: &Value) -> Result<String> {
    if let Some(i) = p.get("instance").and_then(Value::as_str).map(str::trim).filter(|i| !i.is_empty()) {
        return Ok(i.trim_end_matches('/').to_ascii_lowercase());
    }
    match p.get("host").and_then(Value::as_str).map(str::trim).filter(|h| !h.is_empty()) {
        Some(h) => Ok(format!("https://{}", h.to_ascii_lowercase())),
        None => Err("Give the connection's instance or host, as accounts.list gives them.".to_owned()),
    }
}

/// Signs out of the connection and forgets its tokens. Its projects stay linked to it.
fn disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let instance = named(p)?;
    let host = servers::host_of(&instance).to_owned();
    let mut result = json!({"operation": "disconnect", "instance": instance, "host": host});
    let mut any = false;
    if s.forge_signing_in_instance() == Some(instance.as_str()) {
        stop_sign_in(s);
        any = true;
    }
    if let Some(grant) = s.forge_grants().get(&instance).cloned() {
        result["signed_out"] = forget(s, grant);
        any = true;
    }
    match s.forget_git_token(&host) {
        Ok(forgot) => {
            result["forgot_token"] = json!(forgot);
            any |= forgot;
        }
        Err(e) => result["warning"] = json!(format!("The token kept for {host} could not be removed from where it is kept: {e}")),
    }
    any |= s.forge_tokens.remove(&instance).is_some();
    s.forge_auth.token_users.remove(&host);
    if !any && result.get("warning").is_none() {
        return Err(format!("TeXCraft has no connection to {host}."));
    }
    result["projects"] = json!("Projects linked to it stay as they are; their next sync asks to sign in or for a token.");
    Ok(result)
}

fn set_default(s: &mut Session, p: &Value) -> Result<Value> {
    let instance = match p.get("instance") {
        None | Some(Value::Null) => None,
        Some(Value::String(i)) if i.trim().is_empty() => None,
        Some(Value::String(i)) => {
            let i = i.trim().trim_end_matches('/').to_ascii_lowercase();
            if !forges(s).iter().any(|(f, ..)| *f == i) {
                return Err(format!(
                    "{} cannot be the default: the default is gitlab.com, github.com or a server added (forge.add_server), which can host projects and create repositories.",
                    servers::host_of(&i)
                ));
            }
            Some(i)
        }
        Some(_) => return Err("instance: give a connection's id (a string), or null.".to_owned()),
    };
    if s.forge_auth.default != instance {
        s.forge_auth.default = instance.clone();
        s.forge_auth.default_revision += 1;
    }
    Ok(json!({"operation": "set_default", "default": instance}))
}

impl Session {
    /// The default connection, as the shell kept it (the preferences); one that is not a forge
    /// known here (a server removed meanwhile) is dropped.
    pub fn restore_default_connection(&mut self, instance: Option<String>) {
        let known = |i: &str| forges(self).iter().any(|(f, ..)| f == i);
        self.forge_auth.default = instance.map(|i| i.trim().trim_end_matches('/').to_ascii_lowercase()).filter(|i| known(i));
    }

    /// The default connection's instance, to keep (no secret in it), and a number that goes up
    /// when it changes.
    pub fn default_connection_saved(&self) -> (Option<&str>, u64) {
        (self.forge_auth.default.as_deref(), self.forge_auth.default_revision)
    }

    /// The default connection's instance and service, if one is chosen.
    pub fn default_connection(&self) -> Option<(String, Service)> {
        let default = self.forge_auth.default.as_deref()?;
        forges(self).into_iter().find(|(i, ..)| i == default).map(|(i, service, _)| (i, service))
    }

    /// Who the token kept or given for `instance` was checked to be for, in this session.
    pub fn token_user(&self, instance: &str) -> Option<&User> {
        self.forge_auth.token_users.get(servers::host_of(instance))
    }
}
