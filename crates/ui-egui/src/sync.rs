//! Sync (docs/forges.md, step 6): beside the project's name in the top bar, for a project with a
//! remote (its folder's git remote, or the forge branch it is linked to), one button that makes a
//! version of the changes, takes in the remote's new commits and sends the project's. It says how
//! many versions wait to be sent and received, as text (↑2 ↓1) and in its name, not by colour; a
//! remote on a hosting service TeXCraft recognises (Overleaf) is named beside it. Files in conflict
//! open the dialog that chooses which version of each to keep; a server that wants a token asks
//! for one here (an Overleaf git token, a forge's personal access token).
//!
//! Everything goes through `project.sync`, `git.remotes` and `forge.status`.

use egui::RichText;
use serde_json::{Value, json};
use texcraft_engine::ForgeProblem;

use crate::App;
use crate::dialogs::{Dialog, Outcome, buttons, title};
use crate::i18n::trn;
use crate::theme::Tokens;

/// The folder's remote, as `git.remotes` last said it, and whether a Sync runs.
#[derive(Default)]
pub(crate) struct SyncView {
    remote: Value,
    /// What the remote was read at: the project, its latest version, the transfer running.
    pub key: Option<String>,
    /// A Sync was started and has not ended: its failure opens what gets it through.
    pub running: bool,
}

/// The Sync button's counts, and the service recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Versions to send (a Sync makes one of the changes since the latest first); none when the
    /// remote has nothing of the project yet. For a forge branch: files changed.
    pub up: Option<usize>,
    /// Versions to receive, as last seen (a Sync or a pull).
    pub down: usize,
    /// The project is linked to a forge branch: `up` counts files.
    pub files: bool,
    /// The hosting service the remote is on, by name.
    pub hosting: Option<&'static str>,
    /// The remote's name and branch (`origin/main`), or the forge branch.
    pub place: String,
    pub url: String,
}

/// Reads the folder's remote again when the project, its latest version or a transfer changed.
fn refresh(app: &mut App) {
    let key = format!("{:?}\n{:?}\n{}\n{}", app.session.dir(), app.session.git_busy(), app.session.unversioned(), app.session.is_dirty());
    if app.sync.key.as_ref() == Some(&key) {
        return;
    }
    app.sync.remote = app.session.execute("git.remotes", &json!({})).unwrap_or_default();
    app.sync.key = Some(key);
}

/// The counts, when the project has a remote to sync with.
pub(crate) fn counts(app: &mut App) -> Option<Counts> {
    if let Some(link) = app.session.forge().cloned() {
        let now = app.ctx.input(|i| i.time);
        let changed = crate::forge::changed_count(app, now);
        return Some(Counts {
            up: Some(changed),
            down: 0,
            files: true,
            hosting: None,
            place: format!("{} · {}", link.address.path, link.branch),
            url: link.web_url.clone(),
        });
    }
    refresh(app);
    let remote = &app.sync.remote;
    let name = remote["remote"].as_str()?;
    let pending = usize::from(app.session.unversioned());
    let up = remote["tracking"].as_str().map(|_| remote["ahead"].as_u64().unwrap_or(0) as usize + pending);
    let hosting = remote["url"].as_str().and_then(texcraft_engine::git_hosting).map(texcraft_engine::GitHosting::name);
    Some(Counts {
        up,
        down: remote["behind"].as_u64().unwrap_or(0) as usize,
        files: false,
        hosting,
        place: format!("{name}/{}", remote["remote_branch"].as_str().unwrap_or_default()),
        url: remote["url"].as_str().unwrap_or_default().to_owned(),
    })
}

/// Whether the project has a remote to sync with, as last read (the top bar reads it each frame).
pub(crate) fn has_remote(app: &App) -> bool {
    app.session.forge().is_some() || app.sync.remote["remote"].is_string()
}

/// The button's text after "Sync": the counts, as arrows and numbers.
pub(crate) fn arrows(c: &Counts) -> String {
    let mut out = String::new();
    match c.up {
        Some(0) => {}
        Some(n) => out.push_str(&format!(" ↑{n}")),
        None => out.push_str(" ↑"),
    }
    if c.down > 0 {
        out.push_str(&format!(" ↓{}", c.down));
    }
    out
}

/// The counts in words, for the button's name and tooltip.
pub(crate) fn said(c: &Counts) -> String {
    let mut parts = Vec::new();
    match (c.up, c.files) {
        (None, _) => parts.push(tl!("nothing sent to the remote yet").to_owned()),
        (Some(0), _) => {}
        (Some(n), false) => parts.push(trn(n, "{n} version to send", "{n} versions to send")),
        (Some(n), true) => parts.push(trn(n, "{n} changed file to send", "{n} changed files to send")),
    }
    if c.down > 0 {
        parts.push(trn(c.down, "{n} version to receive", "{n} versions to receive"));
    }
    if parts.is_empty() {
        parts.push(tl!("up to date, as last synced").to_owned());
    }
    let mut out = parts.join(", ");
    if let Some(service) = c.hosting {
        out.push_str(", ");
        out.push_str(&tl!("on {service}").replace("{service}", service));
    }
    out
}

/// The Sync button, and the hosting service's name beside it. `compact`: a narrow window's, the
/// arrows and counts without the word.
pub(crate) fn button(app: &mut App, ui: &mut egui::Ui, compact: bool) {
    let Some(c) = counts(app) else { return };
    let t = app.tokens;
    let busy = app.session.git_busy() == Some("sync") || (app.sync.running && app.session.forge_busy().is_some());
    let can = texcraft_engine::cmd::find("project.sync").map_or(Ok(()), |s| (s.enabled)(&app.session));
    let word = if busy { tl!("Syncing…") } else { tl!("Sync") };
    let text = if compact && !busy { arrows(&c).trim().to_owned() } else { format!("{word}{}", arrows(&c)) };
    let text = if text.is_empty() { word.to_owned() } else { text };
    let name = format!("{word}: {} ({})", said(&c), c.place);
    let r = crate::widgets::framed_button(ui, &t, Some("arrow-up-down"), &text, &name, can.is_ok() && !busy);
    let r = match &can {
        Err(why) if !busy => r.on_disabled_hover_text(crate::i18n::msg(why)),
        _ => r,
    };
    if r.clicked() {
        start(app, json!({}));
    }
    if let Some(service) = c.hosting {
        hosting_label(ui, &t, service, &c.url);
    }
}

/// A remote's hosting service: our own small icon and its name, with the address on hover.
pub(crate) fn hosting_label(ui: &mut egui::Ui, t: &Tokens, service: &str, url: &str) {
    ui.add(crate::icons::image("online-project", t.icon_size - 2.0, t.text_muted));
    let hint = tl!("The remote is on {service}: {url}").replace("{service}", service).replace("{url}", url);
    let label = ui.add(egui::Label::new(RichText::new(service).font(t.small()).color(t.text_muted)).sense(egui::Sense::hover()));
    label.on_hover_text(hint);
}

/// Starts a Sync, in the background, with `extra` parameters (a token, choices for conflicts).
pub(crate) fn start(app: &mut App, extra: Value) {
    let mut params = json!({"background": true});
    if let (Some(to), Some(from)) = (params.as_object_mut(), extra.as_object()) {
        to.extend(from.clone());
    }
    match app.session.execute("project.sync", &params) {
        Ok(done) if done["started"] == true => {
            app.sync.running = true;
            app.view.status = Some((tl!("Syncing…").to_owned(), false));
        }
        Ok(done) => synced(app, &done),
        Err(e) => {
            app.view.status = Some((crate::i18n::msg(&e), true));
            after_failure(app, "project.sync");
        }
    }
}

/// A Sync ended: what it did, on the status line.
pub(crate) fn synced(app: &mut App, done: &Value) {
    app.sync.running = false;
    app.sync.key = None;
    let at = format!("{}/{}", done["pulled"]["remote"].as_str().unwrap_or("origin"), done["pulled"]["branch"].as_str().unwrap_or_default());
    let files: Vec<&str> = done["pulled"]["changed"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str()).collect();
    let sent = done["pushed"]["up_to_date"] != true;
    let said = match (files.is_empty(), sent) {
        (true, false) => tl!("Up to date with {remote}.").replace("{remote}", &at),
        (true, true) => tl!("Synced with {remote}: your versions are sent.").replace("{remote}", &at),
        (false, _) => tl!("Synced with {remote}: took in {files}.").replace("{remote}", &at).replace("{files}", &files.join(", ")),
    };
    app.view.status = Some((said, false));
}

/// A push, a pull or a Sync failed (`command` is what ran): files in conflict open the dialog
/// that chooses which version of each to keep; a missing or refused token asks for one.
pub(crate) fn after_failure(app: &mut App, command: &'static str) {
    app.sync.running = false;
    match app.session.forge_problem().cloned() {
        Some(ForgeProblem::NeedsToken { instance, hosting }) => {
            app.view.dialog =
                Some(Dialog::GitToken(Box::new(TokenForm { command, instance, hosting, token: String::new(), focus: true, remember: true })));
        }
        Some(ForgeProblem::Conflicts(_)) if command == "project.sync" => crate::forge::ask_resolve_sync(app),
        // A forge branch that does not take this user's commits: the commit dialog offers a new
        // branch and a merge request.
        Some(ForgeProblem::CannotPush) if command == "project.sync" => crate::forge::ask_commit(app),
        _ => {}
    }
}

/// An operation the window ran in the background failed: whether it was a Sync's (or a push's
/// or a pull's this module answers).
pub(crate) fn failed(app: &mut App, was: Option<&str>, e: &str) -> bool {
    let command = match was {
        Some("sync") => "project.sync",
        Some("commit" | "pull") if app.sync.running => "project.sync",
        Some("git_push") => "git.push",
        Some("git_pull") => "git.pull",
        _ => return false,
    };
    let problem = app.session.forge_problem().cloned();
    let answered = matches!(problem, Some(ForgeProblem::NeedsToken { .. }))
        || (command == "project.sync" && matches!(problem, Some(ForgeProblem::Conflicts(_))));
    if !answered && command != "project.sync" {
        return false;
    }
    app.view.status = Some((crate::i18n::msg(e), true));
    after_failure(app, command);
    true
}

/// The dialog that asks for a token for a remote's server.
pub(crate) struct TokenForm {
    /// What to run again with it: `project.sync`, `git.push` or `git.pull`.
    pub command: &'static str,
    pub instance: String,
    pub hosting: Option<texcraft_engine::GitHosting>,
    pub token: String,
    pub focus: bool,
    /// Remember on this computer: the token is kept for the server (`git.keep_credential`).
    pub remember: bool,
}

/// The token dialog, while it is open.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let browser = app.session.library().is_some();
    let mut outcome = Outcome::Open;
    let modal = crate::sheet::show(ctx, &t, "texcraft-git-token", 440.0, crate::sheet::Leave::Close, |ui| {
        if let Some(Dialog::GitToken(form)) = app.view.dialog.as_mut() {
            outcome = token_form(ui, &t, form, browser);
        }
    });
    if modal.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Open | Outcome::Discard => {}
        Outcome::Cancel => app.view.dialog = None,
        Outcome::Run => {
            let Some(Dialog::GitToken(form)) = app.view.dialog.take() else { return };
            if form.remember {
                let host = texcraft_engine::forge::servers::host_of(&form.instance).to_owned();
                // Said if it could not be kept; the operation goes on with the token all the same.
                let _ = app.run("git.keep_credential", json!({"host": host, "token": form.token.trim()}));
            }
            let token = json!({"token": form.token.trim()});
            match form.command {
                "git.push" => crate::history::push_with(app, token),
                "git.pull" => crate::history::pull_with(app, token),
                _ => start(app, token),
            }
        }
    }
}

fn token_form(ui: &mut egui::Ui, t: &Tokens, form: &mut TokenForm, browser: bool) -> Outcome {
    let host = texcraft_engine::forge::servers::host_of(&form.instance).to_owned();
    let service = form.hosting.map_or(host.clone(), |h| h.name().to_owned());
    title(ui, t, &tl!("A Token for {service}").replace("{service}", &service));
    let how = match form.hosting {
        Some(_) => tl!(
            "{service} lets git reach a project with a git authentication token: make one in your {service} account's settings, then paste it here."
        )
        .replace("{service}", &service),
        None => tl!(
            "{host} asks for a personal access token that can read and write the repository. Signing in to GitLab or GitHub in Settings › Accounts works too."
        )
        .replace("{host}", &host),
    };
    ui.label(how);
    let field = crate::forge::field(ui, t, tl!("Token"), &mut form.token, "", true);
    if std::mem::take(&mut form.focus) {
        field.request_focus();
    }
    ui.add_space(t.unit * 2.0);
    ui.checkbox(&mut form.remember, if browser { tl!("Remember in this tab") } else { tl!("Remember on this computer") });
    let kept = match (form.remember, browser) {
        (true, false) => {
            tl!(
                "Kept in the system's keychain, so that Sync, push and pull ask no more. Settings › Accounts lists it, with Replace… and Disconnect…."
            )
        }
        (true, true) => {
            tl!("Kept in this tab's session storage until the tab is closed. Settings › Accounts lists it, with Replace… and Disconnect….")
        }
        (false, false) => tl!("Kept while TeXCraft runs, then forgotten."),
        (false, true) => tl!("Kept while this page is open, then forgotten."),
    };
    ui.label(RichText::new(kept).font(t.small()).color(t.text_muted));
    let ready = !form.token.trim().is_empty();
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let action = match form.command {
        "git.push" => tl!("Push"),
        "git.pull" => crate::i18n::tc("button", "Pull"),
        _ => tl!("Sync"),
    };
    match buttons(ui, t, action, false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}
