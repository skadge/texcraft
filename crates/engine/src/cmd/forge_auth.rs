//! Signing in to GitLab and GitHub (docs/forges.md, step 4), so that opening and committing need
//! no personal access token.
//!
//! - **GitLab** (gitlab.com): OAuth with PKCE. On the desktop, `forge.sign_in` listens on the
//!   loopback address for GitLab's redirect and gives the page to open in the system's browser;
//!   the code that comes back is exchanged in the background. In the browser it gives the page to
//!   go to (the whole page goes, and comes back): the state and the verifier wait in the store
//!   meanwhile, and the shell hands the code back with `forge.sign_in_return`. It asks to read
//!   (`read_api read_user`); `forge.sign_in {write: true}` asks again to commit (`api`).
//! - **GitHub** (github.com, the desktop only): the device flow. The code to enter on GitHub's page
//!   comes from [`Session::forge_signing_in`]; GitHub is asked in the background until it is
//!   entered.
//!
//! A sign-in ([`Grant`]) is kept in the store the shell gives ([`Session::set_forge_secrets`]):
//! the system's keychain on the desktop, the tab's session storage in the browser, this session
//! only otherwise. Never in the preferences, the library, a log or a command's result. Expired
//! access tokens are renewed with the refresh token before an operation; `forge.sign_out`
//! revokes the token (GitLab) and forgets it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use texcraft_forge::oauth::{self, Clients, Grant, Pkce};
use texcraft_forge::task::Pending;
use texcraft_forge::{Service, Token};

use super::{CommandSpec, always, no_params};
use crate::ai::{SecretString, Secrets, SessionSecrets};
use crate::{Result, Session};

/// How long the desktop waits for the browser to come back from GitLab.
#[cfg(not(target_arch = "wasm32"))]
const WAIT: Duration = Duration::from_secs(10 * 60);

/// The store's entry of the GitLab sign-in waiting for the browser to come back.
const PENDING: &str = "texcraft.forge.pending";

/// Where a sign-in stands, for the interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// Starting: asking GitHub for a code, or exchanging GitLab's code.
    Starting,
    /// Waiting for the user to let TeXCraft in on `url`, in the system's browser (GitLab).
    Browser { url: String },
    /// Waiting for the user to enter `user_code` at `verification_uri` (GitHub).
    Code { user_code: String, verification_uri: String },
}

/// Why signing in to a service is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// This build has no application on the service: its client id is still the placeholder.
    NoApplication,
    /// The browser: GitHub's token endpoint does not answer web pages.
    Browser,
}

/// A sign-in running in the background.
struct SignIn {
    service: Service,
    pending: Pending<Grant>,
    stop: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
}

// A sign-in dropped (another started, or the session ended) stops waiting.
impl Drop for SignIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The session's sign-ins.
pub(crate) struct State {
    secrets: Arc<dyn Secrets>,
    /// The sign-ins kept, by instance.
    grants: BTreeMap<String, Grant>,
    /// Why the store could not be read, when it could not.
    store_error: Option<String>,
    clients: Clients,
    /// Where the desktop listens for GitLab's redirect.
    loopback: String,
    /// The browser's page, which GitLab sends back to (none on the desktop).
    redirect: Option<String>,
    /// How long one of GitHub's seconds lasts while polling (tests make it short).
    second: Duration,
    task: Option<SignIn>,
    signing_out: Option<(Service, Pending<()>)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            secrets: Arc::new(SessionSecrets::default()),
            grants: BTreeMap::new(),
            store_error: None,
            clients: Clients::default(),
            loopback: oauth::LOOPBACK.to_owned(),
            redirect: None,
            second: Duration::from_secs(1),
            task: None,
            signing_out: None,
        }
    }
}

/// The store's entry of the sign-in to `instance`.
fn handle(instance: &str) -> String {
    let host = instance.split_once("://").map_or(instance, |(_, h)| h);
    format!("texcraft.forge.{host}")
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "forge.sign_in",
            label: "Sign In to GitLab or GitHub…",
            menu: &[],
            shortcut: None,
            params: sign_in_schema,
            enabled: always,
            run: sign_in,
            changes: false,
        },
        CommandSpec {
            id: "forge.sign_in_return",
            label: "Finish Signing In",
            menu: &[],
            shortcut: None,
            params: return_schema,
            enabled: always,
            run: sign_in_return,
            changes: false,
        },
        CommandSpec {
            id: "forge.cancel_sign_in",
            label: "Cancel Signing In",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: signing_in,
            run: cancel,
            changes: false,
        },
        CommandSpec {
            id: "forge.sign_out",
            label: "Sign Out of GitLab or GitHub",
            menu: &[],
            shortcut: None,
            params: sign_out_schema,
            enabled: always,
            run: sign_out,
            changes: false,
        },
        CommandSpec {
            id: "forge.accounts",
            label: "GitLab and GitHub Accounts",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: accounts,
            changes: false,
        },
    ]
}

fn sign_in_schema() -> Value {
    json!({"type": "object", "properties": {
        "service": {"enum": ["gitlab", "github"], "description": "Where to sign in: gitlab.com (OAuth in the browser) or github.com (a code to enter on GitHub's page; the desktop only). Default: gitlab."},
        "write": {"type": "boolean", "description": "GitLab: ask for the api scope, which committing needs, instead of read_api. Default: false."}
    }, "additionalProperties": false})
}

fn return_schema() -> Value {
    json!({"type": "object", "properties": {
        "code": {"type": "string", "description": "The code GitLab sent the page back with."},
        "state": {"type": "string", "description": "The state it came back with."},
        "error": {"type": "string", "description": "GitLab's error, when the user did not let TeXCraft in."},
        "error_description": {"type": "string"}
    }, "additionalProperties": false})
}

fn sign_out_schema() -> Value {
    json!({"type": "object", "properties": {
        "service": {"enum": ["gitlab", "github"], "description": "Default: gitlab."}
    }, "additionalProperties": false})
}

fn service(p: &Value) -> Result<Service> {
    match p.get("service").and_then(Value::as_str) {
        Some(s) => Service::parse(s).ok_or_else(|| format!("service: say \"gitlab\" or \"github\", not {s:?}.")),
        None => Ok(Service::GitLab),
    }
}

fn signing_in(s: &Session) -> Result<()> {
    match s.forge_auth.task {
        Some(_) => Ok(()),
        None => Err("No sign-in is running.".to_owned()),
    }
}

/// `why` signing in to `service` is not offered, in words.
fn unavailable(service: Service, why: Unavailable) -> String {
    match why {
        Unavailable::NoApplication => {
            format!("This build has no {} application yet, so it cannot sign in: use a personal access token instead.", service.name())
        }
        Unavailable::Browser => "In the browser, GitHub needs a personal access token: GitHub's sign-in does not answer web pages.".to_owned(),
    }
}

/// Starts `make` in the background as the sign-in to `service`, in place of any other.
fn run<F, Fut>(s: &mut Session, service: Service, progress: Progress, stop: Arc<AtomicBool>, make: F) -> Result<Arc<Mutex<Progress>>>
where
    F: FnOnce(Arc<Mutex<Progress>>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = texcraft_forge::Result<Grant>> + 'static,
{
    stop_sign_in(s);
    let progress = Arc::new(Mutex::new(progress));
    let shared = progress.clone();
    let pending = texcraft_forge::task::spawn(move || make(shared))?;
    s.forge_auth.task = Some(SignIn { service, pending, stop, progress: progress.clone() });
    Ok(progress)
}

fn stop_sign_in(s: &mut Session) {
    if let Some(task) = s.forge_auth.task.take() {
        task.stop.store(true, Ordering::Relaxed);
    }
}

fn sign_in(s: &mut Session, p: &Value) -> Result<Value> {
    let service = service(p)?;
    if let Some(why) = s.forge_sign_in_unavailable(service) {
        return Err(unavailable(service, why));
    }
    let client_id = s.forge_auth.clients.ready(service).map_err(|e| e.to_string())?.to_owned();
    match service {
        Service::GitLab => gitlab(s, client_id, p.get("write").and_then(Value::as_bool).unwrap_or(false)),
        Service::GitHub => github(s, client_id),
    }
}

/// GitLab: the page to let TeXCraft in, and the redirect waited for (the desktop) or the page left
/// for it (the browser).
fn gitlab(s: &mut Session, client_id: String, write: bool) -> Result<Value> {
    let pkce = Pkce::new().map_err(|e| e.to_string())?;
    let state = oauth::new_state().map_err(|e| e.to_string())?;
    let scopes = if write { oauth::GITLAB_WRITE } else { oauth::GITLAB_READ };
    let instance = oauth::instance(Service::GitLab);
    if let Some(page) = s.forge_auth.redirect.clone() {
        let pending = SecretString::new(oauth::pending_json(&state, &pkce, &page, write, instance));
        s.forge_auth.secrets.put(PENDING, &pending).map_err(|e| format!("This page cannot keep the sign-in while it goes to GitLab: {e}"))?;
        let url = oauth::authorize_url(instance, &client_id, &page, scopes, &state, &pkce);
        return Ok(json!({"operation": "sign_in", "service": "gitlab", "navigate": url}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let listener = oauth::Loopback::bind(&s.forge_auth.loopback).map_err(|e| e.to_string())?;
        let redirect = listener.redirect_uri();
        let url = oauth::authorize_url(instance, &client_id, &redirect, scopes, &state, &pkce);
        let connect = s.forge_transport.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let make = move |progress: Arc<Mutex<Progress>>| async move {
            let code = listener.wait(&state, WAIT, &stopped)?;
            *progress.lock().unwrap_or_else(PoisonError::into_inner) = Progress::Starting;
            let transport = connect();
            let grant = oauth::exchange(&*transport, instance, &client_id, &code, &redirect, &pkce).await?;
            oauth::with_user(&*transport, grant).await
        };
        run(s, Service::GitLab, Progress::Browser { url: url.clone() }, stop, make)?;
        Ok(json!({"operation": "sign_in", "service": "gitlab", "started": true, "open": url}))
    }
    #[cfg(target_arch = "wasm32")]
    Err("This page did not say its address, which GitLab sends the browser back to.".to_owned())
}

/// GitHub: the device flow, in the background; its code comes from [`Session::forge_signing_in`].
fn github(s: &mut Session, client_id: String) -> Result<Value> {
    let connect = s.forge_transport.clone();
    let second = s.forge_auth.second;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let make = move |progress: Arc<Mutex<Progress>>| async move {
        let transport = connect();
        let device = oauth::device_code(&*transport, &client_id, oauth::GITHUB_SCOPE).await?;
        *progress.lock().unwrap_or_else(PoisonError::into_inner) =
            Progress::Code { user_code: device.user_code.clone(), verification_uri: device.verification_uri.clone() };
        let wait = |seconds: u64| {
            pause(second * u32::try_from(seconds).unwrap_or(u32::MAX), &stopped);
            std::future::ready(())
        };
        let grant = oauth::device_flow(&*transport, &client_id, &device, wait, &stopped).await?;
        oauth::with_user(&*transport, grant).await
    };
    run(s, Service::GitHub, Progress::Starting, stop, make)?;
    Ok(json!({"operation": "sign_in", "service": "github", "started": true}))
}

/// Waits `time` on the sign-in's own thread, or less when it is stopped.
fn pause(time: Duration, stop: &AtomicBool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let end = std::time::Instant::now() + time;
        while !stop.load(Ordering::Relaxed) {
            let left = end.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            std::thread::sleep(left.min(Duration::from_millis(100)));
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (time, stop);
}

/// The browser came back from GitLab: the code it brought is exchanged in the background, if its
/// state is the one this page sent.
fn sign_in_return(s: &mut Session, p: &Value) -> Result<Value> {
    let kept = s.forge_auth.secrets.get(PENDING).map_err(|e| format!("The sign-in this page kept cannot be read: {e}"))?;
    let _ = s.forge_auth.secrets.remove(PENDING);
    let Some(kept) = kept else { return Err("No sign-in was waiting on this page: sign in again.".to_owned()) };
    let kept: Value = serde_json::from_str(kept.expose()).map_err(|_| "The sign-in this page kept is damaged: sign in again.".to_owned())?;
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    if let Some(error) = p.get("error").and_then(Value::as_str) {
        let why = p.get("error_description").and_then(Value::as_str).filter(|d| !d.is_empty()).unwrap_or(error);
        return Err(format!("GitLab did not let TeXCraft in: {why}"));
    }
    let state = text(&kept, "state");
    if state.is_empty() || text(p, "state") != state {
        return Err("GitLab's answer is not for the sign-in this page started: sign in again.".to_owned());
    }
    let code = Token::new(&text(p, "code")).map_err(|_| "GitLab came back with no code: sign in again.".to_owned())?;
    let pkce = Pkce::from_verifier(&text(&kept, "verifier")).map_err(|e| e.to_string())?;
    let redirect = text(&kept, "redirect");
    let client_id = s.forge_auth.clients.ready(Service::GitLab).map_err(|e| e.to_string())?.to_owned();
    let connect = s.forge_transport.clone();
    let make = move |_: Arc<Mutex<Progress>>| async move {
        let transport = connect();
        let instance = oauth::instance(Service::GitLab);
        let grant = oauth::exchange(&*transport, instance, &client_id, &code, &redirect, &pkce).await?;
        oauth::with_user(&*transport, grant).await
    };
    run(s, Service::GitLab, Progress::Starting, Arc::new(AtomicBool::new(false)), make)?;
    Ok(json!({"operation": "sign_in", "service": "gitlab", "started": true}))
}

fn cancel(s: &mut Session, _: &Value) -> Result<Value> {
    stop_sign_in(s);
    Ok(json!({"operation": "sign_in", "cancelled": true}))
}

fn sign_out(s: &mut Session, p: &Value) -> Result<Value> {
    let service = service(p)?;
    let instance = oauth::instance(service);
    let Some(grant) = s.forge_auth.grants.remove(instance) else {
        return Err(format!("TeXCraft is not signed in to {}.", service.name()));
    };
    let removed = s.forge_auth.secrets.remove(&handle(instance));
    let mut result = json!({"operation": "sign_out", "service": service.id()});
    if let Err(e) = removed {
        result["warning"] = json!(format!("The sign-in could not be removed from the keychain: {e}"));
    }
    match service {
        Service::GitHub => {
            // GitHub lets only the application's server revoke a token: the user does it there.
            drop(grant);
            result["revoked"] = json!(false);
            result["revoke"] = json!(oauth::GITHUB_REVOKE);
        }
        Service::GitLab => {
            let client_id = s.forge_auth.clients.id(service).to_owned();
            let connect = s.forge_transport.clone();
            let pending = texcraft_forge::task::spawn(move || async move { oauth::revoke(&*connect(), &client_id, &grant).await })?;
            s.forge_auth.signing_out = Some((service, pending));
            result["revoking"] = json!(true);
        }
    }
    Ok(result)
}

/// A sign-in as `forge.accounts` and `forge.sign_in`'s outcome say it: no token.
fn account_json(grant: &Grant) -> Value {
    json!({
        "service": grant.service.id(),
        "instance": grant.instance,
        "name": grant.user.as_ref().map(|u| u.name.clone()),
        "username": grant.user.as_ref().map(|u| u.username.clone()),
        "can_commit": grant.can_commit(),
        "scopes": grant.scopes,
    })
}

fn accounts(s: &mut Session, _: &Value) -> Result<Value> {
    let available: serde_json::Map<String, Value> = [Service::GitLab, Service::GitHub]
        .into_iter()
        .map(|service| (service.id().to_owned(), s.forge_sign_in_unavailable(service).map_or(json!(true), |why| json!(unavailable(service, why)))))
        .collect();
    let signing_in = s.forge_signing_in().map(|(service, progress)| {
        let mut v = json!({"service": service.id()});
        match progress {
            Progress::Starting => v["step"] = json!("starting"),
            Progress::Browser { url } => {
                v["step"] = json!("browser");
                v["url"] = json!(url);
            }
            Progress::Code { user_code, verification_uri } => {
                v["step"] = json!("code");
                v["user_code"] = json!(user_code);
                v["verification_uri"] = json!(verification_uri);
            }
        }
        v
    });
    Ok(json!({
        "accounts": s.forge_auth.grants.values().map(account_json).collect::<Vec<_>>(),
        "sign_in": available,
        "signing_in": signing_in,
        "kept": s.forge_sign_in_kept(),
        "store_error": s.forge_auth.store_error,
    }))
}

impl Session {
    /// Keeps sign-ins in `secrets` (the system's keychain on the desktop, the tab's storage in the
    /// browser), and reads those it already holds.
    pub fn set_forge_secrets(&mut self, secrets: Arc<dyn Secrets>) {
        self.forge_auth.secrets = secrets;
        self.forge_auth.grants.clear();
        self.forge_auth.store_error = None;
        for service in [Service::GitLab, Service::GitHub] {
            let instance = oauth::instance(service);
            match self.forge_auth.secrets.get(&handle(instance)) {
                Ok(Some(kept)) => match Grant::from_stored(kept.expose()) {
                    Ok(grant) => {
                        self.forge_auth.grants.insert(instance.to_owned(), grant);
                    }
                    Err(e) => log::warn!("the sign-in to {} kept cannot be read: {e}", service.name()),
                },
                Ok(None) => {}
                Err(e) => {
                    log::warn!("the sign-ins kept cannot be read: {e}");
                    self.forge_auth.store_error = Some(e);
                }
            }
        }
    }

    /// Signs in as the applications `clients` names (tests; a build's own are its constants).
    pub fn set_forge_clients(&mut self, clients: Clients) {
        self.forge_auth.clients = clients;
    }

    /// Listens for GitLab's redirect on `addr` instead of `127.0.0.1:47321` (tests: port 0).
    pub fn set_forge_loopback(&mut self, addr: &str) {
        addr.clone_into(&mut self.forge_auth.loopback);
    }

    /// The browser's page, which GitLab sends back to after signing in: the page leaves for
    /// GitLab and comes back to it, instead of the desktop's loopback listener.
    pub fn set_forge_redirect(&mut self, page: &str) {
        self.forge_auth.redirect = Some(page.to_owned());
    }

    /// Makes GitHub's seconds between polls last `second` (tests make them short).
    pub fn set_forge_poll_second(&mut self, second: Duration) {
        self.forge_auth.second = second;
    }

    /// The sign-in to `service`'s own instance (gitlab.com, github.com), if any: who, and whether it
    /// can commit.
    pub fn forge_signed_in(&self, service: Service) -> Option<&Grant> {
        self.forge_auth.grants.get(oauth::instance(service))
    }

    pub(crate) fn forge_auth_clients_ref(&self) -> &Clients {
        &self.forge_auth.clients
    }

    /// The sign-in that reaches `instance` for `service`, if any.
    pub(crate) fn forge_grant(&self, instance: &str, service: Service) -> Option<&Grant> {
        self.forge_auth.grants.get(instance).filter(|g| g.service == service)
    }

    /// The sign-in running: its service and where it stands.
    pub fn forge_signing_in(&self) -> Option<(Service, Progress)> {
        let task = self.forge_auth.task.as_ref()?;
        Some((task.service, task.progress.lock().unwrap_or_else(PoisonError::into_inner).clone()))
    }

    /// Why signing in to `service` is not offered here, if it is not.
    pub fn forge_sign_in_unavailable(&self, service: Service) -> Option<Unavailable> {
        if !self.forge_auth.clients.has(service) {
            return Some(Unavailable::NoApplication);
        }
        let browser = cfg!(target_arch = "wasm32") || self.forge_auth.redirect.is_some();
        (service == Service::GitHub && browser).then_some(Unavailable::Browser)
    }

    /// Where sign-ins are kept: `keychain` (the desktop), `tab` (the browser's session storage),
    /// or `session` (in memory only).
    pub fn forge_sign_in_kept(&self) -> &'static str {
        match (self.forge_auth.secrets.lasting(), cfg!(target_arch = "wasm32")) {
            (true, false) => "keychain",
            (true, true) => "tab",
            (false, _) => "session",
        }
    }

    /// Keeps `grant` (renewed, or new) in place of the one for its instance. Says why when the
    /// store could not keep it (it is then kept for this session only).
    pub(crate) fn keep_grant(&mut self, grant: Grant) -> Option<String> {
        let kept = self.forge_auth.secrets.put(&handle(&grant.instance), &SecretString::new(grant.stored()));
        self.forge_auth.grants.insert(grant.instance.clone(), grant);
        kept.err().map(|e| format!("The sign-in is kept for this session only: the keychain could not keep it ({e})."))
    }

    /// The outcome of a sign-in or a sign-out that ended, if one did.
    pub(crate) fn poll_forge_auth(&mut self) -> Option<Result<Value>> {
        if let Some(task) = self.forge_auth.task.as_mut()
            && let Some(outcome) = task.pending.poll()
        {
            self.forge_auth.task = None;
            return Some(match outcome {
                Ok(grant) => {
                    let mut done = account_json(&grant);
                    done["operation"] = json!("sign_in");
                    done["kept"] = json!(self.forge_sign_in_kept());
                    if let Some(warning) = self.keep_grant(grant) {
                        done["warning"] = json!(warning);
                    }
                    Ok(done)
                }
                Err(e) => Err(e.to_string()),
            });
        }
        if let Some((service, pending)) = self.forge_auth.signing_out.as_mut()
            && let Some(outcome) = pending.poll()
        {
            let service = *service;
            self.forge_auth.signing_out = None;
            return Some(match outcome {
                Ok(()) => Ok(json!({"operation": "sign_out", "service": service.id(), "revoked": true})),
                Err(e) => Err(format!("Signed out of {} here, but {} could not be told to revoke the token: {e}", service.name(), service.name())),
            });
        }
        None
    }

    /// Whether a sign-in or a sign-out is running.
    pub fn forge_auth_busy(&self) -> bool {
        self.forge_auth.task.is_some() || self.forge_auth.signing_out.is_some()
    }
}
