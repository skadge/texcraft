//! The status bar (docs/design.md, Regions): the build's state and last result on the left, the
//! caret's place and the engine on the right. Build results are also announced to screen readers.

use egui::{Align, Layout, Sense, vec2};
use texcraft_engine::Status;

use crate::i18n::trn;
use crate::{App, icons, widgets};

/// Makes room for the status bar at the window's bottom, and returns where its content goes. It is
/// drawn last ([`show`]), so the keyboard reaches its controls after the window's others, in the
/// order they read.
pub(crate) fn reserve(app: &App, ui: &mut egui::Ui) -> egui::Rect {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(12, 0));
    egui::Panel::bottom("status").exact_size(t.status_height).frame(frame).show(ui, |ui| ui.max_rect()).inner
}

/// The status bar's content, in the room [`reserve`] made.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, room: egui::Rect) {
    let t = app.tokens;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(room).layout(Layout::left_to_right(Align::Center)));
    let ui = &mut child;
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
        // At the bar's end, made in the order they read: the caret, the language, the comments,
        // the engine, and focus mode's way out.
        widgets::right_aligned(ui, "status-end", |ui| {
            ui.spacing_mut().item_spacing.x = t.unit * 4.0;
            let small = |s: String| egui::RichText::new(s).font(t.small()).color(t.text_muted);
            // The file's language, the comments and the caret belong to the editor, not the start page.
            if !app.start {
                if let Some((line, col)) = app.view.cursor {
                    ui.label(small(tl!("Ln {line}, Col {col}").replace("{line}", &line.to_string()).replace("{col}", &col.to_string())));
                }
                ui.label(small("LaTeX".to_owned()));
                crate::review::status(app, ui);
            }
            ui.label(small(app.session.backend_name()));
            if app.view.focus_mode && !app.start {
                leave_focus_mode(app, ui);
            }
        });
    });
}

/// Focus mode's way out, at the status bar's right end: a button, with its shortcut beside it.
fn leave_focus_mode(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let button = egui::Button::new(egui::RichText::new(tl!("Leave Focus Mode")).font(t.small()).color(t.text)).frame_when_inactive(false);
    let r = ui.add(button);
    widgets::focus_ring(ui, &t, &r, r.rect, t.corner());
    let tip = tl!("Back to the menu bar, the sections and the side panel. Escape does it too when no field has the keyboard.");
    if r.on_hover_text(tip).clicked() {
        crate::ui_cmd::run(app, "view.focus_mode");
        // The button goes with focus mode: the keyboard goes to the text.
        app.editor.focus = true;
    }
    if let Some(keys) = app.keys.shortcut_text("view.focus_mode") {
        ui.label(egui::RichText::new(keys).font(t.small()).color(t.text_muted));
    }
}

/// The icon (`None` while building), its colour and the text describing the build.
fn state(app: &App) -> (Option<&'static str>, egui::Color32, String) {
    let t = &app.tokens;
    if let Some(text) = crate::forge::busy_text(app) {
        return (None, t.text_muted, text);
    }
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

/// What a running build is doing now: the page being built (of how many the last pass had), the
/// file being read, or the step between passes.
pub(crate) fn progress_now(p: &texcraft_engine::Progress) -> String {
    let n = |v: u32| v.to_string();
    if let Some(step) = &p.step {
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
    }
}

/// Why the pass runs, shortened: retex lists the files that changed, which can be many.
fn reason(why: &str) -> String {
    let parts: Vec<&str> = why.split(", ").filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [] => String::new(),
        [a] => (*a).to_owned(),
        [a, b] => format!("{a}, {b}"),
        [a, b, rest @ ..] => tl!("{a}, {b} and {n} more").replace("{a}", a).replace("{b}", b).replace("{n}", &rest.len().to_string()),
    }
}

/// The pass and why it runs, and the time so far: the detail under what the build is doing.
pub(crate) fn progress_detail(p: &texcraft_engine::Progress, seconds: f64) -> String {
    let mut parts = Vec::new();
    if p.pass > 1 {
        let pass = tl!("pass {n}").replace("{n}", &p.pass.to_string());
        parts.push(match p.reason.as_deref().map(reason).filter(|r| !r.is_empty()) {
            Some(why) => format!("{pass} · {why}"),
            None => pass,
        });
    }
    if seconds >= 1.0 {
        parts.push(format!("{seconds:.0} s"));
    }
    parts.join(" · ")
}

/// The page being built and the pass ("Compiling page 3 of 7 · pass 2"), for the PDF pane's
/// toolbar.
pub(crate) fn progress_line(p: &texcraft_engine::Progress) -> String {
    let mut text = progress_now(p);
    if p.pass > 1 {
        text.push_str(&tl!(" · pass {n}").replace("{n}", &p.pass.to_string()));
    }
    text
}

/// A running build's progress on one line, for the status bar: [`progress_line`] and the time.
pub(crate) fn progress_text(p: &texcraft_engine::Progress, seconds: f64) -> String {
    let mut text = progress_line(p);
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
