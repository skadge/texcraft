//! Projects from GitLab (docs/forges.md): the Open from GitLab and Commit to GitLab dialogs, the
//! project and branch in the top bar, and the outcomes of the engine's forge operations on the
//! status line. Everything goes through `forge.open`, `forge.commit` and `forge.status`.

use egui::RichText;
use serde_json::{Value, json};

use crate::dialogs::{Dialog, Outcome, button_row, buttons, title};
use crate::i18n::trn;
use crate::theme::Tokens;
use crate::{App, icons, widgets};

/// The Open from GitLab dialog's fields.
pub(crate) struct OpenForm {
    pub address: String,
    pub branch: String,
    pub token: String,
    pub focus: bool,
}

/// The Commit to GitLab dialog: the changes it lists, the message and, when the session has no
/// token for the instance, the token.
pub(crate) struct CommitForm {
    pub project: String,
    pub branch: String,
    pub changes: Vec<(String, String)>,
    pub message: String,
    pub token: Option<String>,
    pub focus: bool,
}

/// How many files changed since the branch's base, as last counted: (revision, time, count).
#[derive(Default)]
pub(crate) struct Count(Option<(u64, f64, usize)>);

/// Opens the Open from GitLab dialog.
pub(crate) fn ask_open(app: &mut App) {
    app.view.dialog = Some(Dialog::ForgeOpen(OpenForm { address: String::new(), branch: String::new(), token: String::new(), focus: true }));
}

/// Opens the Commit to GitLab dialog, with the files changed since the base.
pub(crate) fn ask_commit(app: &mut App) {
    let Some(link) = app.session.forge().cloned() else {
        let why = tl!("This project was not opened from GitLab: use Open from GitLab… first.");
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
                texcraft_engine::forge::ChangeKind::Added => tl!("added"),
                texcraft_engine::forge::ChangeKind::Modified => tl!("changed"),
                texcraft_engine::forge::ChangeKind::Deleted => tl!("deleted"),
                texcraft_engine::forge::ChangeKind::Moved => tl!("moved"),
            };
            let path = match &c.from {
                Some(from) => format!("{from} → {}", c.path),
                None => c.path.clone(),
            };
            (what.to_owned(), path)
        })
        .collect();
    let token = (!app.session.has_forge_token()).then(String::new);
    app.view.dialog = Some(Dialog::ForgeCommit(CommitForm {
        project: link.address.path.clone(),
        branch: link.branch.clone(),
        changes,
        message: String::new(),
        token,
        focus: true,
    }));
}

/// The open forge dialog, if any.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let browser = app.session.library().is_some();
    let mut outcome = Outcome::Open;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-forge-dialog")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(460.0);
        match app.view.dialog.as_mut() {
            Some(Dialog::ForgeOpen(form)) => outcome = open_form(ui, &t, form, browser),
            Some(Dialog::ForgeCommit(form)) => outcome = commit_form(ui, &t, form, browser),
            _ => {}
        }
    });
    if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Open | Outcome::Discard => {}
        Outcome::Cancel => app.view.dialog = None,
        Outcome::Run => match app.view.dialog.take() {
            Some(Dialog::ForgeOpen(form)) => {
                let params = json!({"url": form.address.trim(), "token": form.token.trim(), "ref": form.branch.trim(), "background": true});
                if app.run("forge.open", params).is_some() {
                    app.view.status = Some((tl!("Opening the project from GitLab…").to_owned(), false));
                }
            }
            Some(Dialog::ForgeCommit(form)) => {
                let mut params = json!({"message": form.message.trim(), "background": true});
                if let Some(token) = &form.token {
                    params["token"] = json!(token.trim());
                }
                if app.run("forge.commit", params).is_some() {
                    app.view.status = Some((tl!("Committing to GitLab…").to_owned(), false));
                }
            }
            _ => {}
        },
    }
}

/// A one-line field named by the label above it (AGENTS.md rule 21).
fn field(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, hint: &str, password: bool) -> egui::Response {
    ui.add_space(t.unit * 2.0);
    let label = ui.label(RichText::new(label).color(t.text_muted));
    let edit = egui::TextEdit::singleline(value).password(password).hint_text(hint).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
    ui.add(edit).labelled_by(label.id)
}

/// What the token is for and that it is not kept.
fn token_note(ui: &mut egui::Ui, t: &Tokens, browser: bool) {
    let kept = if browser {
        tl!("TeXCraft keeps the token only while this page is open: it is never stored.")
    } else {
        tl!("TeXCraft keeps the token only while it runs: it is never written to disk.")
    };
    let text = format!(
        "{} {kept}",
        tl!("Make one in GitLab under Preferences › Access tokens. The read_api scope opens projects; committing needs the api scope.")
    );
    ui.label(RichText::new(text).font(t.small()).color(t.text_muted));
}

fn open_form(ui: &mut egui::Ui, t: &Tokens, form: &mut OpenForm, browser: bool) -> Outcome {
    title(ui, t, tl!("Open from GitLab"));
    let address = field(ui, t, tl!("Project address"), &mut form.address, "https://gitlab.com/group/project", false);
    if std::mem::take(&mut form.focus) {
        address.request_focus();
    }
    let branch = field(ui, t, tl!("Branch (the default branch if left empty)"), &mut form.branch, "main", false);
    let token = field(ui, t, tl!("Personal access token"), &mut form.token, "glpat-…", true);
    ui.add_space(t.unit);
    token_note(ui, t, browser);
    let enter = [address, branch, token].iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let ready = !form.address.trim().is_empty() && !form.token.trim().is_empty();
    match buttons(ui, t, tl!("Open"), false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}

fn commit_form(ui: &mut egui::Ui, t: &Tokens, form: &mut CommitForm, browser: bool) -> Outcome {
    title(ui, t, tl!("Commit to GitLab"));
    let to = tl!("To the branch {branch} of {project}.").replace("{branch}", &form.branch).replace("{project}", &form.project);
    ui.label(to);
    ui.add_space(t.unit * 2.0);
    if form.changes.is_empty() {
        ui.label(RichText::new(tl!("Nothing to commit: the files are as on the branch.")).color(t.text_muted));
        let mut outcome = Outcome::Open;
        ui.add_space(t.unit * 4.0);
        button_row(ui, |ui| {
            if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                outcome = Outcome::Cancel;
            }
        });
        return outcome;
    }
    ui.label(RichText::new(trn(form.changes.len(), "{n} changed file", "{n} changed files")).font(t.strong()));
    egui::ScrollArea::vertical().max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
        for (what, path) in &form.changes {
            ui.horizontal(|ui| {
                ui.label(RichText::new(what).font(t.small()).color(t.text_muted));
                ui.label(RichText::new(path).font(t.small()));
            });
        }
    });
    let message = field(ui, t, tl!("What this commit changes"), &mut form.message, "", false);
    if std::mem::take(&mut form.focus) {
        message.request_focus();
    }
    let mut responses = vec![message];
    if let Some(token) = &mut form.token {
        responses.push(field(ui, t, tl!("Personal access token"), token, "glpat-…", true));
        ui.add_space(t.unit);
        token_note(ui, t, browser);
    }
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let ready = !form.message.trim().is_empty() && form.token.as_ref().is_none_or(|t| !t.trim().is_empty());
    match buttons(ui, t, tl!("Commit"), false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}

/// Takes the outcome of a forge operation that ended, and keeps the window looking while one runs.
pub(crate) fn poll(app: &mut App, ctx: &egui::Context) {
    match app.session.poll_forge() {
        Some(Ok(done)) => match done["operation"].as_str() {
            Some("open") => opened(app, &done),
            _ => committed(app, &done),
        },
        Some(Err(e)) => app.view.status = Some((e, true)),
        None => {}
    }
    if app.session.forge_busy().is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array().into_iter().flatten().filter_map(|v| v.as_str().or_else(|| v["path"].as_str()).map(str::to_owned)).collect()
}

fn opened(app: &mut App, done: &Value) {
    app.start = false;
    app.pdf = crate::pdf_view::PdfView::default();
    app.view.last_edit = Some(0.0);
    app.remember_project();
    let mut said = tl!("Opened {project} from GitLab: {files}, branch {branch}.")
        .replace("{project}", done["project"].as_str().unwrap_or_default())
        .replace("{files}", &trn(done["files"].as_u64().unwrap_or(0) as usize, "{n} file", "{n} files"))
        .replace("{branch}", done["branch"].as_str().unwrap_or_default());
    let skipped = names(&done["skipped"]);
    if !skipped.is_empty() {
        said = format!("{said} {}", tl!("Left on GitLab: {files}.").replace("{files}", &skipped.join(", ")));
    }
    app.view.status = Some((said, false));
}

fn committed(app: &mut App, done: &Value) {
    if done["commit"].is_null() {
        app.view.status = Some((tl!("Nothing to commit: the files are as on the branch.").to_owned(), false));
        return;
    }
    let n = done["changes"].as_array().map_or(0, Vec::len);
    let mut said = tl!("Committed {files} to {branch}.")
        .replace("{files}", &trn(n, "{n} file", "{n} files"))
        .replace("{branch}", done["branch"].as_str().unwrap_or_default());
    let brought: Vec<String> = names(&done["taken"]).into_iter().chain(names(&done["removed"])).collect();
    if !brought.is_empty() {
        said = format!("{said} {}", tl!("Brought in from the branch: {files}.").replace("{files}", &brought.join(", ")));
    }
    let kept = names(&done["kept"]);
    if !kept.is_empty() {
        said = format!("{said} {}", tl!("Changed on the branch but kept as you edited them: {files}.").replace("{files}", &kept.join(", ")));
    }
    app.view.status = Some((said, false));
}

/// What the status line says while a forge operation runs.
pub(crate) fn busy_text(app: &App) -> Option<String> {
    Some(
        match app.session.forge_busy()? {
            "open" => tl!("Opening the project from GitLab…"),
            _ => tl!("Committing to GitLab…"),
        }
        .to_owned(),
    )
}

/// The files changed since the base, counted again at most once a second while typing.
fn changed_count(app: &mut App, now: f64) -> usize {
    let revision = app.session.revision();
    match app.forge_count.0 {
        Some((r, _, n)) if r == revision => n,
        Some((_, at, n)) if now - at < 1.0 => {
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

/// The GitLab project and branch in the top bar, with how many files changed: a button to the
/// Commit to GitLab dialog.
pub(crate) fn badge(app: &mut App, ui: &mut egui::Ui) {
    let Some(link) = app.session.forge().cloned() else { return };
    let t = app.tokens;
    let now = ui.input(|i| i.time);
    let n = changed_count(app, now);
    let changed = if n == 0 { tl!("up to date").to_owned() } else { trn(n, "{n} changed file", "{n} changed files") };
    let text = format!("{} · {} · {changed}", link.address.path, link.branch);
    let button = egui::Button::image_and_text(icons::image("git-commit-horizontal", t.icon_size, t.text_muted), RichText::new(&text).font(t.small()))
        .frame_when_inactive(false)
        .min_size(egui::vec2(0.0, t.toolbar_height - 8.0));
    let response = ui.add(button);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{text}: {}", tl!("Commit to GitLab…"))));
    if response.on_hover_text(tl!("Commit to GitLab…")).clicked() {
        crate::ui_cmd::run(app, "forge.commit");
    }
}
