//! The rail (docs/design.md, Regions): side panel sections at the top, settings and help at the
//! bottom. Clicking the active section hides the side panel.

use egui::{Align, Layout};

use crate::dialogs::Dialog;
use crate::{App, Side, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(0, 8));
    let shown = egui::Panel::left("rail").exact_size(t.rail_width).resizable(false).frame(frame).show(ui, |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = t.unit;
            let sections = [
                (Side::Files, "folder", tl!("Project Files")),
                (Side::Outline, "list-tree", tl!("Outline")),
                (Side::Search, "search", tl!("Search")),
                (Side::History, "rotate-ccw-clock", tl!("History")),
                (Side::Review, "message-square-text", tl!("Review")),
            ];
            for (side, icon, label) in sections {
                let active = app.view.side == Some(side);
                let r = widgets::rail_button(ui, &t, icon, label, active);
                app.region(crate::Region::Rail, r.id);
                if r.clicked() {
                    app.view.side = if active { None } else { Some(side) };
                }
            }
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = t.unit;
                if widgets::rail_button(ui, &t, "circle-question-mark", tl!("Keyboard Shortcuts"), false).clicked() {
                    app.view.dialog = Some(Dialog::Shortcuts);
                }
                if widgets::rail_button(ui, &t, "settings", tl!("Settings"), false).clicked() {
                    app.view.dialog = Some(Dialog::Settings);
                }
            });
        });
    });
    app.region_area(crate::Region::Rail, shown.response.rect);
}
