//! Choosing a project and a branch on a forge (docs/forges.md, "Choosing a project and a
//! branch"): `forge.projects` lists the projects the account reaches whose path holds what is
//! typed, `forge.branches` a project's branches with the default one. Both work signed in or with
//! a personal access token, on gitlab.com, github.com and the servers added.
//!
//! The Open dialog asks as the user types, in the background (`background`), and
//! [`Session::poll_forge_lookup`] gives the outcome. A lookup asked while one of the same kind
//! runs waits for it and replaces any other that waited, so only the last one typed is answered
//! and the forge is not asked once per key. GitHub's list of the user's repositories is read once
//! and kept for a few minutes, so later queries filter it here and answer at once.

use serde_json::{Value, json};
use texcraft_forge::listing::{self, BranchSummary, Found, Listed, Lister, ProjectSummary};
use texcraft_forge::oauth;
use texcraft_forge::task::Pending;
use texcraft_forge::{Address, Service};

use super::forge::{Key, Renewed, background, key_at, start};
use super::forge_auth::{instance_of, service};
use super::{CommandSpec, always, string};
use crate::{Result, Session};

/// How long GitHub's list of the user's repositories is kept, in seconds.
const KEEP_LISTING: i64 = 300;

/// Which lookup an outcome is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Projects,
    Branches,
}

/// A lookup running, with what it was asked (`params`), and the slot its renewed sign-in goes in.
struct Running<T> {
    pending: Pending<T>,
    params: Value,
    renewed: Renewed,
    /// Kept for the session once it worked, as `forge.open` keeps it.
    key: Key,
    service: Service,
    instance: String,
}

/// One kind of lookup: the one running, and the last one asked meanwhile.
struct Slot<T> {
    running: Option<Running<T>>,
    wanted: Option<Value>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self { running: None, wanted: None }
    }
}

/// Whose listing a kept one is: a token's, or a signed-in user's.
#[derive(Clone, PartialEq, Eq)]
enum Who {
    Token(texcraft_forge::Token),
    User(String),
}

impl Who {
    fn of(key: &Key) -> Self {
        match key {
            Key::Token(t) => Who::Token(t.clone()),
            Key::Grant(g) => Who::User(g.user.as_ref().map(|u| u.username.clone()).unwrap_or_default()),
        }
    }
}

/// GitHub's list of the user's repositories, read at `at` (seconds since 1970).
struct Kept {
    instance: String,
    who: Who,
    at: i64,
    listing: Listed<ProjectSummary>,
}

/// The lookups of a session.
#[derive(Default)]
pub(crate) struct State {
    projects: Slot<Found>,
    branches: Slot<(Listed<BranchSummary>, Option<String>)>,
    kept: Option<Kept>,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "forge.projects",
            label: "Find Projects on GitLab or GitHub",
            menu: &[],
            shortcut: None,
            params: projects_schema,
            enabled: always,
            run: projects,
            changes: false,
        },
        CommandSpec {
            id: "forge.branches",
            label: "List a Project's Branches",
            menu: &[],
            shortcut: None,
            params: branches_schema,
            enabled: always,
            run: branches,
            changes: false,
        },
    ]
}

const SERVICE: &str = "gitlab (default) or github.";
const INSTANCE: &str = "The instance: https://gitlab.com or https://github.com (the default for the service), or a server added (forge.add_server).";
const TOKEN: &str = "A personal access token; may be left out when one was given for the instance in this session, or when signed in to it.";
const BACKGROUND: &str = "Return at once; the outcome arrives later, in the Open dialog. A lookup asked while one runs replaces any other waiting.";

fn projects_schema() -> Value {
    json!({"type": "object", "properties": {
        "service": {"enum": ["gitlab", "github"], "description": SERVICE},
        "instance": {"type": "string", "description": INSTANCE},
        "query": {"type": "string", "description": "Part of the project's path (owner/name, group/name, or a word of it); empty lists the most recently active."},
        "token": {"type": "string", "description": TOKEN},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "additionalProperties": false})
}

fn branches_schema() -> Value {
    json!({"type": "object", "properties": {
        "service": {"enum": ["gitlab", "github"], "description": SERVICE},
        "instance": {"type": "string", "description": INSTANCE},
        "project": {"type": "string", "description": "The project: group/project or owner/repository on the instance, or its address."},
        "token": {"type": "string", "description": TOKEN},
        "background": {"type": "boolean", "description": BACKGROUND}
    }, "required": ["project"], "additionalProperties": false})
}

/// The query of `p`, trimmed and no longer than a path is.
fn query(p: &Value) -> String {
    p.get("query").and_then(Value::as_str).unwrap_or_default().trim().chars().take(200).collect()
}

/// The project `p` names on its instance: a path on `instance`, or an address (which says its
/// instance itself).
fn project_address(p: &Value, service: Service, instance: &str) -> Result<Address> {
    let project = string(p, "project")?.trim();
    let bare = !project.contains("://") && !project.starts_with("git@") && !project.split('/').next().is_some_and(|first| first.contains('.'));
    let text = if bare { format!("{instance}/{project}") } else { project.to_owned() };
    Address::parse_as(&text, Some(service)).map_err(|e| e.to_string())
}

fn project_json(instance: &str, p: &ProjectSummary) -> Value {
    json!({
        "path": p.path,
        "web_url": format!("{instance}/{}", p.path),
        "visibility": p.visibility.id(),
        "private": p.visibility != listing::Visibility::Public,
        "default_branch": p.default_branch,
        "activity": p.activity.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        "description": p.description,
    })
}

fn found_json(service: Service, instance: &str, query: &str, projects: &[ProjectSummary], truncated: bool, searched: bool) -> Value {
    json!({
        "operation": "projects",
        "service": service.id(),
        "instance": instance,
        "query": query,
        "projects": projects.iter().map(|p| project_json(instance, p)).collect::<Vec<_>>(),
        "truncated": truncated,
        "searched": searched,
    })
}

fn branches_json(service: Service, instance: &str, project: &str, listed: &Listed<BranchSummary>, default: Option<&str>) -> Value {
    json!({
        "operation": "branches",
        "service": service.id(),
        "instance": instance,
        "project": project,
        "default": default,
        "branches": listed.items.iter().map(|b| json!({"name": b.name, "protected": b.protected, "default": b.default})).collect::<Vec<_>>(),
        "truncated": listed.truncated,
    })
}

/// GitHub's listing kept for `instance` and `who`, while it is fresh.
fn kept<'a>(s: &'a Session, instance: &str, who: &Who) -> Option<&'a Listed<ProjectSummary>> {
    let k = s.forge_lookups.kept.as_ref()?;
    (k.instance == instance && &k.who == who && oauth::now() - k.at < KEEP_LISTING).then_some(&k.listing)
}

fn projects(s: &mut Session, p: &Value) -> Result<Value> {
    let service = service(p)?;
    let instance = instance_of(p, service)?;
    let query = query(p);
    let key = key_at(s, p, &instance, service)?;
    let who = Who::of(&key);
    if background(p) && s.forge_lookups.projects.running.is_some() {
        s.forge_lookups.projects.wanted = Some(p.clone());
        return Ok(json!({"operation": "projects", "started": true, "queued": true, "query": query}));
    }
    let listing = if service == Service::GitHub { kept(s, &instance, &who).cloned() } else { None };
    // The kept listing answers at once, unless only a search would find what is typed.
    if let Some(listing) = &listing {
        let matched: Vec<ProjectSummary> = listing::matching(&listing.items, &query).into_iter().cloned().collect();
        if !matched.is_empty() || !listing::worth_searching(&query) {
            return Ok(found_json(service, &instance, &query, &matched, listing.truncated, false));
        }
    }
    let (connect, clients, renewed) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default());
    let (filled, sent, at, asked) = (renewed.clone(), key.clone(), instance.clone(), query.clone());
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let lister = Lister::new(connect(), service, &at, token);
        listing::find(&lister, listing, &asked).await
    };
    let started = start(p, make);
    s.keep_renewed(&renewed);
    match started.map_err(|e| e.to_string())? {
        Ok(pending) => {
            s.forge_lookups.projects.running = Some(Running { pending, params: p.clone(), renewed, key, service, instance });
            Ok(json!({"operation": "projects", "started": true, "query": query}))
        }
        Err(found) => Ok(found_projects(s, found, service, &instance, key, &query)),
    }
}

/// What a lookup of projects found: its listing kept, and the key, in words.
fn found_projects(s: &mut Session, found: Found, service: Service, instance: &str, key: Key, query: &str) -> Value {
    if let Some(listing) = found.listing {
        s.forge_lookups.kept = Some(Kept { instance: instance.to_owned(), who: Who::of(&key), at: oauth::now(), listing });
    }
    s.keep_key(instance, key);
    found_json(service, instance, query, &found.projects, found.truncated, found.searched)
}

fn branches(s: &mut Session, p: &Value) -> Result<Value> {
    let service = service(p)?;
    let instance = instance_of(p, service)?;
    let address = project_address(p, service, &instance)?;
    let key = key_at(s, p, &address.instance, service)?;
    if background(p) && s.forge_lookups.branches.running.is_some() {
        s.forge_lookups.branches.wanted = Some(p.clone());
        return Ok(json!({"operation": "branches", "started": true, "queued": true, "project": address.path}));
    }
    let (connect, clients, renewed) = (s.forge_transport.clone(), s.forge_auth_clients(), Renewed::default());
    let (filled, sent, asked) = (renewed.clone(), key.clone(), address.clone());
    let make = move || async move {
        let token = sent.token(&*connect(), &clients, &filled).await?;
        let lister = Lister::new(connect(), service, &asked.instance, token);
        lister.branches(&asked).await
    };
    let started = start(p, make);
    s.keep_renewed(&renewed);
    match started.map_err(|e| e.to_string())? {
        Ok(pending) => {
            let instance = address.instance.clone();
            s.forge_lookups.branches.running = Some(Running { pending, params: p.clone(), renewed, key, service, instance });
            Ok(json!({"operation": "branches", "started": true, "project": address.path}))
        }
        Err((listed, default)) => {
            s.keep_key(&address.instance, key);
            Ok(branches_json(service, &address.instance, &address.path, &listed, default.as_deref()))
        }
    }
}

impl Session {
    /// The outcome of a lookup running in the background (`forge.projects`, `forge.branches`),
    /// once it has ended: what the command returns when it waits, or why it failed. An outcome
    /// a later lookup of the same kind replaced is not given: that one is started instead.
    pub fn poll_forge_lookup(&mut self) -> Option<(Lookup, Result<Value>)> {
        if let Some(done) = self.poll_projects() {
            return Some((Lookup::Projects, done));
        }
        self.poll_branches().map(|done| (Lookup::Branches, done))
    }

    /// Whether a lookup runs in the background.
    pub fn forge_lookup_busy(&self) -> bool {
        self.forge_lookups.projects.running.is_some() || self.forge_lookups.branches.running.is_some()
    }

    fn poll_projects(&mut self) -> Option<Result<Value>> {
        let running = self.forge_lookups.projects.running.as_mut()?;
        let outcome = running.pending.poll()?;
        let Running { params, renewed, key, service, instance, .. } = self.forge_lookups.projects.running.take()?;
        self.keep_renewed(&renewed);
        let query = query(&params);
        // The listing is kept even when a later query replaced this one: it answers that one.
        let done = outcome.map(|found| found_projects(self, found, service, &instance, key, &query)).map_err(|e| e.to_string());
        match self.forge_lookups.projects.wanted.take() {
            Some(wanted) if wanted != params => match projects(self, &wanted) {
                Ok(v) if v["started"] == true => None,
                other => Some(other),
            },
            _ => Some(done),
        }
    }

    fn poll_branches(&mut self) -> Option<Result<Value>> {
        let running = self.forge_lookups.branches.running.as_mut()?;
        let outcome = running.pending.poll()?;
        let Running { params, renewed, key, service, instance, .. } = self.forge_lookups.branches.running.take()?;
        self.keep_renewed(&renewed);
        if outcome.is_ok() {
            self.keep_key(&instance, key);
        }
        match self.forge_lookups.branches.wanted.take() {
            Some(wanted) if wanted != params => match branches(self, &wanted) {
                Ok(v) if v["started"] == true => None,
                other => Some(other),
            },
            _ => Some(match outcome {
                Ok((listed, default)) => {
                    let project = project_address(&params, service, &instance).map(|a| a.path).unwrap_or_default();
                    Ok(branches_json(service, &instance, &project, &listed, default.as_deref()))
                }
                Err(e) => Err(e.to_string()),
            }),
        }
    }
}
