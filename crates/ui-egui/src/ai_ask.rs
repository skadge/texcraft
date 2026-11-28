//! Asking and reviewing with AI (docs/ROADMAP.md, AI5): the side panel's Ask section (its rail
//! button, `view.side.ask`) sends a question with the open file, or the whole project's text
//! files, and shows the answers with the `path:line` places they name as links; Review This
//! Section (`edit.ai_review`, in the panel and the Edit menu) sends the section at the caret, and
//! the remarks arrive as comments. Both go through `ai.ask` and `ai.review`.

use egui::RichText;
use serde_json::json;
use texcraft_engine::ai::{Answered, Link};
use texcraft_engine::project::ProjectPath;

use crate::ai::Kind;
use crate::theme::Tokens;
use crate::{App, widgets};

/// The Ask panel's form.
#[derive(Default)]
pub(crate) struct AskForm {
    question: String,
    /// Ask about the whole project, not only the open file.
    project: bool,
}

/// Why the section at the caret cannot be reviewed now, if it cannot.
pub(crate) fn can_review(app: &App) -> Result<(), String> {
    crate::ai_make::can_make(app, Kind::Review)
}

/// `edit.ai_review`: sends the section at the caret for review.
pub(crate) fn review(app: &mut App) {
    if let Err(why) = can_review(app) {
        app.view.status = Some((why, true));
        return;
    }
    crate::editor::send_edits(app);
    let Some(path) = app.session.current().cloned() else { return };
    let at = crate::suggest::to_source(app, &path, app.editor.source.caret());
    let Ok(text) = app.session.project().text(&path) else { return };
    let (start, end) = texcraft_engine::ai::section_at(text, at);
    crate::ai::request(app, Kind::Review, json!({"path": path.as_str(), "start": start, "end": end}), None);
}

/// Asks the panel's question.
fn ask(app: &mut App) {
    let question = app.ai.ask.question.trim().to_owned();
    let scope = if app.ai.ask.project { "project" } else { "file" };
    let mut target = json!({"question": question, "scope": scope});
    if let Some(path) = app.session.current() {
        target["path"] = json!(path.as_str());
    }
    crate::editor::send_edits(app);
    crate::ai::request(app, Kind::Ask, target, None);
    if app.session.ai_busy() {
        app.ai.ask.question.clear();
    }
}

/// The Ask section of the side panel.
pub(crate) fn panel(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), t.toolbar_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, &t, tl!("Ask AI"));
    });
    let offered = crate::ai::offered(app);
    if !offered {
        ui.add_space(t.unit * 2.0);
        ui.add(egui::Label::new(RichText::new(tl!("Connect a model first (Settings › AI).")).font(t.small()).color(t.text_muted)).wrap());
        if ui.button(tl!("Open Settings")).clicked() {
            crate::ui_cmd::run_with(app, "app.settings", &serde_json::json!({"tab": "ai"}));
        }
        return;
    }
    let busy = app.session.ai_busy();
    let ask_ok = crate::ai::allowed(app, Kind::Ask);
    let review_ok = crate::ai::allowed(app, Kind::Review);
    let file = app.session.current().map(|p| p.as_str().to_owned());
    let mut asked = false;
    let mut goto: Option<(String, u32)> = None;
    let mut review = false;
    let mut cancel = false;
    let form = &mut app.ai.ask;
    let label = ui.label(RichText::new(tl!("Your question")).color(t.text_muted));
    let edit = egui::TextEdit::multiline(&mut form.question)
        .hint_text(tl!("Where do I define the notation for vectors?"))
        .desired_rows(3)
        .desired_width(f32::INFINITY);
    let edit = ui.add(edit).labelled_by(label.id);
    if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command) {
        asked = true;
    }
    let this_file = match &file {
        Some(f) => tl!("This file ({file})").replace("{file}", f),
        None => tl!("This file").to_owned(),
    };
    ui.radio_value(&mut form.project, false, this_file);
    ui.radio_value(&mut form.project, true, tl!("The whole project"));
    ui.horizontal_wrapped(|ui| {
        let ready = !form.question.trim().is_empty() && !busy && (form.project || file.is_some()) && ask_ok.is_ok();
        let r = ui.add_enabled(ready, egui::Button::image_and_text(crate::icons::image(crate::ai::ICON, 14.0, t.text_muted), tl!("Ask")));
        let r = r.on_hover_text(tl!("Ctrl+Enter in the question"));
        let r = match &ask_ok {
            Err(why) => r.on_disabled_hover_text(why),
            Ok(()) => r,
        };
        if r.clicked() {
            asked = true;
        }
        let image = crate::icons::image(crate::ai::ICON, 14.0, t.text_muted);
        let r = ui.add_enabled(!busy && file.is_some() && review_ok.is_ok(), egui::Button::image_and_text(image, tl!("Review This Section")));
        let r = r.on_hover_text(tl!("Send the section at the caret for review; the remarks arrive as comments"));
        let r = match &review_ok {
            Err(why) => r.on_disabled_hover_text(why),
            Ok(()) => r,
        };
        if r.clicked() {
            review = true;
        }
    });
    widgets::hairline(ui, &t);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (i, a) in app.session.ai_answers().iter().enumerate().rev() {
            ui.push_id(i, |ui| answer(ui, &t, a, &mut goto, &mut cancel));
            ui.add_space(t.unit * 2.0);
        }
    });
    if asked {
        ask(app);
    }
    if review {
        crate::ui_cmd::run(app, "edit.ai_review");
    }
    if cancel {
        app.run("ai.cancel", json!({}));
    }
    if let Some((path, line)) = goto
        && let Ok(p) = ProjectPath::new(&path)
        && app.run("file.open", json!({"path": path})).is_some()
    {
        app.view.goto = Some((p, line));
    }
}

/// One question and its answer.
fn answer(ui: &mut egui::Ui, t: &Tokens, a: &Answered, goto: &mut Option<(String, u32)>, cancel: &mut bool) {
    let question = match a {
        Answered::Running { question } | Answered::Done { question, .. } | Answered::Failed { question, .. } | Answered::Cancelled { question } => {
            question
        }
    };
    ui.add(egui::Label::new(RichText::new(question).font(t.strong()).color(t.text)).wrap());
    match a {
        Answered::Running { .. } => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new(tl!("The model is answering…")).font(t.small()).color(t.text_muted));
                if widgets::text_button(ui, t, tl!("Cancel"), true).clicked() {
                    *cancel = true;
                }
            });
        }
        Answered::Done { answer, links, model, .. } => {
            egui::Frame::new().fill(t.accent_soft).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tl!("Answered by AI ({model})").replace("{model}", model)).font(t.small()).color(t.text_muted));
                let shown = linked(ui, t, answer, links, goto);
                crate::status::announce(ui, &shown, answer);
            });
        }
        Answered::Failed { error, .. } => {
            ui.add(
                egui::Label::new(
                    RichText::new(tl!("No answer: {error}").replace("{error}", &crate::i18n::msg(error))).font(t.small()).color(t.error),
                )
                .wrap(),
            );
        }
        Answered::Cancelled { .. } => {
            ui.label(RichText::new(tl!("Cancelled.")).font(t.small()).color(t.text_muted));
        }
    }
}

/// `answer` with its links as links; the response of the whole.
fn linked(ui: &mut egui::Ui, t: &Tokens, answer: &str, links: &[Link], goto: &mut Option<(String, u32)>) -> egui::Response {
    let chars: Vec<char> = answer.chars().collect();
    let part = |a: usize, b: usize| chars.get(a..b).unwrap_or_default().iter().collect::<String>();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut at = 0;
        for l in links {
            if l.start > at {
                ui.add(egui::Label::new(RichText::new(part(at, l.start)).color(t.text)).selectable(true));
            }
            let text = part(l.start, l.end);
            let r = ui.link(&text).on_hover_text(tl!("Go to this line"));
            if r.clicked() {
                *goto = Some((l.path.clone(), l.line));
            }
            at = l.end;
        }
        if at < chars.len() {
            ui.add(egui::Label::new(RichText::new(part(at, chars.len())).color(t.text)).selectable(true));
        }
    })
    .response
}
