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
            });
        });
    });
}

/// The icon (`None` while building), its colour and the text describing the build.
fn state(app: &App) -> (Option<&'static str>, egui::Color32, String) {
    let t = &app.tokens;
    if app.session.is_building() {
        let text = app.view.progress.as_ref().map_or_else(|| tl!("Compiling…").to_owned(), |(p, s)| progress_text(p, *s));
        return (None, t.text_muted, text);
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
        Status::Cancelled => (Some("info"), t.text_muted, tl!("Compiling stopped").to_owned()),
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

/// A running build's progress in words: the page being built (of how many the last pass had), the
/// pass and why it runs, or the step between passes, and the time so far.
pub(crate) fn progress_text(p: &texcraft_engine::Progress, seconds: f64) -> String {
    let n = |v: u32| v.to_string();
    let mut text = if let Some(step) = &p.step {
        tl!("Compiling: {step}").replace("{step}", step)
    } else if p.pages > 0 {
        match p.expected {
            Some(e) if e >= p.pages => tl!("Compiling page {n} of {total}").replace("{n}", &n(p.pages)).replace("{total}", &n(e)),
            _ => tl!("Compiling page {n}").replace("{n}", &n(p.pages)),
        }
    } else if let Some(file) = &p.reading {
        tl!("Compiling: reading {file}").replace("{file}", file.rsplit('/').next().unwrap_or(file))
    } else {
        tl!("Compiling…").to_owned()
    };
    if p.pass > 1 {
        let pass = match &p.reason {
            Some(why) => tl!(" · pass {n} ({why})").replace("{n}", &n(p.pass)).replace("{why}", why),
            None => tl!(" · pass {n}").replace("{n}", &n(p.pass)),
        };
        text.push_str(&pass);
    }
    if seconds >= 1.0 {
        text.push_str(&format!(" · {seconds:.0} s"));
    }
    text
}

/// How far the build is through its pass, when the last pass's page count says.
pub(crate) fn progress_ratio(p: &texcraft_engine::Progress) -> Option<f32> {
    p.expected.filter(|e| *e > 0).map(|e| (p.pages as f32 / e as f32).clamp(0.0, 1.0))
}

fn seconds(s: f64) -> String {
    format!(" · {s:.1} s")
}

/// Announces `text` through a live region when it changes (AGENTS.md rule 21).
pub(crate) fn announce(ui: &egui::Ui, label: &egui::Response, text: &str) {
    ui.ctx().accesskit_node_builder(label.id, |node| {
        node.set_live(egui::accesskit::Live::Polite);
        node.set_label(text);
    });
}
