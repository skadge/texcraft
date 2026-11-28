//! Projects from GitLab or GitHub (docs/forges.md): the Commit dialog (a project is brought in
//! through Import a Project, `import.rs`, whose GitLab and GitHub pages use [`OpenForm`]), the files in conflict with the branch (keep mine or take theirs, or a new branch with
//! a merge request, a pull request on GitHub), the project and branch in the top bar, the branch checked for new commits every two
//! minutes and when the window comes back, and the outcomes of the engine's forge operations on
//! the status line. Everything goes through `forge.open`, `forge.pull`, `forge.commit` and
//! `forge.status`.
//!
//! Signing in (docs/forges.md, step 4) comes first in the dialogs, the token second ("Use a
//! personal access token instead"): `forge.sign_in` opens GitLab's page in the system's browser
//! (the desktop) or goes to it (the browser), or shows GitHub's code to enter; `forge.sign_out` and
//! the accounts are in Settings, with the self-managed servers added there (`forge_server`).

use egui::RichText;
use serde_json::{Value, json};
use texcraft_engine::forge::{Choice, ConflictKind, Service};
use texcraft_engine::{ForgeProblem, SignInProgress, SignInUnavailable};

use crate::dialogs::{Dialog, Outcome, actions, buttons, title};
use crate::i18n::trn;
use crate::theme::Tokens;
use crate::{App, icons};

/// Import a Project's GitLab, GitHub and GitLab Server page: its fields.
pub(crate) struct OpenForm {
    /// The service of the page's card.
    pub service: Service,
    /// The server projects are looked up on: the service's public one, or (the GitLab Server
    /// card) a server added.
    pub instance: String,
    pub address: String,
    pub branch: String,
    pub token: String,
    /// The token field instead of signing in.
    pub use_token: bool,
    /// The project's suggestions and the branch list.
    pub picker: crate::forge_pick::Picker,
}

/// The Commit dialog: the changes it lists, the files in conflict with the branch, the
/// message, where the commit goes and, when the session has no token for the instance, the token.
pub(crate) struct CommitForm {
    pub service: Service,
    /// The instance the project is on.
    pub instance: String,
    pub project: String,
    pub branch: String,
    pub changes: Vec<(String, String)>,
    pub conflicts: Vec<ConflictRow>,
    pub message: String,
    pub token: Option<String>,
    /// The token field instead of signing in (when there is neither).
    pub use_token: bool,
    /// Signed in to read only: committing asks to sign in again, allowing commits.
    pub needs_write: bool,
    /// Commit to a new branch and open a merge request, instead of committing to the branch.
    pub to_new_branch: bool,
    pub new_branch: String,
    /// The branch does not take this user's commits: only a new branch is offered.
    pub cannot_push: bool,
    pub focus: bool,
}

/// A file in conflict with the branch, why, and what to do with it (nothing chosen yet: none).
pub(crate) struct ConflictRow {
    pub path: String,
    pub why: &'static str,
    pub choice: Option<Choice>,
}

/// The files in conflict after an update from the branch, to decide before it is taken in.
pub(crate) struct ResolveForm {
    /// The forge of the branch; none for a project folder's git remote.
    pub service: Option<Service>,
    pub branch: String,
    pub conflicts: Vec<ConflictRow>,
    /// What runs again with the choices: `forge.pull`, or `project.sync`.
    pub command: &'static str,
}

/// A merge request (a pull request on GitHub) just opened: its link.
pub(crate) struct MergeRequestNote {
    pub service: Service,
    pub branch: String,
    pub target: String,
    pub iid: u64,
    pub url: String,
    pub focus: bool,
}

/// How many files changed since the branch's base, as last counted: (revision and base, time,
/// count). A commit, a Sync or a pull moves the base without changing the text: counted again.
#[derive(Default)]
pub(crate) struct Count(Option<((u64, u64, String), f64, usize)>);

/// The branch checked for new commits: when it last was, whether the window had the focus, and
/// whether the operation running is such a check (it is then quiet).
#[derive(Default)]
pub(crate) struct Check {
    last: Option<f64>,
    focused: bool,
    quiet: bool,
    /// What the last check that failed said, so it is not said again every two minutes.
    said: Option<String>,
    /// The message of a commit that did not go through, to offer again.
    message: String,
    /// The branch a commit to a new branch asks to merge into.
    target: String,
    /// The service of the project being opened.
    opening: Service,
    /// The GitHub code whose page was opened, so it is opened once.
    opened_code: Option<String>,
    /// The revision of the session's servers last put in the preferences.
    servers_kept: u64,
    /// The revision of the default connection last put in the preferences.
    default_kept: u64,
    /// The revision of Remember sign-ins in this browser last put in the preferences.
    remember_kept: u64,
}

/// What the sign-in part of a dialog or of Settings shows for a service.
pub(crate) struct SignInView {
    pub service: Service,
    /// The instance: gitlab.com, github.com or a server added.
    pub instance: String,
    /// Who is signed in, and whether they can commit.
    pub user: Option<(String, String, bool)>,
    /// Why signing in is not offered, in words.
    pub unavailable: Option<&'static str>,
    pub progress: Option<SignInProgress>,
    /// Remember sign-ins in this browser, where it is a choice (the browser build).
    pub remember: Option<bool>,
}

/// What the sign-in part asks for.
pub(crate) enum SignInAction {
    SignIn {
        service: Service,
        instance: String,
        write: bool,
    },
    Cancel,
    SignOut {
        service: Service,
        instance: String,
    },
    Open(String),
    /// Add the server at this address, to sign in to it (from the Open dialog).
    AddServer {
        service: Service,
        address: String,
    },
    /// Remember sign-ins in this browser, on or off.
    Remember(bool),
}

/// Why signing in to `service` is not offered, in words.
fn unavailable_words(service: Service, why: SignInUnavailable) -> &'static str {
    match why {
        SignInUnavailable::NoApplication => {
            say(service, tl!("This build has no GitLab application yet"), tl!("This build has no GitHub application yet"))
        }
        SignInUnavailable::Browser => tl!("In the browser, GitHub needs a personal access token: GitHub's sign-in does not answer web pages."),
        SignInUnavailable::UnknownServer => tl!("TeXCraft has no application on this server yet: add it to sign in, or use a personal access token."),
    }
}

/// The sign-in state of `service` on `instance`, for drawing.
pub(crate) fn sign_in_view(app: &App, service: Service, instance: &str) -> SignInView {
    let user = app.session.forge_signed_in_at(instance).filter(|g| g.service == service).map(|g| {
        let (name, username) = g.user.as_ref().map_or_else(|| (String::new(), String::new()), |u| (u.name.clone(), u.username.clone()));
        (name, username, g.can_commit())
    });
    let progress =
        app.session.forge_signing_in().filter(|(s, _)| *s == service && app.session.forge_signing_in_instance() == Some(instance)).map(|(_, p)| p);
    let unavailable = app.session.forge_sign_in_unavailable_at(service, instance).map(|why| unavailable_words(service, why));
    let remember = app.session.remember_in_browser_saved().0;
    SignInView { service, instance: instance.to_owned(), user, unavailable, progress, remember }
}

/// The instance a project at `address` (as typed) for `service` is on; `service`'s public one
/// while the address cannot be read yet.
pub(crate) fn instance_of(address: &str, service: Service) -> String {
    match texcraft_engine::forge::Address::parse_as(address, Some(service)) {
        Ok(a) => a.instance,
        Err(_) => texcraft_engine::forge::oauth::instance(service).to_owned(),
    }
}

/// The sign-in part: who is signed in, the sign-in under way (GitLab's page, GitHub's code), or the
/// Sign in button (off, with why, when it cannot work here).
pub(crate) fn sign_in_part(ui: &mut egui::Ui, t: &Tokens, view: &SignInView, settings: bool) -> Option<SignInAction> {
    let mut action = None;
    let service = view.service;
    match (&view.progress, &view.user) {
        (Some(SignInProgress::Code { user_code, verification_uri }), _) => {
            ui.label(say(service, tl!("Enter this code on GitLab's page:"), tl!("Enter this code on GitHub's page:")));
            let code = ui.label(RichText::new(user_code).font(t.code_font()).size(t.code_size * 1.6).color(t.text));
            code.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{}: {user_code}", tl!("Code"))));
            ui.horizontal(|ui| {
                if ui.button(tl!("Copy Code")).clicked() {
                    ui.ctx().copy_text(user_code.clone());
                }
                if ui.button(say(service, tl!("Open GitLab's Page"), tl!("Open GitHub's Page"))).clicked() {
                    action = Some(SignInAction::Open(verification_uri.clone()));
                }
                if ui.button(tl!("Cancel Signing In")).clicked() {
                    action = Some(SignInAction::Cancel);
                }
            });
        }
        (Some(SignInProgress::Browser { url }), _) => {
            ui.label(say(
                service,
                tl!("Let TeXCraft in on GitLab's page, in your browser: TeXCraft is waiting for it."),
                tl!("Let TeXCraft in on GitHub's page, in your browser: TeXCraft is waiting for it."),
            ));
            ui.horizontal(|ui| {
                if ui.button(tl!("Open the Page Again")).clicked() {
                    action = Some(SignInAction::Open(url.clone()));
                }
                if ui.button(tl!("Cancel Signing In")).clicked() {
                    action = Some(SignInAction::Cancel);
                }
            });
        }
        (Some(SignInProgress::Starting), _) => {
            ui.label(say(service, tl!("Signing in to GitLab…"), tl!("Signing in to GitHub…")));
        }
        (None, Some((name, username, can_commit))) => {
            let who = say(service, tl!("Signed in to GitLab as {name} (@{username})."), tl!("Signed in to GitHub as {name} (@{username})."))
                .replace("{name}", name)
                .replace("{username}", username);
            let can = if *can_commit { tl!("TeXCraft can open and commit.") } else { tl!("TeXCraft can open; committing asks again.") };
            ui.label(format!("{who} {can}"));
            if settings && ui.button(tl!("Sign Out")).on_hover_text(say(service, tl!("Sign out of GitLab"), tl!("Sign out of GitHub"))).clicked() {
                action = Some(SignInAction::SignOut { service, instance: view.instance.clone() });
            }
        }
        (None, None) => {
            let label = say(service, tl!("Sign In with GitLab"), tl!("Sign In with GitHub"));
            let enabled = view.unavailable.is_none();
            let mut r = ui.add_enabled(enabled, egui::Button::new(RichText::new(label).font(t.strong())).min_size(egui::vec2(0.0, 30.0)));
            if let Some(why) = view.unavailable {
                r = r.on_disabled_hover_text(why);
                ui.label(RichText::new(why).font(t.small()).color(t.text_muted));
            }
            if r.clicked() {
                action = Some(SignInAction::SignIn { service, instance: view.instance.clone(), write: false });
            }
            // The browser: where the sign-in will be kept, the same setting as Settings › Accounts'
            // (which shows it on its own page).
            if let Some(mut on) = view.remember
                && enabled
                && !settings
                && ui.checkbox(&mut on, tl!("Remember sign-ins in this browser")).changed()
            {
                action = Some(SignInAction::Remember(on));
            }
        }
    }
    action
}

/// Does what the sign-in part asked for.
pub(crate) fn act(app: &mut App, action: SignInAction) {
    match action {
        SignInAction::SignIn { service, instance, write } => sign_in(app, service, &instance, write),
        SignInAction::Cancel => {
            app.run("forge.cancel_sign_in", json!({}));
        }
        SignInAction::SignOut { service, instance } => {
            if let Some(done) = app.run("forge.sign_out", json!({"service": service.id(), "instance": instance})) {
                signed_out(app, &done);
            }
        }
        SignInAction::Open(url) => app.ctx.open_url(egui::OpenUrl::new_tab(url)),
        SignInAction::Remember(on) => crate::accounts::set_remember(app, on),
        SignInAction::AddServer { service, address } => {
            let back = match app.view.dialog.take() {
                Some(Dialog::Import(form)) => crate::forge_server::Back::Import(form),
                other => {
                    app.view.dialog = other;
                    crate::forge_server::Back::Close
                }
            };
            crate::forge_server::ask_add(app, service, &address, back);
        }
    }
}

/// Signs in to `service` on `instance` (`write`: allowing commits): GitLab's page opened in the
/// browser, or the page left for it (the browser build); GitHub's code shown once it comes.
pub(crate) fn sign_in(app: &mut App, service: Service, instance: &str, write: bool) {
    let Some(done) = app.run("forge.sign_in", json!({"service": service.id(), "instance": instance, "write": write})) else { return };
    if let Some(url) = done["navigate"].as_str() {
        app.view.status = Some((tl!("Going to GitLab to sign in…").to_owned(), false));
        app.ctx.open_url(egui::OpenUrl::same_tab(url));
    } else if let Some(url) = done["open"].as_str() {
        app.view.status = Some((tl!("Sign in on GitLab's page, in your browser.").to_owned(), false));
        app.ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

/// The browser came back from GitLab with `params` (the page's query: `code` and `state`, or
/// `error`): the code is exchanged in the background.
pub(crate) fn returned(app: &mut App, params: Value) {
    if app.run("forge.sign_in_return", params).is_some() {
        app.view.status = Some((tl!("Signing in to GitLab…").to_owned(), false));
    }
}

fn signed_out(app: &mut App, done: &Value) {
    let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
    let mut said = say(service, tl!("Signed out of GitLab."), tl!("Signed out of GitHub.")).to_owned();
    let mut error = false;
    if done["revoked"] == false {
        said = format!("{said} {}", tl!("To end TeXCraft's access, remove it on GitHub, under Settings › Applications › Authorized OAuth Apps."));
    }
    if let Some(w) = done["warning"].as_str() {
        said = format!("{said} {}", crate::i18n::msg(w));
        error = true;
    }
    app.view.status = Some((said, error));
}

fn signed_in(app: &mut App, done: &Value) {
    let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
    let said = say(service, tl!("Signed in to GitLab as {name} (@{username})."), tl!("Signed in to GitHub as {name} (@{username})."))
        .replace("{name}", done["name"].as_str().unwrap_or_default())
        .replace("{username}", done["username"].as_str().unwrap_or_default());
    match done["warning"].as_str() {
        Some(w) => app.view.status = Some((format!("{said} {}", crate::i18n::msg(w)), true)),
        None => app.view.status = Some((said, false)),
    }
    // A commit that waited for the permission to write: its dialog again.
    if app.session.forge().is_some() && app.view.dialog.is_none() && !app.forge_check.message.is_empty() && done["can_commit"] == true {
        ask_commit(app);
    }
}

/// How often the branch is checked for new commits, in seconds.
const CHECK_EVERY: f64 = 120.0;

/// `gitlab`'s words or `github`'s, as `service` is: each a whole sentence, for translators.
pub(crate) fn say(service: Service, gitlab: &'static str, github: &'static str) -> &'static str {
    match service {
        Service::GitLab => gitlab,
        Service::GitHub => github,
    }
}

/// The service of the project open, GitLab when it is linked to none.
fn linked(app: &App) -> Service {
    app.session.forge().map(|l| l.address.service).unwrap_or_default()
}

/// The menu label of forge command `id` for the project open: it names its service. None for
/// another command, or when no project from a forge is open (the command's own label then).
pub(crate) fn label(app: &App, id: &str) -> Option<&'static str> {
    let service = app.session.forge()?.address.service;
    Some(match id {
        "forge.commit" => say(service, tl!("Commit to GitLab…"), tl!("Commit to GitHub…")),
        "forge.pull" => say(service, tl!("Update from GitLab"), tl!("Update from GitHub")),
        "forge.status" => say(service, tl!("GitLab Status"), tl!("GitHub Status")),
        _ => return None,
    })
}

/// A conflict's reason, in words.
fn why(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Overlap => tl!("changed on the same lines here and on the branch"),
        ConflictKind::Binary => tl!("not text, and changed here and on the branch"),
        ConflictKind::DeletedThere => tl!("changed here, deleted on the branch"),
        ConflictKind::DeletedHere => tl!("deleted here, changed on the branch"),
        ConflictKind::AddedBoth => tl!("added here and on the branch, differently"),
        ConflictKind::TooLarge => tl!("too large to merge"),
    }
}

fn conflict_rows(app: &App) -> Vec<ConflictRow> {
    app.session.forge_conflicts().iter().map(|c| ConflictRow { path: c.path.clone(), why: why(c.kind), choice: None }).collect()
}

/// The `resolve` parameter of the choices made.
fn resolve(rows: &[ConflictRow]) -> Value {
    rows.iter()
        .filter_map(|r| {
            let choice = match r.choice? {
                Choice::Mine => "mine",
                Choice::Theirs => "theirs",
            };
            Some((r.path.clone(), json!(choice)))
        })
        .collect::<serde_json::Map<String, Value>>()
        .into()
}

/// Import a Project at the GitLab or GitHub card of `service`, the address filled in
/// (`forge.open`'s way into the dialog).
pub(crate) fn ask_open_at(app: &mut App, service: Service, address: &str) {
    let source = match service {
        Service::GitLab => crate::import::Source::GitLab,
        Service::GitHub => crate::import::Source::GitHub,
    };
    crate::import::open_at(app, Some(source), address);
}

/// The service last imported from.
pub(crate) fn last_service(app: &App) -> Service {
    app.forge_check.opening
}

/// Remembers the service a project is being imported from.
pub(crate) fn set_opening(app: &mut App, service: Service) {
    app.forge_check.opening = service;
}

/// Offers the token first where signing in is not offered (the form's instance), or where a
/// token is kept for it and no sign-in is.
pub(crate) fn refresh_use_token(app: &App, form: &mut OpenForm) {
    let instance = instance_of_form(form);
    let kept = app.session.has_kept_token(&instance) && app.session.forge_signed_in_at(&instance).is_none();
    form.use_token = kept || app.session.forge_sign_in_unavailable_at(form.service, &instance).is_some();
}

/// The instance the form's project is on: the address's, else the page's server.
pub(crate) fn instance_of_form(form: &OpenForm) -> String {
    let text = form.address.trim();
    let names_host = text.contains("://") || text.starts_with("git@") || text.split('/').next().is_some_and(|first| first.contains('.'));
    if names_host { instance_of(text, form.service) } else { form.instance.clone() }
}

/// Opens the Commit dialog, with the files changed since the base.
pub(crate) fn ask_commit(app: &mut App) {
    let Some(link) = app.session.forge().cloned() else {
        let why = tl!("This project was not opened from GitLab or GitHub: use Open from GitLab or GitHub… first.");
        app.view.status = Some((why.to_owned(), true));
        return;
    };
    let changes = match app.session.forge_changes() {
        Ok(changes) => changes,
        Err(e) => {
            app.view.status = Some((e, true));
            return;
        }
    };
    let changes = changes
        .iter()
        .map(|c| {
            let what = match c.kind {
                texcraft_engine::forge::ChangeKind::Added => crate::i18n::tc("file", "added"),
                texcraft_engine::forge::ChangeKind::Modified => crate::i18n::tc("file", "changed"),
                texcraft_engine::forge::ChangeKind::Deleted => crate::i18n::tc("file", "deleted"),
                texcraft_engine::forge::ChangeKind::Moved => crate::i18n::tc("file", "moved"),
            };
            let path = match &c.from {
                Some(from) => format!("{from} → {}", c.path),
                None => c.path.clone(),
            };
            (what.to_owned(), path)
        })
        .collect();
    let token = (!app.session.has_forge_token()).then(String::new);
    let service = link.address.service;
    let use_token = app.session.forge_sign_in_unavailable_at(service, &link.address.instance).is_some();
    let needs_write = app.session.forge_needs_write();
    let cannot_push = app.session.forge_problem() == Some(&ForgeProblem::CannotPush);
    let new_branch = format!("{}-{}", link.branch, chrono::Utc::now().format("%Y%m%d-%H%M"));
    app.view.dialog = Some(Dialog::ForgeCommit(CommitForm {
        service: link.address.service,
        instance: link.address.instance.clone(),
        project: link.address.path.clone(),
        branch: link.branch.clone(),
        changes,
        conflicts: conflict_rows(app),
        message: app.forge_check.message.clone(),
        token,
        use_token,
        needs_write,
        to_new_branch: cannot_push,
        new_branch,
        cannot_push,
        focus: true,
    }));
}

/// What the status line says while the branch's new commits are taken in.
fn updating(service: Service) -> &'static str {
    say(service, tl!("Updating from GitLab…"), tl!("Updating from GitHub…"))
}

/// Update from the forge: takes in the branch's new commits, in the background.
pub(crate) fn pull(app: &mut App) {
    if app.run("forge.pull", json!({"background": true})).is_some() {
        app.forge_check.quiet = false;
        app.view.status = Some((updating(linked(app)).to_owned(), false));
    }
}

/// Opens the dialog of the files a pull found in conflict.
fn ask_resolve(app: &mut App) {
    let Some(link) = app.session.forge() else { return };
    let (service, branch) = (link.address.service, link.branch.clone());
    app.view.dialog =
        Some(Dialog::ForgeResolve(ResolveForm { service: Some(service), branch, conflicts: conflict_rows(app), command: "forge.pull" }));
}

/// Opens the dialog of the files a Sync found in conflict: choosing syncs again.
pub(crate) fn ask_resolve_sync(app: &mut App) {
    let (service, branch) = match app.session.forge() {
        Some(link) => (Some(link.address.service), link.branch.clone()),
        None => {
            let remote = app.session.execute("git.remotes", &json!({})).unwrap_or_default();
            let place = format!("{}/{}", remote["remote"].as_str().unwrap_or("origin"), remote["remote_branch"].as_str().unwrap_or_default());
            (None, place)
        }
    };
    app.view.dialog = Some(Dialog::ForgeResolve(ResolveForm { service, branch, conflicts: conflict_rows(app), command: "project.sync" }));
}

/// The open forge dialog, if any.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let browser = app.session.library().is_some();
    let mut outcome = Outcome::Open;
    let (service, instance) = match &app.view.dialog {
        Some(Dialog::ForgeCommit(form)) => (form.service, form.instance.clone()),
        _ => (Service::GitLab, texcraft_engine::forge::GITLAB_COM.to_owned()),
    };
    let view = sign_in_view(app, service, &instance);
    let mut action = None;
    let modal = crate::sheet::show(ctx, &t, "texcraft-forge-dialog", 460.0, crate::sheet::Leave::Close, |ui| match app.view.dialog.as_mut() {
        Some(Dialog::ForgeCommit(form)) => outcome = commit_form(ui, &t, form, browser, &view, &mut action),
        Some(Dialog::ForgeResolve(form)) => outcome = resolve_form(ui, &t, form),
        Some(Dialog::ForgeMergeRequest(note)) => outcome = merge_request_note(ui, &t, note),
        _ => {}
    });
    if modal.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    if let Some(action) = action {
        if let (SignInAction::SignIn { write: true, .. }, Some(Dialog::ForgeCommit(form))) = (&action, &app.view.dialog) {
            app.forge_check.message = form.message.trim().to_owned();
        }
        act(app, action);
    }
    match outcome {
        Outcome::Open | Outcome::Discard => {}
        Outcome::Cancel => app.view.dialog = None,
        Outcome::Run => match app.view.dialog.take() {
            Some(Dialog::ForgeCommit(form)) => {
                let mut params = json!({"message": form.message.trim(), "background": true});
                if let Some(token) = form.token.as_ref().filter(|_| form.use_token) {
                    params["token"] = json!(token.trim());
                }
                if form.to_new_branch {
                    params["new_branch"] = json!(form.new_branch.trim());
                } else if !form.conflicts.is_empty() {
                    params["resolve"] = resolve(&form.conflicts);
                }
                app.forge_check.message = form.message.trim().to_owned();
                app.forge_check.target = form.branch.clone();
                if app.run("forge.commit", params).is_some() {
                    app.forge_check.quiet = false;
                    app.view.status = Some((committing(form.service).to_owned(), false));
                }
            }
            Some(Dialog::ForgeResolve(form)) if form.command == "project.sync" => {
                crate::sync::start(app, json!({"resolve": resolve(&form.conflicts)}))
            }
            Some(Dialog::ForgeResolve(form)) if app.run("forge.pull", json!({"resolve": resolve(&form.conflicts), "background": true})).is_some() => {
                app.forge_check.quiet = false;
                app.view.status = Some((updating(form.service.unwrap_or_default()).to_owned(), false));
            }
            _ => {}
        },
    }
}

/// A one-line field named by the label above it (AGENTS.md rule 21).
pub(crate) fn field(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, hint: &str, password: bool) -> egui::Response {
    ui.add_space(t.unit * 2.0);
    let label = ui.label(RichText::new(label).color(t.text_muted));
    let edit = egui::TextEdit::singleline(value).password(password).hint_text(hint).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
    ui.add(edit).labelled_by(label.id)
}

/// What the status line says while a project is opened from `service`.
pub(crate) fn opening(service: Service) -> &'static str {
    say(service, tl!("Opening the project from GitLab…"), tl!("Opening the project from GitHub…"))
}

/// What the status line says while a commit goes to `service`.
fn committing(service: Service) -> &'static str {
    say(service, tl!("Committing to GitLab…"), tl!("Committing to GitHub…"))
}

/// What a token of `service` looks like.
pub(crate) fn token_hint(service: Service) -> &'static str {
    say(service, "glpat-…", "github_pat_…")
}

/// What the token is for and which permissions it needs on `service`; `kept`, where it is kept.
pub(crate) fn token_note(ui: &mut egui::Ui, t: &Tokens, kept: &str, service: Service) {
    let how = say(
        service,
        tl!("Make one in GitLab under Preferences › Access tokens. The read_api scope opens projects; committing needs the api scope."),
        tl!(
            "Make a fine-grained one on GitHub under Settings › Developer settings › Personal access tokens, for this repository. Contents read-only opens it; committing needs Contents read and write, and opening a pull request Pull requests read and write. A classic token needs the repo scope."
        ),
    );
    let text = format!("{how} {kept}");
    ui.label(RichText::new(text.trim_end()).font(t.small()).color(t.text_muted));
}

/// The sign-in part and its fallback ("Use a personal access token instead"); whether the session
/// is signed in for this dialog (then no token is needed).
pub(crate) fn sign_in_or_token(
    ui: &mut egui::Ui,
    t: &Tokens,
    view: &SignInView,
    use_token: &mut bool,
    applies: bool,
    action: &mut Option<SignInAction>,
) -> bool {
    if !applies {
        *use_token = true;
        return false;
    }
    ui.add_space(t.unit * 2.0);
    if let Some(a) = sign_in_part(ui, t, view, false) {
        *action = Some(a);
    }
    let signed_in = view.user.is_some();
    if view.unavailable.is_none() || signed_in {
        let label = if signed_in { tl!("Use a personal access token instead of the sign-in") } else { tl!("Use a personal access token instead") };
        ui.checkbox(use_token, label);
    } else {
        *use_token = true;
    }
    signed_in && !*use_token
}

fn commit_form(ui: &mut egui::Ui, t: &Tokens, form: &mut CommitForm, browser: bool, view: &SignInView, action: &mut Option<SignInAction>) -> Outcome {
    title(ui, t, say(form.service, tl!("Commit to GitLab"), tl!("Commit to GitHub")));
    let to = if form.cannot_push {
        say(
            form.service,
            tl!(
                "The branch {branch} of {project} does not take your commits: it is protected, or your role cannot push to it. Your changes go to a new branch instead, with a merge request into {branch}."
            ),
            tl!(
                "The branch {branch} of {project} does not take your commits: it is protected, or your access cannot push to it. Your changes go to a new branch instead, with a pull request into {branch}."
            ),
        )
    } else {
        tl!("To the branch {branch} of {project}.")
    };
    ui.label(to.replace("{branch}", &form.branch).replace("{project}", &form.project));
    ui.add_space(t.unit * 2.0);
    if form.changes.is_empty() {
        ui.label(RichText::new(tl!("Nothing to commit: the files are as on the branch.")).color(t.text_muted));
        let mut outcome = Outcome::Open;
        ui.add_space(t.unit * 4.0);
        actions(ui, |ui| {
            if crate::dialogs::secondary(ui, tl!("Close"), true).clicked() {
                outcome = Outcome::Cancel;
            }
        });
        return outcome;
    }
    ui.label(RichText::new(trn(form.changes.len(), "{n} changed file", "{n} changed files")).font(t.strong()));
    egui::ScrollArea::vertical().id_salt("forge-changes").max_height(140.0).auto_shrink([false, true]).show(ui, |ui| {
        for (what, path) in &form.changes {
            ui.horizontal(|ui| {
                ui.label(RichText::new(what).font(t.small()).color(t.text_muted));
                ui.label(RichText::new(path).font(t.small()));
            });
        }
    });
    if !form.conflicts.is_empty() {
        ui.add_space(t.unit * 2.0);
        conflicts(ui, t, &mut form.conflicts, !form.to_new_branch, Some(form.service));
    }
    ui.add_space(t.unit * 2.0);
    let label = say(form.service, tl!("Commit to a new branch and open a merge request"), tl!("Commit to a new branch and open a pull request"));
    ui.add_enabled(!form.cannot_push, egui::Checkbox::new(&mut form.to_new_branch, label));
    let mut responses = Vec::new();
    if form.to_new_branch {
        responses.push(field(ui, t, tl!("New branch"), &mut form.new_branch, "", false));
    }
    let message = field(ui, t, tl!("What this commit changes"), &mut form.message, "", false);
    if std::mem::take(&mut form.focus) {
        message.request_focus();
    }
    responses.push(message);
    // Signed in since the dialog opened: no token needed any more.
    if form.token.is_some() && view.user.is_some() && !form.use_token {
        form.token = None;
    }
    let mut needs_write = form.needs_write && view.user.as_ref().is_some_and(|u| !u.2);
    if needs_write {
        ui.add_space(t.unit * 2.0);
        ui.label(RichText::new(tl!(
            "You signed in to GitLab to read only. To commit, GitLab asks you to let TeXCraft write too (the api scope); your message is kept."
        )));
        if view.progress.is_some() {
            if let Some(a) = sign_in_part(ui, t, view, false) {
                *action = Some(a);
            }
        } else if ui.button(RichText::new(tl!("Allow Commits on GitLab")).font(t.strong())).clicked() {
            *action = Some(SignInAction::SignIn { service: form.service, instance: form.instance.clone(), write: true });
        }
        if ui.checkbox(&mut form.use_token, tl!("Use a personal access token instead")).changed() && form.use_token {
            form.token = Some(String::new());
        }
        needs_write = !form.use_token;
    } else if form.token.is_some() {
        sign_in_or_token(ui, t, view, &mut form.use_token, true, action);
    }
    if let Some(token) = form.token.as_mut().filter(|_| form.use_token) {
        responses.push(field(ui, t, tl!("Personal access token"), token, token_hint(form.service), true));
        ui.add_space(t.unit);
        let kept = if browser {
            tl!("Kept while this page is open; a token kept for this server (Settings › Accounts) is replaced by it.")
        } else {
            tl!("Kept while TeXCraft runs; a token kept for this server (Settings › Accounts) is replaced by it.")
        };
        token_note(ui, t, kept, form.service);
    }
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let decided = if form.to_new_branch { !form.new_branch.trim().is_empty() } else { form.conflicts.iter().all(|c| c.choice.is_some()) };
    let has_key = match &form.token {
        None => true,
        Some(token) => form.use_token && !token.trim().is_empty(),
    };
    let ready = decided && !form.message.trim().is_empty() && has_key && !needs_write;
    let action = if form.to_new_branch {
        say(form.service, tl!("Commit and Open Merge Request"), tl!("Commit and Open Pull Request"))
    } else {
        crate::i18n::tc("button", "Commit")
    };
    match buttons(ui, t, action, false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}

/// The files in conflict, each with Keep mine and Take theirs (AGENTS.md rule 21: each named by
/// its label and its file).
fn conflicts(ui: &mut egui::Ui, t: &Tokens, rows: &mut [ConflictRow], enabled: bool, service: Option<Service>) {
    ui.label(RichText::new(trn(rows.len(), "{n} file in conflict with the branch", "{n} files in conflict with the branch")).font(t.strong()));
    let note = match service {
        Some(service) => say(
            service,
            tl!(
                "The branch has new commits that change the same parts of these files. For each, keep your version or take the branch's; or commit to a new branch and open a merge request, where GitLab shows both."
            ),
            tl!(
                "The branch has new commits that change the same parts of these files. For each, keep your version or take the branch's; or commit to a new branch and open a pull request, where GitHub shows both."
            ),
        ),
        None => tl!("The remote has new commits that change the same parts of these files. For each, keep your version or take the remote's."),
    };
    ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
    egui::ScrollArea::vertical().id_salt("forge-conflicts").max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
        for row in rows.iter_mut() {
            ui.add_space(t.unit);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&row.path).font(t.strong()));
                ui.label(RichText::new(row.why).font(t.small()).color(t.text_muted));
            });
            ui.horizontal(|ui| {
                for (choice, label) in
                    [(Choice::Mine, crate::i18n::tc("file", "Keep mine")), (Choice::Theirs, crate::i18n::tc("file", "Take theirs"))]
                {
                    let on = row.choice == Some(choice);
                    let r = ui.add_enabled(enabled, egui::RadioButton::new(on, label));
                    let name = format!("{label}: {}", row.path);
                    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, on, &name));
                    if r.clicked() {
                        row.choice = Some(choice);
                    }
                }
            });
        }
    });
}

fn resolve_form(ui: &mut egui::Ui, t: &Tokens, form: &mut ResolveForm) -> Outcome {
    let sync = form.command == "project.sync";
    let heading = match form.service {
        _ if sync => tl!("Sync"),
        Some(service) => say(service, tl!("Update from GitLab"), tl!("Update from GitHub")),
        None => tl!("Sync"),
    };
    title(ui, t, heading);
    ui.label(tl!("Nothing was taken in from the branch {branch} yet: choose for each file below first.").replace("{branch}", &form.branch));
    ui.add_space(t.unit * 2.0);
    conflicts(ui, t, &mut form.conflicts, true, form.service);
    let ready = form.conflicts.iter().all(|c| c.choice.is_some());
    buttons(ui, t, if sync { tl!("Sync") } else { tl!("Update") }, false, ready)
}

fn merge_request_note(ui: &mut egui::Ui, t: &Tokens, note: &mut MergeRequestNote) -> Outcome {
    title(ui, t, say(note.service, tl!("Merge Request Opened"), tl!("Pull Request Opened")));
    let said = say(
        note.service,
        tl!("Your changes are on the new branch {branch}, in merge request !{iid} into {target}. The project now follows {branch}."),
        tl!("Your changes are on the new branch {branch}, in pull request #{iid} into {target}. The project now follows {branch}."),
    )
    .replace("{branch}", &note.branch)
    .replace("{iid}", &note.iid.to_string())
    .replace("{target}", &note.target);
    ui.label(said);
    ui.add_space(t.unit * 2.0);
    let link = ui.hyperlink_to(say(note.service, tl!("Open the merge request on GitLab"), tl!("Open the pull request on GitHub")), &note.url);
    if std::mem::take(&mut note.focus) {
        link.request_focus();
    }
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        if crate::dialogs::secondary(ui, tl!("Close"), true).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}

/// Takes the outcome of a forge operation that ended, and keeps the window looking while one runs.
pub(crate) fn poll(app: &mut App, ctx: &egui::Context) {
    let was = app.session.forge_busy().or(app.session.git_busy()).or(app.session.import_busy());
    match app.session.poll_forge() {
        Some(Ok(done)) => {
            // A Sync of a forge branch ends with its commit or its pull.
            let forge_ended = matches!(done["operation"].as_str(), Some("commit" | "pull"));
            app.forge_check.said = None;
            app.forge_check.last = Some(ctx.input(|i| i.time));
            match done["operation"].as_str() {
                Some("check_server") => crate::forge_server::checked(app, Ok(done)),
                Some("sign_in") => {
                    signed_in(app, &done);
                    crate::accounts::signed_in(app, &done);
                }
                Some("sign_out") => signed_out(app, &done),
                Some("add_token") => crate::accounts::added(app, &done),
                Some("open") => opened(app, &done),
                Some("pull") => pulled(app, &done),
                Some("create_repository") => crate::forge_save::saved(app, &done),
                Some("owners") => {
                    crate::forge_save::owners_listed(app, &done);
                    crate::where_step::owners_listed(app, &done);
                }
                Some("name_available") => crate::where_step::name_checked(app, Ok(&done)),
                Some("new_project") => crate::where_step::created(app, &done),
                Some("git_push") => {
                    crate::history::pushed(app, &done);
                    crate::where_step::resent(app, &done);
                }
                Some("git_pull") => crate::history::git_pulled(app, &done),
                Some("sync" | "add_remote") if crate::import::link::is_link(&done) => crate::import::link::linked(app, &done),
                Some("sync") => crate::sync::synced(app, &done),
                Some("check_remote") => crate::import::link::checked(app, Ok(done.clone())),
                Some("clone") => crate::import::cloned(app, &done),
                Some("check_address") => crate::import::checked(app, Ok(done.clone())),
                _ => committed(app, &done),
            }
            if forge_ended {
                app.sync.running = false;
            }
            app.forge_check.quiet = false;
        }
        Some(Err(e)) if crate::forge_server::checking(app) => crate::forge_server::checked(app, Err(e)),
        Some(Err(e)) if crate::accounts::waiting(app) => crate::accounts::failed(app, &e),
        Some(Err(e)) => failed(app, was, e),
        None => {}
    }
    // The servers added or removed go to the preferences (no secret in them).
    let (servers, revision) = app.session.forge_servers_saved();
    if revision != app.forge_check.servers_kept {
        app.prefs.forge_servers = servers.to_vec();
        app.forge_check.servers_kept = revision;
        app.prefs_changed = true;
    }
    // The default connection, by its address.
    let (default, revision) = app.session.default_connection_saved();
    if revision != app.forge_check.default_kept {
        app.prefs.default_connection = default.map(str::to_owned);
        app.forge_check.default_kept = revision;
        app.prefs_changed = true;
    }
    // Remember sign-ins in this browser, changed here or in another tab.
    let (remember, revision) = app.session.remember_in_browser_saved();
    if revision != app.forge_check.remember_kept {
        if let Some(on) = remember {
            app.prefs.remember_in_browser = on;
            app.prefs_changed = true;
        }
        app.forge_check.remember_kept = revision;
    }
    // The git servers whose credential the keychain keeps, by host (no secret in them).
    let (hosts, revision) = app.session.git_hosts_saved();
    if revision != app.git_hosts_kept {
        app.prefs.git_hosts = hosts.to_vec();
        app.git_hosts_kept = revision;
        app.prefs_changed = true;
    }
    crate::forge_pick::poll(app);
    if app.session.forge_busy().is_some()
        || app.session.git_busy().is_some()
        || app.session.import_busy().is_some()
        || app.session.forge_auth_busy()
        || app.session.forge_lookup_busy()
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    // GitHub's code came: its page is opened once, where the user enters it.
    if let Some((_, SignInProgress::Code { user_code, verification_uri })) = app.session.forge_signing_in()
        && app.forge_check.opened_code.as_ref() != Some(&user_code)
    {
        app.forge_check.opened_code = Some(user_code);
        ctx.open_url(egui::OpenUrl::new_tab(verification_uri));
    }
    check(app, ctx);
}

/// A forge operation failed: said, and for a commit or a pull that a choice can get through
/// (conflicts, a branch that does not take the commit), its dialog opened again with it.
fn failed(app: &mut App, was: Option<&str>, e: String) {
    if crate::import::failed(app, was, &e) || crate::import::link::failed(app, was, &e) {
        return;
    }
    if crate::forge_save::failed(app, was, &e) || crate::where_step::failed(app, was, &e) || crate::sync::failed(app, was, &e) {
        return;
    }
    if std::mem::take(&mut app.forge_check.quiet) {
        // A periodic check: said once, not every two minutes.
        if app.forge_check.said.as_ref() != Some(&e) {
            app.view.status = Some((e.clone(), true));
            app.forge_check.said = Some(e);
        }
        return;
    }
    app.view.status = Some((e, true));
    match (was, app.session.forge_problem()) {
        (Some("commit"), Some(_)) => ask_commit(app),
        (Some("pull"), Some(ForgeProblem::Conflicts(_))) => ask_resolve(app),
        _ => {}
    }
}

/// Checks the branch for new commits every two minutes and when the window gets the focus back,
/// while a project linked to it is open and a token was given (the forges' limits without one are
/// low); not while a dialog is open or another operation runs.
fn check(app: &mut App, ctx: &egui::Context) {
    let (now, focused) = ctx.input(|i| (i.time, i.focused));
    let came_back = focused && !app.forge_check.focused;
    app.forge_check.focused = focused;
    if app.session.forge().is_none() || !app.session.has_forge_token() {
        app.forge_check.last = None;
        return;
    }
    let Some(last) = app.forge_check.last else {
        // Just opened or signed in: the branch was read then.
        app.forge_check.last = Some(now);
        return;
    };
    let wait = CHECK_EVERY - (now - last);
    if wait > 0.0 && !came_back {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return;
    }
    if app.session.forge_busy().is_some() || app.view.dialog.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
        return;
    }
    app.forge_check.last = Some(now);
    if app.session.execute("forge.pull", &json!({"background": true})).is_ok() {
        app.forge_check.quiet = true;
    }
}

fn pulled(app: &mut App, done: &Value) {
    let branch = done["branch"].as_str().unwrap_or_default();
    if done["moved"] != true {
        if !app.forge_check.quiet {
            app.view.status = Some((tl!("Up to date with the branch {branch}.").replace("{branch}", branch), false));
        }
        return;
    }
    let brought: Vec<String> = names(&done["taken"]).into_iter().chain(names(&done["removed"])).collect();
    let mut said = if brought.is_empty() {
        tl!("Took in the new commits of {branch}: they change none of the project's files.").replace("{branch}", branch)
    } else {
        tl!("Took in the new commits of {branch}: {files}.").replace("{branch}", branch).replace("{files}", &brought.join(", "))
    };
    said = with_merged_and_kept(said, done);
    app.view.status = Some((said, false));
}

/// `said`, with the files merged with the changes made here and those kept as edited.
fn with_merged_and_kept(mut said: String, done: &Value) -> String {
    let merged = names(&done["merged"]);
    if !merged.is_empty() {
        said = format!("{said} {}", tl!("Merged with your changes: {files}.").replace("{files}", &merged.join(", ")));
    }
    let kept = names(&done["kept"]);
    if !kept.is_empty() {
        said = format!("{said} {}", tl!("Changed on the branch but kept as you edited them: {files}.").replace("{files}", &kept.join(", ")));
    }
    said
}

fn names(list: &Value) -> Vec<String> {
    list.as_array().into_iter().flatten().filter_map(|v| v.as_str().or_else(|| v["path"].as_str()).map(str::to_owned)).collect()
}

/// A project opened from a forge (the browser's import): it is open, its import card goes.
pub(crate) fn opened(app: &mut App, done: &Value) {
    app.importing = None;
    app.start = false;
    app.pdf = crate::pdf_view::PdfView::default();
    app.view.last_edit = Some(0.0);
    app.remember_project();
    let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
    let mut said =
        say(service, tl!("Opened {project} from GitLab: {files}, branch {branch}."), tl!("Opened {project} from GitHub: {files}, branch {branch}."))
            .replace("{project}", done["project"].as_str().unwrap_or_default())
            .replace("{files}", &trn(done["files"].as_u64().unwrap_or(0) as usize, "{n} file", "{n} files"))
            .replace("{branch}", done["branch"].as_str().unwrap_or_default());
    let skipped = names(&done["skipped"]);
    if !skipped.is_empty() {
        let left = say(service, tl!("Left on GitLab: {files}."), tl!("Left on GitHub: {files}."));
        said = format!("{said} {}", left.replace("{files}", &skipped.join(", ")));
    }
    app.view.status = Some((said, false));
}

fn committed(app: &mut App, done: &Value) {
    app.forge_check.message.clear();
    if done["commit"].is_null() {
        app.view.status = Some((tl!("Nothing to commit: the files are as on the branch.").to_owned(), false));
        return;
    }
    let n = done["changes"].as_array().map_or(0, Vec::len);
    let branch = done["branch"].as_str().unwrap_or_default();
    let mut said = tl!("Committed {files} to {branch}.").replace("{files}", &trn(n, "{n} file", "{n} files")).replace("{branch}", branch);
    let brought: Vec<String> = names(&done["taken"]).into_iter().chain(names(&done["removed"])).collect();
    if !brought.is_empty() {
        said = format!("{said} {}", tl!("Brought in from the branch: {files}.").replace("{files}", &brought.join(", ")));
    }
    said = with_merged_and_kept(said, done);
    if let Some(e) = done["merge_request_error"].as_str() {
        said = format!("{said} {}", tl!("The merge request could not be opened: {why}").replace("{why}", &crate::i18n::msg(e)));
        app.view.status = Some((said, true));
        return;
    }
    if let (Some(url), Some(iid)) = (done["merge_request"]["web_url"].as_str(), done["merge_request"]["iid"].as_u64()) {
        let target = std::mem::take(&mut app.forge_check.target);
        let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
        let note = MergeRequestNote { service, branch: branch.to_owned(), target, iid, url: url.to_owned(), focus: true };
        app.view.dialog = Some(Dialog::ForgeMergeRequest(note));
    }
    app.view.status = Some((said, false));
}

/// What the status line says while a forge operation runs.
pub(crate) fn busy_text(app: &App) -> Option<String> {
    if app.session.forge_busy().is_none()
        && let Some(op) = app.session.git_busy()
    {
        return crate::forge_save::busy_text(app, op).map(str::to_owned);
    }
    Some(
        match app.session.forge_busy()? {
            _ if app.sync.running => tl!("Syncing…"),
            "open" => opening(app.forge_check.opening),
            // A periodic check says nothing while it runs.
            "pull" if app.forge_check.quiet => return None,
            "pull" => updating(linked(app)),
            _ => committing(linked(app)),
        }
        .to_owned(),
    )
}

/// The files changed since the base, counted again at most once a second while typing.
pub(crate) fn changed_count(app: &mut App, now: f64) -> usize {
    let base = app.session.forge().map(|l| l.base.clone()).unwrap_or_default();
    let revision = (app.session.revision(), app.session.review_revision(), base);
    match &app.forge_count.0 {
        Some((r, _, n)) if *r == revision => *n,
        // Typing: counted again at most once a second; the base moved: now.
        Some((r, at, n)) if r.2 == revision.2 && now - at < 1.0 => {
            let (at, n) = (*at, *n);
            app.ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 - (now - at)));
            n
        }
        _ => {
            let n = app.session.forge_changes().map_or(0, |c| c.len());
            app.forge_count.0 = Some((revision, now, n));
            n
        }
    }
}

/// The forge's project and branch in the top bar, with how many files changed: a button to the
/// Commit dialog.
pub(crate) fn badge(app: &mut App, ui: &mut egui::Ui) {
    let Some(link) = app.session.forge().cloned() else { return };
    let t = app.tokens;
    let now = ui.input(|i| i.time);
    let n = changed_count(app, now);
    let conflicts = app.session.forge_conflicts().len();
    let changed = match (conflicts, n) {
        (1.., _) => trn(conflicts, "{n} file in conflict", "{n} files in conflict"),
        (0, 0) => tl!("up to date").to_owned(),
        (0, n) => trn(n, "{n} changed file", "{n} changed files"),
    };
    let text = format!("{} · {} · {changed}", link.address.path, link.branch);
    let button = egui::Button::image_and_text(icons::image("git-commit-horizontal", t.icon_size, t.text_muted), RichText::new(&text).font(t.small()))
        .frame_when_inactive(false)
        .min_size(egui::vec2(0.0, t.toolbar_height - 8.0));
    let response = ui.add(button);
    let commit = say(link.address.service, tl!("Commit to GitLab…"), tl!("Commit to GitHub…"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{text}: {commit}")));
    if response.on_hover_text(commit).clicked() {
        crate::ui_cmd::run(app, "forge.commit");
    }
}
