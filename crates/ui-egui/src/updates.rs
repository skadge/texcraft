//! Updates (docs/ROADMAP.md, A8): the daily check shortly after the start, a quiet notice in the
//! status bar when a newer version is out, and the What's New dialog with the release's notes and
//! what this copy can do (Help › Check for Updates… opens it too). The engine does the checking
//! and the replacing (`texcraft_engine::update`); this is what the window shows of it.

use egui::RichText;
use serde_json::json;
use texcraft_engine::UpdateEvent;
use texcraft_engine::update::{Action, Install, Release};

use crate::dialogs::{Dialog, Next};
use crate::theme::Tokens;
use crate::{App, widgets};

/// How long after the start the daily check waits, so that it never slows the start.
const STARTUP_DELAY: f64 = 8.0;

/// The window's part of the updates: when the daily check is due.
#[derive(Default)]
pub(crate) struct UpdateView {
    /// The window's time at its first frame.
    started: Option<f64>,
    /// The daily check was asked for (or not due) in this run.
    asked: bool,
    /// The dialog's Check Again or Install has the keyboard next.
    pub focus: bool,
}

/// Each frame: the daily check once its delay has passed, and the end of a check or download.
pub(crate) fn tick(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let started = *app.updates.started.get_or_insert(now);
    if !app.updates.asked && app.session.updates().enabled() {
        let wait = STARTUP_DELAY - (now - started);
        if wait <= 0.0 {
            app.updates.asked = true;
            // Nothing is asked when the setting is off or the last answer is recent.
            if let Err(e) = app.session.execute("app.check_updates", &json!({"automatic": true})) {
                log::warn!("updates: {e}");
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }
    match app.session.poll_update() {
        Some(UpdateEvent::Installed) => {
            let version = app.session.updates().available().map(|r| r.version.clone()).unwrap_or_default();
            app.view.status = Some((tf!("TeXCraft {version} is in place: restart to use it.", version = version), false));
        }
        Some(UpdateEvent::InstallFailed(why)) => app.view.status = Some((crate::i18n::msg(&why), true)),
        // The dialog, when open, shows the answer; the status bar's notice shows a new version.
        Some(UpdateEvent::Checked { .. } | UpdateEvent::CheckFailed { .. }) | None => {}
    }
    let u = app.session.updates();
    if u.checking() || u.progress().is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

/// Help › Check for Updates…: the dialog, and a check at once.
pub(crate) fn check_now(app: &mut App) {
    app.view.dialog = Some(Dialog::Updates);
    app.updates.focus = true;
    if let Err(e) = app.session.execute("app.check_updates", &json!({})) {
        app.view.status = Some((crate::i18n::msg(&e), true));
    }
}

/// Restarts into the new version (the desktop's AppImage) or reloads the page (the browser), once
/// unsaved changes are saved or let go.
pub(crate) fn restart(app: &mut App) {
    let u = app.session.updates();
    if *u.install() == Install::Browser && u.available().is_some() {
        app.guard(Next::Reload);
    } else if u.restart_program().is_some() {
        app.guard(Next::Restart);
    } else {
        app.view.status = Some((tl!("No update is ready to restart into.").to_owned(), true));
    }
}

/// The status bar's notice: a newer version is out, or in place. None otherwise.
pub(crate) fn status_item(app: &mut App, ui: &mut egui::Ui) {
    let u = app.session.updates();
    let Some(release) = u.available() else { return };
    let text = if u.installed() {
        tl!("Restart to update").to_owned()
    } else if *u.install() == Install::Browser {
        tl!("A new version is ready").to_owned()
    } else {
        tf!("TeXCraft {version} is available", version = release.version)
    };
    let t = app.tokens;
    let button = egui::Button::new(RichText::new(&text).font(t.small()).color(t.text)).frame_when_inactive(false);
    let r = ui.add(button);
    widgets::focus_ring(ui, &t, &r, r.rect, t.corner());
    crate::status::announce(ui, &r, &text);
    if r.on_hover_text(tl!("What's new, and how to update")).clicked() {
        app.view.dialog = Some(Dialog::Updates);
        app.updates.focus = true;
    }
}

/// Settings' row: the daily check on or off, what it contacts, and when it last answered.
pub(crate) fn setting(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let browser = *app.session.updates().install() == Install::Browser;
    let what = if browser {
        tl!("Asks this site for its version once a day; nothing about you is sent.")
    } else {
        tl!("Asks gitlab.com once a day; nothing about you is sent.")
    };
    let last = app.session.settings.updates.last_check.and_then(|s| chrono::DateTime::from_timestamp(s, 0));
    let help = match last {
        Some(at) => format!("{what} {}", tf!("Last checked {when}.", when = crate::i18n::ago(at, chrono::Utc::now()))),
        None => what.to_owned(),
    };
    crate::settings::setting(ui, t, tl!("Check for updates"), &help, |ui| {
        let mut on = app.session.settings.updates.check;
        let r = ui.checkbox(&mut on, "");
        if r.changed() {
            app.run("app.update_settings", json!({"check": on}));
        }
        Some(r)
    });
}

/// What the dialog's buttons asked.
enum Chosen {
    Close,
    CheckAgain,
    Install,
    Restart,
    Open(String),
}

/// The What's New dialog: the state of the check, the release's notes, what this copy can do.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let focus = std::mem::take(&mut app.updates.focus);
    let unsaved = app.session.is_dirty() && !crate::autosave::applies(app);
    let mut chosen = None;
    let shown = crate::sheet::show(ctx, &t, "texcraft-updates", 520.0, crate::sheet::Leave::Close, |ui| {
        chosen = body(app, ui, &t, focus, unsaved);
    });
    if shown.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        chosen = Some(Chosen::Close);
    }
    match chosen {
        None => {}
        Some(Chosen::Close) => app.view.dialog = None,
        Some(Chosen::CheckAgain) => check_now(app),
        Some(Chosen::Install) => {
            app.run("app.install_update", json!({}));
        }
        Some(Chosen::Restart) => {
            app.view.dialog = None;
            restart(app);
        }
        Some(Chosen::Open(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
    }
}

/// The primary button, which takes the keyboard when the dialog opens.
fn primary(ui: &mut egui::Ui, t: &Tokens, label: &str, focus: bool) -> egui::Response {
    let r = crate::dialogs::primary(ui, t, label, true, false);
    if focus {
        r.request_focus();
    }
    r
}

fn secondary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    crate::dialogs::secondary(ui, label, true)
}

fn body(app: &App, ui: &mut egui::Ui, t: &Tokens, focus: bool, unsaved: bool) -> Option<Chosen> {
    let u = app.session.updates();
    let mut chosen = None;
    let muted = |s: &str| RichText::new(s.to_owned()).color(t.text_muted);
    let Some(release) = u.available() else {
        // Checking, up to date, or the check failed.
        let (head, line) = if u.checking() {
            (tl!("Checking for updates…").to_owned(), String::new())
        } else if let Some(e) = u.error() {
            (tl!("The check for updates did not work").to_owned(), crate::i18n::msg(e))
        } else if u.answered() {
            (tl!("TeXCraft is up to date").to_owned(), tf!("You have version {version}, the latest.", version = u.running()))
        } else {
            (tl!("Updates").to_owned(), tf!("You have version {version}.", version = u.running()))
        };
        crate::dialogs::title(ui, t, &head);
        let label = ui.label(if line.is_empty() { muted(" ") } else { muted(&line) });
        crate::status::announce(ui, &label, &format!("{head}. {line}"));
        ui.add_space(t.unit * 4.0);
        crate::dialogs::actions(ui, |ui| {
            if crate::dialogs::secondary(ui, tl!("Check Again"), !u.checking()).clicked() {
                chosen = Some(Chosen::CheckAgain);
            }
            let close = secondary(ui, tl!("Close"));
            if focus {
                close.request_focus();
            }
            if close.clicked() {
                chosen = Some(Chosen::Close);
            }
        });
        return chosen;
    };
    let action = u.action().unwrap_or(Action::ReleasePage);
    let page = u.page().unwrap_or_else(|| release.page.clone());
    let head = if action == Action::Reload {
        tl!("A new version is ready").to_owned()
    } else {
        tf!("TeXCraft {version} is available", version = release.version)
    };
    crate::dialogs::title(ui, t, &head);
    ui.label(muted(&tf!("You have version {version}.", version = u.running())));
    notes(ui, t, release);
    ui.add_space(t.unit * 2.0);
    let how = how(app, &action, release, unsaved);
    let said = ui.label(RichText::new(&how).font(t.small()).color(t.text_muted));
    crate::status::announce(ui, &said, &how);
    ui.add_space(t.unit * 4.0);
    crate::dialogs::actions(ui, |ui| {
        match &action {
            Action::ReplaceAppImage { .. } if u.installed() => {
                if primary(ui, t, tl!("Restart Now"), focus).clicked() {
                    chosen = Some(Chosen::Restart);
                }
            }
            Action::ReplaceAppImage { .. } => {
                let busy = u.progress().is_some();
                let label = if busy { tl!("Downloading…") } else { tl!("Download and Install") };
                let r = crate::dialogs::primary(ui, t, label, !busy, false);
                if focus {
                    r.request_focus();
                }
                if r.clicked() {
                    chosen = Some(Chosen::Install);
                }
            }
            Action::Download { asset } => {
                if primary(ui, t, &tf!("Download {file}", file = asset.name), focus).clicked() {
                    chosen = Some(Chosen::Open(asset.url.clone()));
                }
            }
            Action::Reload => {
                if primary(ui, t, tl!("Reload"), focus).clicked() {
                    chosen = Some(Chosen::Restart);
                }
            }
            Action::Flatpak { .. } | Action::ReleasePage => {
                if primary(ui, t, tl!("Open the Release Page"), focus).clicked() {
                    chosen = Some(Chosen::Open(page.clone()));
                }
            }
        }
        if matches!(action, Action::ReplaceAppImage { .. } | Action::Download { .. }) && secondary(ui, tl!("Release Page")).clicked() {
            chosen = Some(Chosen::Open(page.clone()));
        }
        if secondary(ui, tl!("Close")).clicked() {
            chosen = Some(Chosen::Close);
        }
    });
    chosen
}

/// The release's notes, as text, in a box that scrolls.
fn notes(ui: &mut egui::Ui, t: &Tokens, release: &Release) {
    let text = texcraft_engine::update::notes_text(&release.notes);
    if text.is_empty() {
        return;
    }
    ui.add_space(t.unit * 2.0);
    ui.label(RichText::new(tl!("What's New")).font(t.strong()));
    egui::Frame::new().fill(t.canvas).stroke(egui::Stroke::new(1.0, t.separator)).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(
        ui,
        |ui| {
            egui::ScrollArea::vertical().id_salt("update-notes").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                // Selectable, so its words can be copied; the notes are the release's, in its language.
                let r = ui.add(egui::Label::new(RichText::new(&text).font(t.small())).selectable(true).wrap());
                let _ = r.accessible_name(tl!("What's New"));
            });
        },
    );
}

/// What updating does here, in words (AGENTS.md rule 15: what TeXCraft does and does not).
fn how(app: &App, action: &Action, release: &Release, unsaved: bool) -> String {
    let u = app.session.updates();
    match action {
        Action::ReplaceAppImage { .. } if u.installed() => tl!("The new version is in place. Restart TeXCraft to use it: the old one is kept beside it until the new one starts.").to_owned(),
        Action::ReplaceAppImage { .. } => match u.progress() {
            Some((done, total)) if total > 0 => tf!(
                "Downloading: {done} of {total}…",
                done = crate::i18n::file_size(done),
                total = crate::i18n::file_size(total)
            ),
            Some((done, _)) => tf!("Downloading: {done}…", done = crate::i18n::file_size(done)),
            None => tl!("TeXCraft downloads the new AppImage, checks it against the release's checksums and puts it in place of this one.").to_owned(),
        },
        Action::Flatpak { bundle } => {
            let file = bundle.as_ref().map_or_else(|| format!("TeXCraft-{}-x86_64.flatpak", release.version), |b| b.name.clone());
            tf!(
                "Updates come through Flatpak: update TeXCraft in your software centre, or install the release's bundle with: flatpak install --user {file}",
                file = file
            )
        }
        Action::Download { .. } if *u.install() == Install::MacOs => {
            tl!("TeXCraft does not replace itself on macOS: the app is not signed. Download the new one and put it in place of this one.").to_owned()
        }
        Action::Download { .. } => {
            tl!("TeXCraft does not replace itself on Windows: the program is not signed. Download the new zip and unpack it in place of this folder.").to_owned()
        }
        Action::ReleasePage => tl!("This copy cannot replace itself (it was built from source, or the release has no file for it): get the new version from the release page.").to_owned(),
        Action::Reload if unsaved => tl!("Reloading loads the new version. The project has unsaved changes: you are asked about them first.").to_owned(),
        Action::Reload => tl!("Reloading loads the new version. Your projects are kept in this browser.").to_owned(),
    }
}
