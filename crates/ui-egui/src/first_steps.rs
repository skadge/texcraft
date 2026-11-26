//! The first-steps hint: a strip above the editor and the PDF, shown with the first project until
//! it is dismissed, that says where three things are that a new user looks for: the formatted
//! view, the build log and the user guide. Not a tour: it takes no keyboard and hides nothing, and
//! Help › First Steps shows it again.

use egui::{RichText, vec2};

use crate::{App, widgets};

/// `help.first_steps`: shows the hint again, or dismisses it.
pub(crate) fn toggle(app: &mut App) {
    app.prefs.first_steps_seen = !app.prefs.first_steps_seen;
    app.prefs_changed = true;
    if app.prefs.first_steps_seen {
        app.view.status = Some((tl!("Hint dismissed. Help › First Steps shows it again.").to_owned(), false));
    }
}

/// The strip, while it is not dismissed; call before the panes, after the rail and side panel.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.prefs.first_steps_seen {
        return;
    }
    let t = app.tokens;
    let mut chosen = None;
    let frame = egui::Frame::new().fill(crate::theme::blend(t.surface, t.accent_soft)).inner_margin(egui::Margin::symmetric(12, 8));
    egui::Panel::top("first-steps").frame(frame).show(ui, |ui| {
        let region = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Region).accessibility_label(tl!("First steps"));
        ui.scope_builder(region, |ui| {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                crate::icons::paint(ui, icon, "info", 16.0, t.accent);
                // The text wraps in the room the buttons leave; the buttons follow it at the
                // row's end, in reading order.
                let room = widgets::room_before_right(ui, "first-steps-buttons").max(120.0);
                ui.allocate_ui(vec2(room, 0.0), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(tl!("New here? Formatted, above the text, shows your document as it reads, and you can edit it there. The PDF's Build log explains any error. Help › User Guide (F1) has the rest.")).color(t.text));
                    });
                });
                widgets::right_aligned(ui, "first-steps-buttons", |ui| {
                    if ui.button(tl!("Open the Guide")).clicked() {
                        chosen = Some("help.guide");
                    }
                    if ui.button(tl!("Dismiss")).clicked() {
                        chosen = Some("help.first_steps");
                    }
                });
            });
        });
    });
    if let Some(id) = chosen {
        crate::ui_cmd::run(app, id);
    }
}
