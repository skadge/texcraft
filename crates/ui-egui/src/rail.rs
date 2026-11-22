//! The rail (docs/design.md, Regions): side panel sections at the top, settings and help at the
//! bottom. Clicking the active section hides the side panel.

use egui::{Align, Layout};
use serde_json::json;

use crate::dialogs::Dialog;
use crate::prefs::ThemePref;
use crate::theme::Tokens;
use crate::{App, Side, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(0, 8));
    egui::Panel::left("rail").exact_size(t.rail_width).resizable(false).frame(frame).show(ui, |ui| {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = t.unit;
            for (side, icon, label) in [(Side::Files, "folder", tl!("Project Files")), (Side::Outline, "list-tree", tl!("Outline"))] {
                let active = app.view.side == Some(side);
                if widgets::rail_button(ui, &t, icon, label, active).clicked() {
                    app.view.side = if active { None } else { Some(side) };
                }
            }
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = t.unit;
                if widgets::rail_button(ui, &t, "circle-question-mark", tl!("Keyboard Shortcuts"), false).clicked() {
                    app.view.dialog = Some(Dialog::Shortcuts);
                }
                let settings = widgets::rail_button(ui, &t, "settings", tl!("Settings"), false);
                egui::Popup::menu(&settings).align(egui::RectAlign::RIGHT_END).show(|ui| settings_menu(app, ui));
            });
        });
    });
}

/// Quick settings; the full Settings dialog comes in phase 1 (docs/plan.md).
fn settings_menu(app: &mut App, ui: &mut egui::Ui) {
    let t: Tokens = app.tokens;
    ui.set_min_width(240.0);
    widgets::section_title(ui, &t, tl!("Appearance"));
    let themes = [ThemePref::System, ThemePref::Light, ThemePref::Dark];
    let selected = themes.iter().position(|th| *th == app.prefs.theme).unwrap_or(0);
    if let Some(i) = widgets::segmented(ui, &t, "theme", &[(tl!("System"), None), (tl!("Light"), None), (tl!("Dark"), None)], selected) {
        let ctx = ui.ctx().clone();
        app.set_theme(&ctx, themes.get(i).copied().unwrap_or_default());
    }
    ui.add_space(t.unit * 2.0);
    widgets::section_title(ui, &t, tl!("Compiling"));
    let mut auto = app.session.settings.auto_compile;
    if ui.checkbox(&mut auto, tl!("Compile automatically after typing")).changed() {
        app.run("build.auto", json!({"on": auto}));
    }
}
