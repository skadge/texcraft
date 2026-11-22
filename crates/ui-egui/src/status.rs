//! The status bar (docs/design.md, Regions): the build's state and last result on the left, the
//! caret's place and the engine on the right. Build results are also announced to screen readers.

use egui::{Align, Layout, Sense, vec2};
use texcraft_engine::Status;

use crate::i18n::trn;
use crate::{App, icons, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(12, 0));
    egui::Panel::bottom("status").exact_size(t.status_height).frame(frame).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = t.unit * 1.5;
            let (icon, color, text) = state(app);
            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            match icon {
                None => widgets::spinner(ui, rect.center(), 5.5, t.accent),
                Some(name) => icons::paint(ui, rect, name, 14.0, color),
            }
            let label = ui.label(egui::RichText::new(&text).font(t.small()).color(if icon.is_some() { color } else { t.text_muted }));
            announce(ui, &label, &text);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = t.unit * 4.0;
                let small = |s: String| egui::RichText::new(s).font(t.small()).color(t.text_muted);
                ui.label(small(app.session.backend_name()));
                ui.label(small("LaTeX".to_owned()));
                if let Some((line, col)) = app.view.cursor {
                    ui.label(small(tl!("Ln {line}, Col {col}").replace("{line}", &line.to_string()).replace("{col}", &col.to_string())));
                }
                // Vim's mode, in words as well as by the caret's shape; announced when it changes.
                if let Some(vim) = crate::editor::vim_status(app) {
                    let label = ui.label(egui::RichText::new(&vim).font(egui::FontId::monospace(t.small().size)).color(t.text));
                    announce(ui, &label, &vim);
                }
            });
        });
    });
}

/// The icon (`None` while building), its colour and the text describing the build.
fn state(app: &App) -> (Option<&'static str>, egui::Color32, String) {
    let t = &app.tokens;
    if app.session.is_building() {
        return (None, t.text_muted, tl!("Compiling…").to_owned());
    }
    // A message from the last command (an error, or where the focus went) until the next build.
    if let Some((message, error)) = &app.view.status {
        return if *error { (Some("circle-exclamation-point"), t.error, message.clone()) } else { (Some("info"), t.text_muted, message.clone()) };
    }
    let Some(o) = app.session.last_outcome() else {
        return (Some("circle-check"), t.text_muted, tl!("Not compiled yet").to_owned());
    };
    match &o.status {
        Status::Unavailable(reason) => (Some("circle-exclamation-point"), t.error, reason.clone()),
        Status::Failed => (Some("circle-exclamation-point"), t.error, trn(o.errors(), "Failed with {n} error", "Failed with {n} errors")),
        Status::Built if o.errors() > 0 => {
            (Some("circle-exclamation-point"), t.error, trn(o.errors(), "Compiled with {n} error", "Compiled with {n} errors"))
        }
        Status::Built if o.warnings() > 0 => (
            Some("triangle-exclamation-point"),
            t.warning,
            trn(o.warnings(), "Compiled with {n} warning", "Compiled with {n} warnings") + &seconds(o.seconds),
        ),
        Status::Built => (Some("circle-check"), t.success, tl!("Compiled").to_owned() + &seconds(o.seconds)),
    }
}

fn seconds(s: f64) -> String {
    format!(" · {s:.1} s")
}

/// Announces `text` through a live region when it changes (AGENTS.md rule 21).
fn announce(ui: &egui::Ui, label: &egui::Response, text: &str) {
    ui.ctx().accesskit_node_builder(label.id, |node| {
        node.set_live(egui::accesskit::Live::Polite);
        node.set_label(text);
    });
}
