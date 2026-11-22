//! Modal dialogs: a one-line prompt that runs a command, a confirmation for actions that cannot
//! be undone, the keyboard shortcuts, and About. Each traps focus while open, closes with Escape
//! and returns focus to the window (standards/accessibility.md).

use egui::{Align, Layout, RichText};
use serde_json::Value;
use texcraft_engine::cmd;

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
    pub params: fn(&str) -> Value,
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

pub(crate) enum Dialog {
    Prompt(Prompt),
    Confirm(Confirm),
    Shortcuts,
    About,
}

/// The window's open dialog, if any.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = app.view.dialog.as_mut() else { return };
    let t = app.tokens;
    let mut outcome = Outcome::Open;
    let width = match dialog {
        Dialog::Shortcuts => 520.0,
        _ => 400.0,
    };
    let modal = egui::Modal::new(egui::Id::unique("texcraft-dialog")).show(ctx, |ui| {
        ui.set_width(width);
        match dialog {
            Dialog::Prompt(p) => outcome = prompt(ui, &t, p),
            Dialog::Confirm(c) => outcome = confirm(ui, &t, c),
            Dialog::Shortcuts => outcome = shortcuts(ui, &t),
            Dialog::About => outcome = about(ui, &t, &app.session.backend_name()),
        }
    });
    if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Open => {}
        Outcome::Cancel => app.view.dialog = None,
        Outcome::Run => match app.view.dialog.take() {
            Some(Dialog::Prompt(p)) => {
                let params = (p.params)(p.value.trim());
                app.run(p.command, params);
            }
            Some(Dialog::Confirm(c)) => {
                app.run(c.command, c.params);
            }
            _ => {}
        },
    }
}

enum Outcome {
    Open,
    Cancel,
    Run,
}

fn title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).font(t.title()));
    ui.add_space(t.unit * 2.0);
}

/// Cancel and the primary action, right-aligned. Returns the outcome chosen.
fn buttons(ui: &mut egui::Ui, t: &Tokens, action: &str, danger: bool, enabled: bool) -> Outcome {
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
    ui.label(RichText::new(p.label).color(t.text_muted));
    let edit = ui.add(egui::TextEdit::singleline(&mut p.value).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)));
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

fn shortcuts(ui: &mut egui::Ui, t: &Tokens) -> Outcome {
    title(ui, t, tl!("Keyboard Shortcuts"));
    let mut rows: Vec<(&str, String)> = cmd::all().iter().filter_map(|c| c.shortcut.map(|s| (c.label, platform_keys(s)))).collect();
    rows.extend([(tl!("Find in file"), platform_keys("Cmd+F")), (tl!("Undo"), platform_keys("Cmd+Z")), (tl!("Redo"), platform_keys("Cmd+Shift+Z"))]);
    egui::Grid::new("shortcuts").num_columns(2).spacing(egui::vec2(t.unit * 8.0, t.unit * 2.0)).show(ui, |ui| {
        for (label, keys) in rows {
            ui.label(crate::i18n::t(label));
            ui.horizontal(|ui| {
                for key in keys.split('+') {
                    key_cap(ui, t, key);
                }
            });
            ui.end_row();
        }
    });
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}

fn key_cap(ui: &mut egui::Ui, t: &Tokens, key: &str) {
    egui::Frame::new()
        .fill(t.frame)
        .stroke(t.hairline())
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| ui.label(RichText::new(key).font(t.small())));
}

/// `Cmd+S` written the way this platform's users read it.
pub(crate) fn platform_keys(spec: &str) -> String {
    let cmd = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    spec.replace("Cmd", cmd)
}

fn about(ui: &mut egui::Ui, t: &Tokens, engine: &str) -> Outcome {
    title(ui, t, "TeXCraft");
    ui.label(format!("{} {}", tl!("Version"), env!("CARGO_PKG_VERSION")));
    ui.label(RichText::new(format!("{} {engine}", tl!("Typesetting engine:"))).color(t.text_muted));
    ui.add_space(t.unit * 2.0);
    ui.label(RichText::new(tl!("Free software under the MIT or Apache 2.0 licence. Icons from Lucide (ISC).")).font(t.small()).color(t.text_muted));
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}
