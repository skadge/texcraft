//! Projects from GitLab (docs/forges.md): the Open from GitLab and Commit to GitLab dialogs, the
//! files in conflict with the branch (keep mine or take theirs, or a new branch with a merge
//! request), the project and branch in the top bar, the branch checked for new commits every two
//! minutes and when the window comes back, and the outcomes of the engine's forge operations on
//! the status line. Everything goes through `forge.open`, `forge.pull`, `forge.commit` and
//! `forge.status`.

use egui::RichText;
use serde_json::{Value, json};
use texcraft_engine::ForgeProblem;
use texcraft_engine::forge::{Choice, ConflictKind};

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

/// The Commit to GitLab dialog: the changes it lists, the files in conflict with the branch, the
/// message, where the commit goes and, when the session has no token for the instance, the token.
pub(crate) struct CommitForm {
    pub project: String,
    pub branch: String,
    pub changes: Vec<(String, String)>,
    pub conflicts: Vec<ConflictRow>,
    pub message: String,
    pub token: Option<String>,
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

/// The files in conflict after Update from GitLab, to decide before the branch is taken in.
pub(crate) struct ResolveForm {
    pub branch: String,
    pub conflicts: Vec<ConflictRow>,
}

/// A merge request just opened: its link.
pub(crate) struct MergeRequestNote {
    pub branch: String,
    pub target: String,
    pub iid: u64,
    pub url: String,
    pub focus: bool,
}

/// How many files changed since the branch's base, as last counted: (revision, time, count).
#[derive(Default)]
pub(crate) struct Count(Option<((u64, u64), f64, usize)>);

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
}

/// How often the branch is checked for new commits, in seconds.
const CHECK_EVERY: f64 = 120.0;

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
    let cannot_push = app.session.forge_problem() == Some(&ForgeProblem::CannotPush);
    let new_branch = format!("{}-{}", link.branch, chrono::Local::now().format("%Y%m%d-%H%M"));
    app.view.dialog = Some(Dialog::ForgeCommit(CommitForm {
        project: link.address.path.clone(),
        branch: link.branch.clone(),
        changes,
        conflicts: conflict_rows(app),
        message: app.forge_check.message.clone(),
        token,
        to_new_branch: cannot_push,
        new_branch,
        cannot_push,
        focus: true,
    }));
}

/// Update from GitLab: takes in the branch's new commits, in the background.
pub(crate) fn pull(app: &mut App) {
    if app.run("forge.pull", json!({"background": true})).is_some() {
        app.forge_check.quiet = false;
        app.view.status = Some((tl!("Updating from GitLab…").to_owned(), false));
    }
}

/// Opens the dialog of the files a pull found in conflict.
fn ask_resolve(app: &mut App) {
    let Some(link) = app.session.forge() else { return };
    let branch = link.branch.clone();
    app.view.dialog = Some(Dialog::ForgeResolve(ResolveForm { branch, conflicts: conflict_rows(app) }));
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
            Some(Dialog::ForgeResolve(form)) => outcome = resolve_form(ui, &t, form),
            Some(Dialog::ForgeMergeRequest(note)) => outcome = merge_request_note(ui, &t, note),
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
                if form.to_new_branch {
                    params["new_branch"] = json!(form.new_branch.trim());
                } else if !form.conflicts.is_empty() {
                    params["resolve"] = resolve(&form.conflicts);
                }
                app.forge_check.message = form.message.trim().to_owned();
                app.forge_check.target = form.branch.clone();
                if app.run("forge.commit", params).is_some() {
                    app.forge_check.quiet = false;
                    app.view.status = Some((tl!("Committing to GitLab…").to_owned(), false));
                }
            }
            Some(Dialog::ForgeResolve(form)) if app.run("forge.pull", json!({"resolve": resolve(&form.conflicts), "background": true})).is_some() => {
                app.forge_check.quiet = false;
                app.view.status = Some((tl!("Updating from GitLab…").to_owned(), false));
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
    let to = if form.cannot_push {
        tl!(
            "The branch {branch} of {project} does not take your commits: it is protected, or your role cannot push to it. Your changes go to a new branch instead, with a merge request into {branch}."
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
        button_row(ui, |ui| {
            if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
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
        conflicts(ui, t, &mut form.conflicts, !form.to_new_branch);
    }
    ui.add_space(t.unit * 2.0);
    let label = tl!("Commit to a new branch and open a merge request");
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
    if let Some(token) = &mut form.token {
        responses.push(field(ui, t, tl!("Personal access token"), token, "glpat-…", true));
        ui.add_space(t.unit);
        token_note(ui, t, browser);
    }
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let decided = if form.to_new_branch { !form.new_branch.trim().is_empty() } else { form.conflicts.iter().all(|c| c.choice.is_some()) };
    let ready = decided && !form.message.trim().is_empty() && form.token.as_ref().is_none_or(|t| !t.trim().is_empty());
    let action = if form.to_new_branch { tl!("Commit and Open Merge Request") } else { tl!("Commit") };
    match buttons(ui, t, action, false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}

/// The files in conflict, each with Keep mine and Take theirs (AGENTS.md rule 21: each named by
/// its label and its file).
fn conflicts(ui: &mut egui::Ui, t: &Tokens, rows: &mut [ConflictRow], enabled: bool) {
    ui.label(RichText::new(trn(rows.len(), "{n} file in conflict with the branch", "{n} files in conflict with the branch")).font(t.strong()));
    let note = tl!(
        "The branch has new commits that change the same parts of these files. For each, keep your version or take the branch's; or commit to a new branch and open a merge request, where GitLab shows both."
    );
    ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
    egui::ScrollArea::vertical().id_salt("forge-conflicts").max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
        for row in rows.iter_mut() {
            ui.add_space(t.unit);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&row.path).font(t.strong()));
                ui.label(RichText::new(row.why).font(t.small()).color(t.text_muted));
            });
            ui.horizontal(|ui| {
                for (choice, label) in [(Choice::Mine, tl!("Keep mine")), (Choice::Theirs, tl!("Take theirs"))] {
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
    title(ui, t, tl!("Update from GitLab"));
    ui.label(tl!("Nothing was taken in from the branch {branch} yet: choose for each file below first.").replace("{branch}", &form.branch));
    ui.add_space(t.unit * 2.0);
    conflicts(ui, t, &mut form.conflicts, true);
    let ready = form.conflicts.iter().all(|c| c.choice.is_some());
    buttons(ui, t, tl!("Update"), false, ready)
}

fn merge_request_note(ui: &mut egui::Ui, t: &Tokens, note: &mut MergeRequestNote) -> Outcome {
    title(ui, t, tl!("Merge Request Opened"));
    let said = tl!("Your changes are on the new branch {branch}, in merge request !{iid} into {target}. The project now follows {branch}.")
        .replace("{branch}", &note.branch)
        .replace("{iid}", &note.iid.to_string())
        .replace("{target}", &note.target);
    ui.label(said);
    ui.add_space(t.unit * 2.0);
    let link = ui.hyperlink_to(tl!("Open the merge request on GitLab"), &note.url);
    if std::mem::take(&mut note.focus) {
        link.request_focus();
    }
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    button_row(ui, |ui| {
        if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}

/// Takes the outcome of a forge operation that ended, and keeps the window looking while one runs.
pub(crate) fn poll(app: &mut App, ctx: &egui::Context) {
    let was = app.session.forge_busy();
    match app.session.poll_forge() {
        Some(Ok(done)) => {
            app.forge_check.said = None;
            app.forge_check.last = Some(ctx.input(|i| i.time));
            match done["operation"].as_str() {
                Some("open") => opened(app, &done),
                Some("pull") => pulled(app, &done),
                _ => committed(app, &done),
            }
            app.forge_check.quiet = false;
        }
        Some(Err(e)) => failed(app, was, e),
        None => {}
    }
    if app.session.forge_busy().is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    check(app, ctx);
}

/// A forge operation failed: said, and for a commit or a pull that a choice can get through
/// (conflicts, a branch that does not take the commit), its dialog opened again with it.
fn failed(app: &mut App, was: Option<&str>, e: String) {
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
/// while a project linked to it is open and a token was given (GitLab's limits without one are
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
        said = format!("{said} {}", tl!("The merge request could not be opened: {why}").replace("{why}", e));
        app.view.status = Some((said, true));
        return;
    }
    if let (Some(url), Some(iid)) = (done["merge_request"]["web_url"].as_str(), done["merge_request"]["iid"].as_u64()) {
        let target = std::mem::take(&mut app.forge_check.target);
        app.view.dialog =
            Some(Dialog::ForgeMergeRequest(MergeRequestNote { branch: branch.to_owned(), target, iid, url: url.to_owned(), focus: true }));
    }
    app.view.status = Some((said, false));
}

/// What the status line says while a forge operation runs.
pub(crate) fn busy_text(app: &App) -> Option<String> {
    Some(
        match app.session.forge_busy()? {
            "open" => tl!("Opening the project from GitLab…"),
            // A periodic check says nothing while it runs.
            "pull" if app.forge_check.quiet => return None,
            "pull" => tl!("Updating from GitLab…"),
            _ => tl!("Committing to GitLab…"),
        }
        .to_owned(),
    )
}

/// The files changed since the base, counted again at most once a second while typing.
fn changed_count(app: &mut App, now: f64) -> usize {
    let revision = (app.session.revision(), app.session.review_revision());
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
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{text}: {}", tl!("Commit to GitLab…"))));
    if response.on_hover_text(tl!("Commit to GitLab…")).clicked() {
        crate::ui_cmd::run(app, "forge.commit");
    }
}
