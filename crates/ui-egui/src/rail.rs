//! The rail (docs/design.md, Regions): side panel sections at the top, settings and help at the
//! bottom. Clicking the active section hides the side panel.

use egui::{Align, Layout};

use crate::{App, Side, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(0, 8));
    let shown = egui::Panel::left("rail").exact_size(t.rail_width).resizable(false).frame(frame).show(ui, |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = t.unit;
            // The sections are a tab list; the two buttons at the bottom open dialogs.
            let list = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::TabList).accessibility_label(tl!("Sections"));
            ui.scope_builder(list, |ui| {
                let sections = [
                    (Side::Files, "folder", tl!("Project Files")),
                    (Side::Outline, "list-tree", tl!("Outline")),
                    (Side::Search, "search", tl!("Search")),
                    (Side::History, "rotate-ccw-clock", tl!("History")),
                    (Side::Review, "message-square-text", tl!("Review")),
                    (Side::Ask, crate::ai::ICON, tl!("Ask AI")),
                ];
                for (side, icon, label) in sections {
                    let active = app.view.side == Some(side);
                    let r = widgets::rail_button(ui, &t, icon, label, Some(active));
                    app.region(crate::Region::Rail, r.id);
                    if active {
                        app.view.rail_active = Some(r.id);
                    }
                    if r.clicked() {
                        app.view.side = if active { None } else { Some(side) };
                        if !active {
                            app.view.rail_active = Some(r.id);
                        }
                    }
                }
            });
            // At the rail's foot, read top to bottom as they are shown: Settings, then Keyboard
            // Shortcuts.
            let room = ui.available_rect_before_wrap();
            let height = 2.0 * t.rail_button + t.unit;
            let foot = egui::Rect::from_min_max(egui::pos2(room.left(), (room.bottom() - height).max(room.top())), room.max);
            let mut chosen = None;
            ui.scope_builder(egui::UiBuilder::new().max_rect(foot).layout(Layout::top_down(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.y = t.unit;
                if widgets::rail_button(ui, &t, "settings", tl!("Settings"), None).clicked() {
                    chosen = Some("app.settings");
                }
                if widgets::rail_button(ui, &t, "circle-question-mark", tl!("Keyboard Shortcuts"), None).clicked() {
                    chosen = Some("help.shortcuts");
                }
            });
            if let Some(id) = chosen {
                crate::ui_cmd::run(app, id);
            }
        });
    });
    app.region_area(crate::Region::Rail, shown.response.rect);
}
