//! Updates (docs/ROADMAP.md, A8; `crate::update`): `app.check_updates` asks for the latest
//! release (once a day by itself, `{"automatic": true}`, when the setting is on; at once when
//! asked), `app.update_status` says what it found and what this copy can do, `app.update_settings`
//! turns the daily check on or off, and `app.install_update` puts a new AppImage in place. A
//! session checks only once the shell has enabled it ([`Session::enable_updates`]): tests, the
//! command line and agents' sessions contact nothing.

use std::sync::Arc;

use serde_json::{Value, json};
use texcraft_forge::task::Pending;

use super::{CommandSpec, always};
use crate::update::{self, Action, Install, Release};
use crate::{Result, Session};

/// The clock, in Unix seconds (tests give theirs).
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// What the session knows about updates.
pub struct State {
    /// How the feed is reached; none until the shell enables checking.
    connect: Option<super::forge::Connect>,
    install: Install,
    /// The version this copy counts as (`TEXCRAFT_PRETEND_VERSION` can set it, to try an update).
    running: String,
    clock: Clock,
    /// The check under way, and whether the user asked for it.
    checking: Option<(Pending<std::result::Result<update::Found, String>>, bool)>,
    /// The latest release, once a check has answered.
    latest: Option<Release>,
    /// A check answered in this session.
    answered: bool,
    /// Why the last check or installation failed.
    error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    fetch: Option<Arc<dyn update::appimage::Fetch>>,
    #[cfg(not(target_arch = "wasm32"))]
    job: Option<Job>,
    /// The new version is in place; it runs once this one restarts.
    installed: bool,
}

/// An AppImage being downloaded and put in place, on its own thread.
#[cfg(not(target_arch = "wasm32"))]
struct Job {
    progress: Arc<std::sync::Mutex<(u64, u64)>>,
    outcome: std::sync::mpsc::Receiver<std::result::Result<(), String>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            connect: None,
            install: Install::Source,
            running: env!("CARGO_PKG_VERSION").to_owned(),
            clock: Arc::new(|| chrono::Utc::now().timestamp()),
            checking: None,
            latest: None,
            answered: false,
            error: None,
            #[cfg(not(target_arch = "wasm32"))]
            fetch: None,
            #[cfg(not(target_arch = "wasm32"))]
            job: None,
            installed: false,
        }
    }
}

/// What [`Session::poll_update`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A check answered; `manual` when the user asked for it.
    Checked { manual: bool },
    /// A check failed, with why.
    CheckFailed { manual: bool, error: String },
    /// The new version is in place.
    Installed,
    /// The installation failed, with why; nothing was replaced.
    InstallFailed(String),
}

impl State {
    /// How this copy was installed.
    pub fn install(&self) -> &Install {
        &self.install
    }

    /// The version this copy counts as.
    pub fn running(&self) -> &str {
        &self.running
    }

    /// Whether this session checks at all (the shell enabled it).
    pub fn enabled(&self) -> bool {
        self.connect.is_some()
    }

    pub fn checking(&self) -> bool {
        self.checking.is_some()
    }

    /// The latest release, once a check has answered (none: no release published).
    pub fn latest(&self) -> Option<&Release> {
        self.latest.as_ref()
    }

    /// A check answered in this session.
    pub fn answered(&self) -> bool {
        self.answered
    }

    /// The latest release when it is newer than this copy.
    pub fn available(&self) -> Option<&Release> {
        self.latest.as_ref().filter(|r| update::is_newer(&r.version, &self.running))
    }

    /// What updating to the newer release does here.
    pub fn action(&self) -> Option<Action> {
        self.available().map(|r| update::plan(&self.install, r, std::env::consts::ARCH))
    }

    /// The release page for this copy (the mirror's on Windows and macOS).
    pub fn page(&self) -> Option<String> {
        self.available().map(|r| update::page_for(&self.install, r))
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The download under way: bytes so far, and in all (0 when not known).
    pub fn progress(&self) -> Option<(u64, u64)> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.job.as_ref().map(|j| *j.progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// The new version is in place and runs once this copy restarts.
    pub fn installed(&self) -> bool {
        self.installed
    }

    /// The program to start after this one exits to run the new version: the AppImage, once
    /// replaced.
    pub fn restart_program(&self) -> Option<&std::path::Path> {
        match &self.install {
            Install::AppImage(file) if self.installed => Some(file),
            _ => None,
        }
    }
}

impl Session {
    /// Lets the session check for updates through this build's transport, for a copy installed
    /// as `install`; removes what an earlier update kept (the old AppImage) now that this copy runs.
    pub fn enable_updates(&mut self, install: Install) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Install::AppImage(file) = &install {
                update::appimage::forget_old(file);
            }
            const APP: craft_update::App = craft_update::App::new("texcraft", "TeXCraft", "LaTeX editor", update::GITHUB_REPO);
            if let Some(v) = craft_update::self_update::current_version(&APP, env!("CARGO_PKG_VERSION")) {
                self.update.running = v.to_string();
            }
        }
        self.update.install = install;
        self.update.connect = Some(Arc::new(texcraft_forge::transport));
    }

    /// Checks through the transports `connect` makes, for a copy installed as `install` running
    /// `running` (tests).
    pub fn set_update_transport(
        &mut self,
        install: Install,
        running: &str,
        connect: impl Fn() -> Box<dyn texcraft_forge::Transport> + Send + Sync + 'static,
    ) {
        self.update.install = install;
        self.update.running = running.to_owned();
        self.update.connect = Some(Arc::new(connect));
    }

    /// Reads the time from `clock`, in Unix seconds (tests).
    pub fn set_update_clock(&mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) {
        self.update.clock = Arc::new(clock);
    }

    /// Fetches the release's files with `fetch` instead of the network (tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_update_fetch(&mut self, fetch: Arc<dyn update::appimage::Fetch>) {
        self.update.fetch = Some(fetch);
    }

    /// What the session knows about updates.
    pub fn updates(&self) -> &State {
        &self.update
    }

    /// The end of a check or an installation under way, once there is one.
    pub fn poll_update(&mut self) -> Option<Event> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(job) = &self.update.job {
            let ended = match job.outcome.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("The update stopped unexpectedly; nothing was replaced.".to_owned())),
            };
            if let Some(outcome) = ended {
                self.update.job = None;
                return Some(match outcome {
                    Ok(()) => {
                        self.update.installed = true;
                        self.update.error = None;
                        Event::Installed
                    }
                    Err(e) => {
                        self.update.error = Some(e.clone());
                        Event::InstallFailed(e)
                    }
                });
            }
        }
        let (pending, manual) = self.update.checking.as_mut()?;
        let manual = *manual;
        let outcome = pending.poll()?;
        self.update.checking = None;
        match outcome.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(found) => {
                self.update.latest = found;
                self.update.answered = true;
                self.update.error = None;
                self.settings.updates.last_check = Some((self.update.clock)());
                Some(Event::Checked { manual })
            }
            Err(error) => {
                log::warn!("updates: {error}");
                self.update.error = Some(error.clone());
                Some(Event::CheckFailed { manual, error })
            }
        }
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "app.check_updates",
            label: "Check for Updates",
            menu: &[],
            shortcut: None,
            params: check_schema,
            enabled: always,
            run: check,
            changes: false,
        },
        CommandSpec {
            id: "app.update_status",
            label: "Update Status",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: always,
            run: status,
            changes: false,
        },
        CommandSpec {
            id: "app.update_settings",
            label: "Update Settings",
            menu: &[],
            shortcut: None,
            params: settings_schema,
            enabled: always,
            run: settings,
            changes: true,
        },
        CommandSpec {
            id: "app.install_update",
            label: "Install the Update",
            menu: &[],
            shortcut: None,
            params: super::no_params,
            enabled: can_install,
            run: install,
            changes: true,
        },
    ]
}

fn check_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "automatic": {"type": "boolean", "description": "The daily check: nothing is asked when the setting is off or the last answer is less than a day old. Without it, the check runs at once."}
        },
        "additionalProperties": false
    })
}

fn settings_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"check": {"type": "boolean", "description": "Check for updates once a day, shortly after the start."}},
        "required": ["check"],
        "additionalProperties": false
    })
}

/// Starts a check; it ends in [`Session::poll_update`].
fn check(s: &mut Session, params: &Value) -> Result<Value> {
    let automatic = params.get("automatic").and_then(Value::as_bool).unwrap_or(false);
    let Some(connect) = s.update.connect.clone() else {
        return Err("This copy of TeXCraft does not check for updates.".to_owned());
    };
    let now = (s.update.clock)();
    if automatic && !s.settings.updates.check {
        return Ok(json!({"started": false, "reason": "off"}));
    }
    if automatic && !update::due(&s.settings.updates, now) {
        return Ok(json!({"started": false, "reason": "checked_today"}));
    }
    if let Some((_, manual)) = &mut s.update.checking {
        *manual |= !automatic;
        return Ok(json!({"started": false, "reason": "checking"}));
    }
    let site = s.update.install == Install::Browser;
    let pending = texcraft_forge::task::spawn(move || async move {
        let transport = connect();
        Ok(update::latest(&*transport, site, now).await)
    })?;
    s.update.checking = Some((pending, !automatic));
    s.update.error = None;
    Ok(json!({"started": true}))
}

fn status(s: &mut Session, _: &Value) -> Result<Value> {
    let u = &s.update;
    let latest = u.latest.as_ref().map(|r| update::describe(r, &update::plan(&u.install, r, std::env::consts::ARCH)));
    Ok(json!({
        "running": u.running,
        "install": u.install.name(),
        "enabled": u.enabled(),
        "check": s.settings.updates.check,
        "lastCheck": s.settings.updates.last_check.and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|d| d.to_rfc3339()),
        "checking": u.checking(),
        "answered": u.answered,
        "latest": latest,
        "available": u.available().is_some(),
        "page": u.page(),
        "error": u.error,
        "downloading": u.progress().map(|(done, total)| json!({"done": done, "total": total})),
        "installed": u.installed,
    }))
}

fn settings(s: &mut Session, params: &Value) -> Result<Value> {
    let on = params.get("check").and_then(Value::as_bool).ok_or("the parameter `check` (true or false) is missing")?;
    s.settings.updates.check = on;
    Ok(json!({"check": on}))
}

fn can_install(s: &Session) -> Result<()> {
    let u = &s.update;
    if u.installed {
        return Err("The update is in place: restart TeXCraft to use it.".to_owned());
    }
    if u.progress().is_some() {
        return Err("The update is downloading.".to_owned());
    }
    match u.action() {
        None => Err("No newer version is known: check for updates first.".to_owned()),
        Some(Action::ReplaceAppImage { .. }) => Ok(()),
        Some(Action::Flatpak { .. }) => Err("Updates come through Flatpak: TeXCraft does not replace itself there.".to_owned()),
        Some(Action::Download { .. }) => {
            Err("TeXCraft does not replace itself on this system: download the new version from the release page.".to_owned())
        }
        Some(Action::ReleasePage) => Err("This copy cannot replace itself: get the new version from the release page.".to_owned()),
        Some(Action::Reload) => Err("Reload the page to use the new version.".to_owned()),
    }
}

/// Downloads the new AppImage and puts it in place, on its own thread; it ends in
/// [`Session::poll_update`].
fn install(s: &mut Session, _: &Value) -> Result<Value> {
    can_install(s)?;
    #[cfg(target_arch = "wasm32")]
    return Err("Reload the page to use the new version.".to_owned());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (Some(Action::ReplaceAppImage { asset, sums }), Install::AppImage(current)) = (s.update.action(), s.update.install.clone()) else {
            return Err("This copy cannot replace itself: get the new version from the release page.".to_owned());
        };
        let fetch: Arc<dyn update::appimage::Fetch> = match &s.update.fetch {
            Some(f) => f.clone(),
            None => Arc::new(update::appimage::Http::new()?),
        };
        let progress = Arc::new(std::sync::Mutex::new((0, 0)));
        let (tx, outcome) = std::sync::mpsc::channel();
        let shared = progress.clone();
        let name = asset.name.clone();
        std::thread::Builder::new()
            .name("texcraft-update".to_owned())
            .spawn(move || {
                let mut report = |done, total| *shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = (done, total);
                let _ = tx.send(update::appimage::update(&*fetch, &asset, sums.as_ref(), &current, &mut report));
            })
            .map_err(|e| format!("The update could not start: {e}"))?;
        s.update.job = Some(Job { progress, outcome });
        s.update.error = None;
        Ok(json!({"started": true, "file": name}))
    }
}
