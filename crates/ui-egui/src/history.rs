//! The side panel's History (docs/ROADMAP.md, C4): the project's saved versions, newest first,
//! from its git repository (C8), each with what it changed (C9). Choosing a version lists the files it changed; choosing a file
//! compares it with the version before, line by line, read-only. A file, or the whole project, can
//! be brought back as it was, as unsaved changes; in a repository the user made, a Commit button
//! commits the saved project.
//!
//! Everything goes through the engine's `history.*` commands. The list is read again whenever the
//! repository's latest commit changes (a save, a commit).

use egui::{Align, Align2, Layout, RichText, Sense, vec2};
use serde_json::{Value, json};

use crate::dialogs::{Confirm, Dialog, Prompt};
use crate::i18n::trn;
use crate::theme::Tokens;
use crate::widgets::{self, Row};
use crate::{App, Region};

/// How many versions the panel lists.
const LIMIT: usize = 200;

/// The History section's state.
#[derive(Default)]
pub(crate) struct HistoryView {
    commits: Vec<Commit>,
    /// The repository and commit the list was read at.
    key: Option<String>,
    error: Option<String>,
    /// The chosen version, by id, and the chosen file in it.
    selected: Option<String>,
    file: Option<String>,
    diff: Option<Diff>,
    /// The repository's remote as `git.remotes` says it, and what it was read at.
    remote: Value,
    remote_key: Option<String>,
}

struct Commit {
    id: String,
    /// Its first parent; none for the first version.
    parent: Option<String>,
    time: Option<chrono::DateTime<chrono::Utc>>,
    author: String,
    message: String,
    /// The files it changed: path and how ("added", "modified", "deleted").
    changes: Vec<(String, String)>,
}

/// A file of a version compared with the version before.
struct Diff {
    commit: String,
    path: String,
    change: String,
    binary: bool,
    lines: Vec<Line>,
    added: usize,
    removed: usize,
    truncated: bool,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Added,
    Removed,
    Same,
    /// Between two hunks: unchanged lines left out.
    Gap,
}

struct Line {
    kind: Kind,
    /// Its number in the version it is from (the new one, or the old one for a removed line).
    number: Option<u64>,
    text: String,
}

fn parse_commits(v: &Value) -> Vec<Commit> {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    v["commits"]
        .as_array()
        .map(|all| {
            all.iter()
                .map(|c| Commit {
                    id: s(c, "id"),
                    parent: c["parent"].as_str().map(str::to_owned),
                    time: c["time"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.to_utc()),
                    author: s(c, "author"),
                    message: s(c, "message"),
                    changes: c["changes"].as_array().map(|ch| ch.iter().map(|x| (s(x, "path"), s(x, "change"))).collect()).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_diff(commit: &str, path: &str, v: &Value) -> Diff {
    let mut lines = Vec::new();
    for (i, hunk) in v["hunks"].as_array().into_iter().flatten().enumerate() {
        if i > 0 {
            lines.push(Line { kind: Kind::Gap, number: None, text: String::new() });
        }
        for l in hunk["lines"].as_array().into_iter().flatten() {
            let kind = match l["kind"].as_str() {
                Some("added") => Kind::Added,
                Some("removed") => Kind::Removed,
                _ => Kind::Same,
            };
            let number = if kind == Kind::Removed { l["old"].as_u64() } else { l["new"].as_u64() };
            lines.push(Line { kind, number, text: l["text"].as_str().unwrap_or_default().to_owned() });
        }
    }
    let n = |k: &str| v[k].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    Diff {
        commit: commit.to_owned(),
        path: path.to_owned(),
        change: v["change"].as_str().unwrap_or_default().to_owned(),
        binary: v["binary"].as_bool().unwrap_or(false),
        lines,
        added: n("added"),
        removed: n("removed"),
        truncated: v["truncated"].as_bool().unwrap_or(false),
        error: None,
    }
}

impl HistoryView {
    /// Reads the list again when the repository's latest commit changed. Returns the repository's
    /// owner ("ours", "theirs"), none when the project has none.
    fn refresh(&mut self, session: &mut texcraft_engine::Session) -> Option<String> {
        let status = session.execute("history.status", &json!({})).unwrap_or_default();
        let owner = status["repository"].as_str().map(str::to_owned);
        let key = owner.as_ref().map(|_| format!("{}\n{}\n{}", status["folder"], status["prefix"], status["head"]));
        if key == self.key {
            return owner;
        }
        self.key = key;
        self.commits.clear();
        self.error = None;
        if owner.is_some() {
            match session.execute("history.list", &json!({"limit": LIMIT})) {
                Ok(v) => self.commits = parse_commits(&v),
                Err(e) => self.error = Some(e),
            }
        }
        if !self.commits.iter().any(|c| Some(&c.id) == self.selected.as_ref()) {
            self.selected = None;
            self.file = None;
            self.diff = None;
        }
        owner
    }

    /// Compares the chosen file when it changed.
    fn load_diff(&mut self, session: &mut texcraft_engine::Session) {
        let (Some(commit), Some(path)) = (&self.selected, &self.file) else {
            self.diff = None;
            return;
        };
        if self.diff.as_ref().is_some_and(|d| &d.commit == commit && &d.path == path) {
            return;
        }
        self.diff = Some(match session.execute("history.diff", &json!({"commit": commit, "path": path})) {
            Ok(v) => parse_diff(commit, path, &v),
            Err(e) => Diff {
                commit: commit.clone(),
                path: path.clone(),
                change: String::new(),
                binary: false,
                lines: Vec::new(),
                added: 0,
                removed: 0,
                truncated: false,
                error: Some(e),
            },
        });
    }
}

/// The word for how a file changed, so the change is not told by colour or icon alone.
fn change_word(change: &str) -> &'static str {
    match change {
        "added" => crate::i18n::tc("file", "added"),
        "deleted" => crate::i18n::tc("file", "deleted"),
        _ => crate::i18n::tc("file", "modified"),
    }
}

fn change_icon(change: &str) -> &'static str {
    match change {
        "added" => "file-plus",
        "deleted" => "trash",
        _ => "file-text",
    }
}

/// Why the project has no history to show (AGENTS.md rule 15).
fn no_history(session: &texcraft_engine::Session) -> &'static str {
    if session.library().is_some() {
        // A project of the browser's library: its history starts at its first save.
        tl!("The project has no history yet: save it to start one.")
    } else if session.dir().is_none() {
        tl!("The project has no history yet: save it in a folder to start one.")
    } else {
        tl!("The project's history starts at its next save.")
    }
}

enum Action {
    Select(String),
    File(String),
    CloseDiff,
    RestoreFile,
    RestoreProject(String),
    Commit,
}

/// The list of versions; the chosen one with its files and Restore. `first`: the first row.
fn versions(ui: &mut egui::Ui, t: &Tokens, view: &HistoryView, action: &mut Option<Action>, first: &mut Option<egui::Id>) {
    ui.scope_builder(egui::UiBuilder::new().scope_id(egui::Id::unique("texcraft-history-versions")), |ui| {
        let now = chrono::Utc::now();
        egui::ScrollArea::vertical().id_salt("history-list").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for c in &view.commits {
                let selected = view.selected.as_ref() == Some(&c.id);
                let r = commit_row(ui, t, c, selected, now);
                first.get_or_insert(r.id);
                if r.clicked() {
                    *action = Some(Action::Select(c.id.clone()));
                }
                if !selected {
                    continue;
                }
                for (path, change) in &c.changes {
                    let row = Row {
                        depth: 1,
                        icon: change_icon(change),
                        label: path,
                        selected: view.file.as_ref() == Some(path),
                        disclosure: None,
                        tag: Some(change_word(change)),
                        icon_color: None,
                        muted: false,
                    };
                    if widgets::row(ui, t, &row).clicked() {
                        *action = Some(Action::File(path.clone()));
                    }
                }
                ui.horizontal(|ui| {
                    ui.add_space(t.unit * 6.0);
                    if widgets::text_button(ui, t, tl!("Restore the Project at This Version…"), true).clicked() {
                        *action = Some(Action::RestoreProject(c.id.clone()));
                    }
                });
            }
        });
    });
}

pub(crate) fn panel(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut view = std::mem::take(&mut app.history);
    let owner = view.refresh(&mut app.session);
    view.load_diff(&mut app.session);
    let mut action = None;
    ui.spacing_mut().item_spacing = vec2(1.5 * t.unit, t.unit);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, &t, tl!("History"));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if owner.as_deref() == Some("theirs") {
                let can = texcraft_engine::cmd::find("history.commit").map_or(Ok(()), |c| (c.enabled)(&app.session));
                let r = widgets::text_button(ui, &t, tl!("Commit…"), can.is_ok());
                app.region(Region::Side, r.id);
                match &can {
                    Err(reason) => {
                        let _ = r.on_disabled_hover_text(reason.as_str());
                    }
                    Ok(()) if r.clicked() => action = Some(Action::Commit),
                    Ok(()) => {}
                }
            }
        });
    });
    // What the list is, or why there is none.
    let note = match (&owner, &view.error) {
        (_, Some(e)) => Some((crate::i18n::msg(e), t.error)),
        (None, None) => Some((no_history(&app.session).to_owned(), t.text_muted)),
        (Some(_), None) if view.commits.is_empty() => Some((tl!("No saved versions yet.").to_owned(), t.text_muted)),
        (Some(o), None) if o == "theirs" => {
            Some((tl!("This folder is in a git repository of its own: saving does not commit, Commit does.").to_owned(), t.text_muted))
        }
        (Some(_), None) => None,
    };
    if let Some((text, color)) = note {
        ui.horizontal(|ui| {
            ui.add_space(t.unit * 2.0);
            ui.add(egui::Label::new(RichText::new(text).font(t.small()).color(color)).wrap());
        });
    }
    if owner.is_some() && app.session.library().is_none() {
        remote_section(app, ui, &t, &mut view);
    }
    widgets::hairline(ui, &t);
    // The versions, then a version's file's changes below them, in the order the keyboard visits
    // them too. The list keeps its own id, so its controls keep theirs (and the keyboard) when the
    // changes open.
    let mut first = None;
    let fill = egui::Frame::new().fill(t.frame);
    if let Some(diff) = &view.diff {
        egui::Panel::top("history-versions")
            .resizable(true)
            .default_size(ui.available_height() * 0.4)
            .min_size(120.0)
            .frame(fill)
            .show(ui, |ui| versions(ui, &t, &view, &mut action, &mut first));
        widgets::name_splitter(ui, "history-versions", tl!("Resize the versions and the changes"));
        egui::CentralPanel::default().frame(fill).show(ui, |ui| diff_view(ui, &t, diff, &mut action));
    } else {
        egui::CentralPanel::default().frame(fill).show(ui, |ui| versions(ui, &t, &view, &mut action, &mut first));
    }
    if let Some(id) = first {
        app.region(Region::Side, id);
    }
    match action {
        Some(Action::Select(id)) => {
            if view.selected.as_ref() == Some(&id) {
                view.selected = None;
            } else {
                view.selected = Some(id);
            }
            view.file = None;
        }
        Some(Action::File(path)) => view.file = Some(path),
        Some(Action::CloseDiff) => view.file = None,
        Some(Action::RestoreFile) => {
            if let Some(dialog) = restore_file(&view) {
                app.view.dialog = Some(dialog);
            }
        }
        Some(Action::RestoreProject(id)) => {
            let when = view.commits.iter().find(|c| c.id == id).and_then(|c| c.time).map(|t| crate::i18n::ago(t, chrono::Utc::now()));
            app.view.dialog = Some(Dialog::Confirm(Confirm {
                title: tl!("Restore the project as it was {when}?").replace("{when}", when.as_deref().unwrap_or_default()),
                message: tl!(
                    "Every file comes back as it was then, and files added since leave the project, as unsaved changes. The next save makes a new version: the history keeps every version."
                )
                .to_owned(),
                action: tl!("Restore"),
                command: "history.restore",
                params: json!({"commit": id}),
            }));
        }
        Some(Action::Commit) => {
            app.view.dialog = Some(Dialog::Prompt(Prompt {
                title: tl!("Commit the Project"),
                label: tl!("What this commit changes"),
                value: String::new(),
                action: crate::i18n::tc("button", "Commit"),
                command: "history.commit",
                params: Box::new(|message| json!({"message": message})),
                focus: true,
            }));
        }
        None => {}
    }
    app.history = view;
}

/// The folder's remote (docs/forges.md, step 5): where it is, how far ahead or behind, Push and
/// Pull; or, with none, Save to GitHub and Save to GitLab.
fn remote_section(app: &mut App, ui: &mut egui::Ui, t: &Tokens, view: &mut HistoryView) {
    let key = format!("{:?}\n{:?}", view.key, app.session.git_busy());
    if view.remote_key.as_ref() != Some(&key) {
        view.remote = app.session.execute("git.remotes", &json!({})).unwrap_or_default();
        view.remote_key = Some(key);
    }
    let remote = view.remote.clone();
    let muted = |text: String| RichText::new(text).font(t.small()).color(t.text_muted);
    ui.horizontal_wrapped(|ui| {
        ui.add_space(t.unit * 2.0);
        let Some(name) = remote["remote"].as_str() else {
            ui.label(muted(tl!("No remote yet: save the project to GitHub or GitLab to keep it there too.").to_owned()));
            return;
        };
        let place = remote["web_url"].as_str().or(remote["url"].as_str()).unwrap_or_default();
        let shown = place.trim_start_matches("https://").to_owned();
        ui.label(muted(tl!("Remote {name}:").replace("{name}", name)));
        match remote["web_url"].as_str() {
            Some(url) => {
                ui.hyperlink_to(RichText::new(shown).font(t.small()), url);
            }
            None => {
                ui.label(muted(shown));
            }
        }
    });
    let state = match (remote["remote"].as_str(), remote["ahead"].as_u64(), remote["behind"].as_u64()) {
        (None, ..) => None,
        (Some(_), None, _) | (Some(_), _, None) => Some(tl!("Not pushed yet.").to_owned()),
        (Some(_), Some(0), Some(0)) => Some(tl!("Up to date with the remote, as last pushed or pulled.").to_owned()),
        (Some(_), Some(a), Some(b)) => {
            let mut parts = Vec::new();
            if a > 0 {
                parts.push(trn(a as usize, "{n} commit to push", "{n} commits to push"));
            }
            if b > 0 {
                parts.push(trn(b as usize, "{n} commit to pull", "{n} commits to pull"));
            }
            Some(format!("{}.", parts.join(", ")))
        }
    };
    if let Some(state) = state {
        ui.horizontal(|ui| {
            ui.add_space(t.unit * 2.0);
            ui.label(muted(state));
        });
    }
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 2.0);
        let commands: &[(&str, &str)] = if remote["remote"].is_string() {
            &[("git.push", tl!("Push")), ("git.pull", crate::i18n::tc("button", "Pull"))]
        } else {
            &[("forge.save_to_github", tl!("Save to GitHub…")), ("forge.save_to_gitlab", tl!("Save to GitLab…"))]
        };
        for (id, label) in commands {
            let can = texcraft_engine::cmd::find(id).map_or(Ok(()), |c| (c.enabled)(&app.session));
            let r = widgets::text_button(ui, t, label, can.is_ok());
            match &can {
                Err(reason) => {
                    let _ = r.on_disabled_hover_text(reason.as_str());
                }
                Ok(()) if r.clicked() => crate::ui_cmd::run(app, id),
                Ok(()) => {}
            }
        }
        if let Some(op) = app.session.git_busy()
            && let Some(text) = crate::forge_save::busy_text(app, op)
        {
            let said = ui.label(muted(text.to_owned()));
            crate::status::announce(ui, &said, text);
        }
    });
}

/// Push: the branch's commits to its remote, in the background.
pub(crate) fn push(app: &mut App) {
    if app.run("git.push", json!({"background": true})).is_some() {
        app.view.status = Some((tl!("Pushing…").to_owned(), false));
    }
}

/// Pull: the remote's new commits into the folder, in the background.
pub(crate) fn pull(app: &mut App) {
    if app.run("git.pull", json!({"background": true})).is_some() {
        app.view.status = Some((tl!("Pulling…").to_owned(), false));
    }
}

/// A push ended.
pub(crate) fn pushed(app: &mut App, done: &Value) {
    let at = format!("{}/{}", done["remote"].as_str().unwrap_or_default(), done["branch"].as_str().unwrap_or_default());
    let said = if done["up_to_date"] == true { tl!("{remote} has these commits already.") } else { tl!("Pushed to {remote}.") };
    app.view.status = Some((said.replace("{remote}", &at), false));
}

/// A pull ended: what it brought.
pub(crate) fn git_pulled(app: &mut App, done: &Value) {
    let at = format!("{}/{}", done["remote"].as_str().unwrap_or_default(), done["branch"].as_str().unwrap_or_default());
    let files: Vec<&str> = done["changed"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str()).collect();
    let said = if done["up_to_date"] == true {
        tl!("Up to date with {remote}.").replace("{remote}", &at)
    } else if done["merged"] == true {
        tl!("Merged {remote} into the project: {files}.").replace("{remote}", &at).replace("{files}", &files.join(", "))
    } else if files.is_empty() {
        tl!("Pulled {remote}: no file of the project changed.").replace("{remote}", &at)
    } else {
        tl!("Pulled {remote}: {files}.").replace("{remote}", &at).replace("{files}", &files.join(", "))
    };
    app.view.status = Some((said, false));
}

/// The confirmation before bringing back the compared file: as it was in the chosen version, or
/// just before it when that version deleted it.
fn restore_file(view: &HistoryView) -> Option<Dialog> {
    let diff = view.diff.as_ref()?;
    let commit = view.commits.iter().find(|c| c.id == diff.commit)?;
    let (version, title) = if diff.change == "deleted" {
        (commit.parent.clone()?, tl!("Restore {file} as it was before it was deleted?"))
    } else {
        (commit.id.clone(), tl!("Restore {file} as it was {when}?"))
    };
    let when = commit.time.map(|t| crate::i18n::ago(t, chrono::Utc::now())).unwrap_or_default();
    let file = diff.path.rsplit('/').next().unwrap_or(&diff.path);
    let params = json!({"commit": version, "path": diff.path});
    Some(Dialog::Confirm(Confirm {
        title: title.replace("{file}", file).replace("{when}", &when),
        message: tl!("It comes back as unsaved changes. The next save makes a new version: the history keeps every version.").to_owned(),
        action: tl!("Restore"),
        command: "history.restore",
        params,
    }))
}

/// The files a version changed, by name: at most three, then how many others.
fn changed_files(changes: &[(String, String)]) -> String {
    let names: Vec<&str> = changes.iter().map(|(p, _)| p.rsplit('/').next().unwrap_or(p)).collect();
    match names.len() {
        0 => tl!("no files").to_owned(),
        1..=3 => names.join(", "),
        n => format!("{}, {}", names[..2].join(", "), trn(n - 2, "and {n} other", "and {n} others")),
    }
}

/// A message's body: what follows its subject line, trimmed.
fn body(message: &str) -> &str {
    message.split_once('\n').map_or("", |(_, rest)| rest.trim())
}

/// A version: its message's subject, then when, who, and the files it changed, on a second line;
/// the message's body on hover (and as the row's description).
fn commit_row(ui: &mut egui::Ui, t: &Tokens, c: &Commit, selected: bool, now: chrono::DateTime<chrono::Utc>) -> egui::Response {
    let height = 2.0 * t.row_height - 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let message = c.message.lines().next().unwrap_or_default();
    let when = c.time.map(|time| crate::i18n::ago(time, now)).unwrap_or_default();
    let files = changed_files(&c.changes);
    let meta = format!("{when} · {} · {files}", c.author);
    let name = format!("{message}, {when}, {}, {files}", c.author);
    let details = body(&c.message);
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &name));
    ui.ctx().accesskit_node_builder(r.id, |node| {
        node.set_expanded(selected);
        if !details.is_empty() {
            node.set_description(details.to_owned());
        }
    });
    let r = if details.is_empty() { r } else { r.on_hover_text(details) };
    if !ui.is_rect_visible(rect) {
        return r;
    }
    let base = if selected { crate::theme::blend(t.frame, t.accent_soft) } else { t.frame };
    let fill = widgets::state_fill(t, &r, base);
    let inner = rect.shrink2(vec2(t.unit, 0.0));
    if fill != t.frame {
        ui.painter().rect_filled(inner, t.corner(), fill);
    }
    let x = rect.left() + 2.0 * t.unit;
    let icon = egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(t.icon_size, height / 2.0 + t.unit));
    crate::icons::paint(ui, icon, "git-commit-horizontal", t.icon_size, if selected { t.accent } else { t.text_muted });
    let left = x + t.icon_size + 2.0 * t.unit;
    let width = (rect.right() - left - t.unit).max(0.0);
    let line = |text: &str, font: egui::FontId, color: egui::Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        ui.painter().layout_job(job)
    };
    let top = line(message, if selected { t.strong() } else { t.body() }, t.text);
    let bottom = line(&meta, t.small(), t.text_muted);
    let gap = (height - top.size().y - bottom.size().y) / 3.0;
    ui.painter().galley(egui::pos2(left, rect.top() + gap), top.clone(), t.text);
    ui.painter().galley(egui::pos2(left, rect.top() + 2.0 * gap + top.size().y), bottom, t.text_muted);
    widgets::focus_ring(ui, t, &r, inner, t.corner());
    r
}

/// The compared file: its name, what changed, Restore, and the lines.
fn diff_view(ui: &mut egui::Ui, t: &Tokens, diff: &Diff, action: &mut Option<Action>) {
    widgets::hairline(ui, t);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        let name = diff.path.rsplit('/').next().unwrap_or(&diff.path);
        ui.add(egui::Label::new(RichText::new(name).font(t.strong()).color(t.text)).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, t, "x", tl!("Close the Comparison"), true).clicked() {
                *action = Some(Action::CloseDiff);
            }
            if diff.error.is_none() && widgets::text_button(ui, t, tl!("Restore This File…"), true).clicked() {
                *action = Some(Action::RestoreFile);
            }
        });
    });
    let summary = if let Some(e) = &diff.error {
        crate::i18n::msg(e)
    } else if diff.binary {
        tl!("{change}; not a text file, so its lines are not compared.").replace("{change}", change_word(&diff.change))
    } else {
        let added = trn(diff.added, "{n} line added", "{n} lines added");
        let removed = trn(diff.removed, "{n} line removed", "{n} lines removed");
        format!("{} · {added}, {removed}", change_word(&diff.change))
    };
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 2.0);
        let color = if diff.error.is_some() { t.error } else { t.text_muted };
        let label = ui.add(egui::Label::new(RichText::new(&summary).font(t.small()).color(color)).wrap());
        crate::status::announce(ui, &label, &format!("{}: {summary}", diff.path));
    });
    if diff.truncated {
        ui.horizontal(|ui| {
            ui.add_space(t.unit * 2.0);
            ui.label(RichText::new(tl!("The comparison is too long: only its start is shown.")).font(t.small()).color(t.warning));
        });
    }
    let row = t.code_line_height.max(t.row_height - 2.0 * t.unit);
    egui::ScrollArea::both().id_salt("history-diff-lines").auto_shrink([false, false]).show_rows(ui, row, diff.lines.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for line in &diff.lines[range] {
            diff_line(ui, t, line, row);
        }
    });
}

/// One line of the comparison: a + or − mark (not colour alone), its number, its text.
fn diff_line(ui: &mut egui::Ui, t: &Tokens, line: &Line, height: f32) {
    let font = t.code_font();
    let (mark, fill, said) = match line.kind {
        Kind::Added => ("+", Some(t.diff_added), crate::i18n::tc("line", "Added")),
        Kind::Removed => ("−", Some(t.diff_removed), crate::i18n::tc("line", "Removed")),
        Kind::Same => (" ", None, tl!("Unchanged")),
        Kind::Gap => ("⋯", None, tl!("Unchanged lines left out")),
    };
    let text = if line.kind == Kind::Gap { String::new() } else { line.text.replace('\t', "    ") };
    let text_width = ui.painter().layout_no_wrap(text.clone(), font.clone(), t.text).size().x;
    let gutter = 9.0 * t.unit;
    let width = ui.available_width().max(gutter + text_width + 2.0 * t.unit);
    let (rect, r) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let number = line.number.map(|n| n.to_string()).unwrap_or_default();
    let name = if line.kind == Kind::Gap { said.to_owned() } else { format!("{said} {number}: {}", line.text) };
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    if !ui.is_rect_visible(rect) {
        return;
    }
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    let y = rect.center().y;
    ui.painter().text(egui::pos2(rect.left() + 5.0 * t.unit, y), Align2::RIGHT_CENTER, &number, t.small(), t.text_muted);
    ui.painter().text(egui::pos2(rect.left() + 6.5 * t.unit, y), Align2::CENTER_CENTER, mark, font.clone(), t.text);
    ui.painter().text(egui::pos2(rect.left() + gutter, y), Align2::LEFT_CENTER, text, font, t.text);
}
