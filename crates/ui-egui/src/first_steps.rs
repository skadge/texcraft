//! The first-steps hint: one quiet line above the editor and the PDF, shown with the first
//! project until it is dismissed, that points at what a new user may not find alone: the
//! Formatted switch, which shows the document as it reads and edits it there. Not a tour: it
//! takes no keyboard and hides nothing, and Help › First Steps shows it again.

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
/// One line: the text gives way (cut short, whole in its tooltip) to Dismiss at its end.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.prefs.first_steps_seen {
        return;
    }
    let t = app.tokens;
    let mut dismiss = false;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin { left: 12, right: 4, top: 0, bottom: 0 });
    let strip = egui::Panel::top("first-steps").frame(frame).show(ui, |ui| {
        let region = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Region).accessibility_label(tl!("First steps"));
        ui.scope_builder(region, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.unit * 1.5;
                let (icon, _) = ui.allocate_exact_size(vec2(t.icon_size, t.icon_size), egui::Sense::hover());
                crate::icons::paint(ui, icon, "info", t.icon_size - 2.0, t.accent_muted);
                let room = widgets::room_before_right(ui, "first-steps-buttons").max(40.0);
                let text = tl!("New here? Formatted, above the text, shows your document as it reads, and you can edit it there.");
                ui.allocate_ui_with_layout(vec2(room, t.row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(text).font(t.small()).color(t.text_muted)).truncate());
                });
                widgets::right_aligned(ui, "first-steps-buttons", |ui| {
                    dismiss = widgets::text_button(ui, &t, tl!("Dismiss"), true).clicked();
                });
            });
        });
    });
    let rect = strip.response.rect;
    ui.painter().hline(rect.x_range(), rect.bottom(), t.hairline());
    if dismiss {
        crate::ui_cmd::run(app, "help.first_steps");
    }
}
