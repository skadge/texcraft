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
}

pub(crate) enum Dialog {
    Prompt(Prompt),
    Confirm(Confirm),
    /// The project has unsaved changes and `Next` would lose them.
    Unsaved(Next),
    Shortcuts,
    Settings,
    About,
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
        }
    }
}

/// The window's open dialog, if any.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    if matches!(app.view.dialog, Some(Dialog::Shortcuts)) {
        shortcuts(app, ctx);
        return;
    }
    if matches!(app.view.dialog, Some(Dialog::Settings)) {
        crate::settings::show(app, ctx);
        return;
    }
    let project = app.project_name();
    let engine = app.session.backend_name();
    let Some(dialog) = app.view.dialog.as_mut() else { return };
    let t = app.tokens;
    let mut outcome = Outcome::Open;
    let width = 400.0;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-dialog")).frame(crate::widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(width);
        match dialog {
            Dialog::Prompt(p) => outcome = prompt(ui, &t, p),
            Dialog::Confirm(c) => outcome = confirm(ui, &t, c),
            Dialog::Unsaved(_) => outcome = unsaved(ui, &t, &project),
            Dialog::Shortcuts | Dialog::Settings => {}
            Dialog::About => outcome = about(ui, &t, &engine),
        }
    });
    if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
            Some(Dialog::Prompt(p)) => {
                let params = (p.params)(p.value.trim());
                if app.run(p.command, params).is_some() && p.command == "project.save_as" {
                    app.remember_project();
                    if let Some(next) = app.after_save.take() {
                        app.proceed(next);
                    }
                }
            }
            Some(Dialog::Confirm(c)) => {
                if app.run(c.command, c.params.clone()).is_some()
                    && c.command == "project.delete"
                    && let Some(name) = c.params["name"].as_str()
                {
                    app.prefs.recent.retain(|p| p != name);
                    app.prefs_changed = true;
                }
            }
            Some(Dialog::Unsaved(next)) => app.save_then(next),
            _ => {}
        },
        Outcome::Discard => {
            if let Some(Dialog::Unsaved(next)) = app.view.dialog.take() {
                app.proceed(next);
            }
        }
    }
}

enum Outcome {
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
    button_row(ui, |ui| {
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

fn title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).font(t.title()));
    ui.add_space(t.unit * 2.0);
}

/// Cancel and the primary action, right-aligned. Returns the outcome chosen.
fn buttons(ui: &mut egui::Ui, t: &Tokens, action: &str, danger: bool, enabled: bool) -> Outcome {
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    button_row(ui, |ui| {
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
fn shortcuts(app: &mut App, ctx: &egui::Context) {
    let text = crate::i18n::keymap_text();
    let outcome = craft_keymap::ui::ShortcutsDialog::new(&mut app.keys.keymap, &text).show(ctx, &mut app.keys.dialog);
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
        ui.label(format!("{} {}", tl!("Version"), env!("CARGO_PKG_VERSION")));
        ui.label(RichText::new(format!("{} {engine}", tl!("Typesetting engine:"))).color(t.text_muted));
        ui.add_space(t.unit * 2.0);
        ui.label(
            RichText::new(tl!("Free software under the MIT or Apache 2.0 licence. Icons from Lucide (ISC).")).font(t.small()).color(t.text_muted),
        );
    });
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    button_row(ui, |ui| {
        if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}

/// A row of buttons from the right edge, as tall as a button: a layout from the right would
/// otherwise take the dialog's whole height and push its buttons down.
fn button_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), Layout::right_to_left(Align::Center), add);
}
