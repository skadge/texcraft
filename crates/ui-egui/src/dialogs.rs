//! Modal dialogs: a one-line prompt that runs a command, a confirmation for actions that cannot
//! be undone, the keyboard shortcuts, and About. Each traps focus while open, closes with Escape
//! and returns focus to the window (standards/accessibility.md).

use egui::{Align, Layout, RichText};
use serde_json::Value;

use crate::App;
use crate::theme::Tokens;

/// A dialog that asks for one line of text, then runs a command with it.
pub(crate) struct Prompt {
    pub title: &'static str,
    pub label: &'static str,
    pub value: String,
    pub action: &'static str,
    pub command: &'static str,
    /// Builds the command's parameters from the text typed.
    pub params: Box<dyn Fn(&str) -> Value>,
    pub focus: bool,
}

/// A question before an action that cannot be undone.
pub(crate) struct Confirm {
    pub title: String,
    pub message: String,
    pub action: &'static str,
    pub command: &'static str,
    pub params: Value,
}

/// What happens once unsaved changes are saved or discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Next {
    Quit,
    NewProject,
    OpenProject(String),
    /// Choose a project folder, then open it.
    PickProject,
    /// Ask for a project on GitLab or GitHub to open.
    OpenFromForge,
    /// Make a new project from a zip file or a folder's files (`project.import` with these
    /// parameters), to keep under this name.
    ImportProject(Value, String),
}

pub(crate) enum Dialog {
    Prompt(Prompt),
    Confirm(Confirm),
    /// The project has unsaved changes and `Next` would lose them.
    Unsaved(Next),
    Shortcuts,
    Settings,
    About,
    GoToFile(crate::goto_file::Palette),
    /// The formatted view's picker of a citation, a cross-reference or a figure's image.
    Choose(Box<crate::insert_pick::Chooser>),
    ForgeOpen(crate::forge::OpenForm),
    ForgeCommit(crate::forge::CommitForm),
    ForgeResolve(crate::forge::ResolveForm),
    ForgeMergeRequest(crate::forge::MergeRequestNote),
    /// Save Project to GitHub or GitLab: a new repository for the project.
    ForgeSave(Box<crate::forge_save::SaveForm>),
    ForgeServer(crate::forge_server::ServerForm),
    ForgeProposal(crate::forge_server::ProposalForm),
    Figure(Box<crate::figure_dialog::FigureForm>),
    /// What explaining a message sends, before the first one goes.
    /// Make with AI: a description, the model's LaTeX typeset, Insert.
    AiMake(Box<crate::ai_make::MakeForm>),
    /// Add a Reference with AI: a title or DOI, the drafted entry.
    AiReference(Box<crate::ai_make::ReferenceForm>),
    /// Ask AI to Change This: how to change the selection, in the user's words.
    AiInstruct(Box<crate::ai::InstructForm>),
    /// A dropped zip file, or files already in the project: what to do with them.
    Drop(Box<crate::drop::DropForm>),
}

impl Dialog {
    /// The dialog's name for the control channel.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Dialog::Prompt(_) => "prompt",
            Dialog::Confirm(_) => "confirm",
            Dialog::Unsaved(_) => "unsaved",
            Dialog::Shortcuts => "shortcuts",
            Dialog::Settings => "settings",
            Dialog::About => "about",
            Dialog::GoToFile(_) => "go_to_file",
            Dialog::Choose(_) => "choose",
            Dialog::ForgeOpen(_) => "forge_open",
            Dialog::ForgeCommit(_) => "forge_commit",
            Dialog::ForgeResolve(_) => "forge_resolve",
            Dialog::ForgeMergeRequest(_) => "forge_merge_request",
            Dialog::ForgeSave(_) => "forge_save",
            Dialog::ForgeServer(_) => "forge_server",
            Dialog::ForgeProposal(_) => "forge_proposal",
            Dialog::Figure(_) => "figure",
            Dialog::AiMake(_) => "ai_make",
            Dialog::AiReference(_) => "ai_reference",
            Dialog::AiInstruct(_) => "ai_instruct",
            Dialog::Drop(_) => "drop",
        }
    }
}

/// Draws the open dialog when it draws its own window; whether it does.
fn own_window(app: &mut App, ctx: &egui::Context) -> bool {
    let draw: fn(&mut App, &egui::Context) = match app.view.dialog {
        Some(Dialog::Shortcuts) => shortcuts,
        Some(Dialog::Settings) => crate::settings::show,
        Some(Dialog::AiMake(_)) => crate::ai_make::make_dialog,
        Some(Dialog::AiReference(_)) => crate::ai_make::reference_dialog,
        Some(Dialog::AiInstruct(_)) => crate::ai::instruct_dialog,
        Some(Dialog::Drop(_)) => crate::drop::dialog,
        Some(Dialog::GoToFile(_)) => crate::goto_file::show,
        Some(Dialog::Choose(_)) => crate::insert_pick::show,
        Some(Dialog::ForgeServer(_) | Dialog::ForgeProposal(_)) => crate::forge_server::dialog,
        Some(Dialog::ForgeSave(_)) => crate::forge_save::dialog,
        Some(Dialog::ForgeOpen(_) | Dialog::ForgeCommit(_) | Dialog::ForgeResolve(_) | Dialog::ForgeMergeRequest(_)) => crate::forge::dialog,
        _ => return false,
    };
    draw(app, ctx);
    true
}

/// The window's open dialog, if any.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    if own_window(app, ctx) {
        return;
    }
    let project = app.project_name();
    let engine = app.session.backend_name();
    let Some(dialog) = app.view.dialog.as_mut() else { return };
    let t = app.tokens;
    let mut outcome = Outcome::Open;
    let shown = crate::sheet::show(ctx, &t, "texcraft-dialog", 400.0, crate::sheet::Leave::Close, |ui| match dialog {
        Dialog::Prompt(p) => outcome = prompt(ui, &t, p),
        Dialog::Confirm(c) => outcome = confirm(ui, &t, c),
        Dialog::Unsaved(_) => outcome = unsaved(ui, &t, &project),
        Dialog::Shortcuts
        | Dialog::Settings
        | Dialog::GoToFile(_)
        | Dialog::Choose(_)
        | Dialog::ForgeOpen(_)
        | Dialog::ForgeCommit(_)
        | Dialog::ForgeResolve(_)
        | Dialog::ForgeMergeRequest(_)
        | Dialog::ForgeSave(_)
        | Dialog::ForgeServer(_)
        | Dialog::ForgeProposal(_)
        | Dialog::AiMake(_)
        | Dialog::AiReference(_)
        | Dialog::AiInstruct(_)
        | Dialog::Drop(_) => {}
        Dialog::Figure(f) => outcome = crate::figure_dialog::form(ui, &t, f),
        Dialog::About => outcome = about(ui, &t, &engine),
    });
    if shown.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Open => {}
        Outcome::Cancel => app.view.dialog = None,
        Outcome::Run => match app.view.dialog.take() {
            // Go to Line moves the editor's caret: a view action, not an engine command.
            Some(Dialog::Prompt(p)) if p.command == "view.goto_line" => match (p.value.trim().parse::<u32>(), app.session.current().cloned()) {
                (Ok(line), Some(path)) if line > 0 => {
                    app.view.goto = Some((path, line));
                    app.editor.focus = true;
                }
                _ => app.view.status = Some((tl!("Type a line number, from 1.").to_owned(), true)),
            },
            // The formatted view's link, footnote and formula: an edit of the text.
            Some(Dialog::Prompt(p)) if p.command.starts_with("edit.") => {
                let params = (p.params)(p.value.trim());
                crate::ui_cmd::run_with(app, p.command, &params);
            }
            Some(Dialog::Prompt(p)) => {
                let params = (p.params)(p.value.trim());
                if let Some(done) = app.run(p.command, params) {
                    match p.command {
                        "project.save_as" => {
                            app.remember_project();
                            if let Some(next) = app.after_save.take() {
                                app.proceed(next);
                            }
                        }
                        "project.from_template" => app.template_project_opened(),
                        "build.export" => crate::ui_cmd::export_started(app, &done),
                        "build.save_output" => {
                            let said = tl!("Saved to {path}.").replace("{path}", done["path"].as_str().unwrap_or_default());
                            app.view.status = Some((said, false));
                        }
                        "history.commit" => {
                            let said = if done["commit"].is_string() { tl!("Committed.") } else { tl!("Nothing changed since the last commit.") };
                            app.view.status = Some((said.to_owned(), false));
                        }
                        _ => {}
                    }
                }
            }
            Some(Dialog::Confirm(c)) => match app.run(c.command, c.params.clone()) {
                Some(_) if c.command == "project.delete" => {
                    if let Some(name) = c.params["name"].as_str() {
                        app.prefs.recent.retain(|p| p != name);
                        app.prefs_changed = true;
                    }
                }
                Some(_) if c.command == "history.restore" => {
                    let said = tl!("Restored, as unsaved changes: the next save makes a new version.");
                    app.view.status = Some((said.to_owned(), false));
                }
                Some(done) if c.command == "project.replace" => {
                    let n = done["replaced"].as_u64().unwrap_or(0) as usize;
                    let files = done["files"].as_array().map_or(0, Vec::len);
                    let said = tl!("Replaced {matches} in {files}.")
                        .replace("{matches}", &crate::i18n::trn(n, "{n} match", "{n} matches"))
                        .replace("{files}", &crate::i18n::trn(files, "{n} file", "{n} files"));
                    app.view.status = Some((said, false));
                }
                _ => {}
            },
            Some(Dialog::Unsaved(next)) => app.save_then(next),
            Some(Dialog::Figure(f)) => crate::figure_dialog::apply(app, &f),
            _ => {}
        },
        Outcome::Discard => {
            if let Some(Dialog::Unsaved(next)) = app.view.dialog.take() {
                app.proceed(next);
            }
        }
    }
}

pub(crate) enum Outcome {
    Open,
    Cancel,
    Run,
    /// Go on without saving.
    Discard,
}

fn unsaved(ui: &mut egui::Ui, t: &Tokens, project: &str) -> Outcome {
    title(ui, t, &tl!("Save the changes to {project}?").replace("{project}", project));
    ui.label(tl!("Changes that are not saved are lost."));
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        let save = egui::Button::new(RichText::new(tl!("Save")).color(t.on_accent).font(t.strong())).fill(t.accent).stroke(egui::Stroke::NONE);
        if ui.add(save.min_size(egui::vec2(88.0, 30.0))).clicked() {
            outcome = Outcome::Run;
        }
        if ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new(tl!("Don't Save")).color(t.error)).min_size(egui::vec2(96.0, 30.0))).clicked() {
                outcome = Outcome::Discard;
            }
        });
    });
    outcome
}

/// The dialog's title: at the top of its box, or of a phone's sheet ([`crate::sheet::title`]).
pub(crate) fn title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    crate::sheet::title(ui, t, text);
}

/// The dialog's actions, from the right: the last row of its box, or a phone's sheet's bottom
/// bar ([`crate::sheet::actions`]).
pub(crate) fn actions(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    crate::sheet::actions(ui, add);
}

/// Cancel and the primary action, right-aligned. Returns the outcome chosen.
pub(crate) fn buttons(ui: &mut egui::Ui, t: &Tokens, action: &str, danger: bool, enabled: bool) -> Outcome {
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        let fill = if danger { t.error } else { t.accent };
        let label = RichText::new(action).color(t.on_accent).font(t.strong());
        let primary = egui::Button::new(label).fill(fill).stroke(egui::Stroke::NONE).min_size(egui::vec2(88.0, 30.0));
        if ui.add_enabled(enabled, primary).clicked() {
            outcome = Outcome::Run;
        }
        if ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}

fn prompt(ui: &mut egui::Ui, t: &Tokens, p: &mut Prompt) -> Outcome {
    title(ui, t, p.title);
    let label = ui.label(RichText::new(p.label).color(t.text_muted));
    // The field is named by the label above it (AGENTS.md rule 21).
    let edit = ui.add(egui::TextEdit::singleline(&mut p.value).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0))).labelled_by(label.id);
    if p.focus {
        edit.request_focus();
        p.focus = false;
    }
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let ready = !p.value.trim().is_empty();
    match buttons(ui, t, p.action, false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}

fn confirm(ui: &mut egui::Ui, t: &Tokens, c: &Confirm) -> Outcome {
    title(ui, t, &c.title);
    ui.label(c.message.as_str());
    buttons(ui, t, c.action, true, true)
}

/// The shared Keyboard Shortcuts dialog (craft-keymap): every command, its shortcuts, editing
/// and conflicts. Changes are kept in the preferences.
///
/// The dialog's body is hosted in a modal of ours, so the window behind it cannot take the keyboard
/// while it is open; the body draws its own OK and Cancel and handles Escape.
fn shortcuts(app: &mut App, ctx: &egui::Context) {
    // A state closed with OK or Cancel shows nothing: opening the dialog again starts a new one.
    if !app.keys.dialog.is_open() {
        app.keys.dialog = craft_keymap::ui::DialogState::new();
    }
    let text = crate::i18n::keymap_text();
    let t = app.tokens;
    let screen = ctx.content_rect().size();
    let size = (screen * 0.8).min(egui::vec2(1320.0, 900.0)).max(egui::vec2(240.0, 180.0)).min(screen - egui::vec2(48.0, 48.0));
    let mut outcome = None;
    egui::Modal::new(egui::Id::unique("texcraft-shortcuts")).frame(crate::widgets::dialog_frame(ctx)).show(ctx, |ui| {
        title(ui, &t, tl!("Keyboard Shortcuts"));
        let body = egui::vec2(size.x, (size.y - ui.min_rect().height()).max(120.0));
        ui.allocate_ui(body, |ui| {
            ui.set_min_size(body);
            outcome = craft_keymap::ui::ShortcutsDialog::new(&mut app.keys.keymap, &text).ui(ui, &mut app.keys.dialog);
        });
    });
    match outcome {
        Some(craft_keymap::ui::Outcome::Ok) => {
            app.prefs.shortcuts = app.keys.keymap.overrides().clone();
            app.prefs_changed = true;
            app.view.dialog = None;
        }
        Some(craft_keymap::ui::Outcome::Changed) => {
            app.prefs.shortcuts = app.keys.keymap.overrides().clone();
            app.prefs_changed = true;
        }
        Some(craft_keymap::ui::Outcome::Cancel) => app.view.dialog = None,
        _ => {}
    }
}

fn about(ui: &mut egui::Ui, t: &Tokens, engine: &str) -> Outcome {
    ui.vertical_centered(|ui| {
        let (icon, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
        crate::app_icon::paint(ui, icon);
        ui.add_space(t.unit * 2.0);
        title(ui, t, "TeXCraft");
        ui.label(tf!("Version {version}", version = env!("CARGO_PKG_VERSION")));
        ui.label(RichText::new(tf!("Typesetting engine: {engine}", engine = engine)).color(t.text_muted));
        ui.add_space(t.unit * 2.0);
        ui.label(
            RichText::new(tl!("Free software under the MIT or Apache 2.0 licence. Icons from Lucide (ISC).")).font(t.small()).color(t.text_muted),
        );
    });
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}
