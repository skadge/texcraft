//! Making LaTeX and drafting references with AI (docs/ROADMAP.md, AI4): Insert › Make with AI…
//! asks for a description, shows the model's LaTeX typeset (or retex's messages, with Ask Again
//! with the Error) and inserts it as a suggested change at the caret; Insert › Add a Reference
//! with AI… drafts a BibTeX entry from a title or DOI into a `.bib`, as a suggested change, and
//! says plainly that models invent references. Both go through `ai.make`, `ai.insert_made` and
//! `ai.reference`; the first request of each kind asks first ([`crate::ai::request`]).

use std::sync::Arc;

use egui::{Align, Layout, RichText};
use serde_json::{Value, json};
use texcraft_engine::ai::{Made, MakeKind, Preview, Referenced, UNVERIFIED};
use texcraft_engine::project::ProjectPath;

use crate::ai::Kind;
use crate::dialogs::{Dialog, button_row, title};
use crate::theme::Tokens;
use crate::{App, widgets};

/// The kinds offered, with their names and an example description.
const KINDS: [(MakeKind, &str, &str); 4] = [
    (MakeKind::Table, "Table", "a table of the three methods' accuracy and speed"),
    (MakeKind::Equation, "Equation", "the equation for the normal distribution's density"),
    (MakeKind::Tikz, "TikZ Diagram", "a diagram of three boxes joined by arrows, left to right"),
    (MakeKind::Figure, "Figure", "a figure environment for the image plot.png, half the line wide"),
];

/// The most a preview is drawn, in points.
const PREVIEW_WIDTH: f32 = 560.0;
const PREVIEW_HEIGHT: f32 = 320.0;

/// The Make with AI dialog.
pub(crate) struct MakeForm {
    kind: MakeKind,
    description: String,
    path: ProjectPath,
    at: usize,
    focus: bool,
    /// A request went from this dialog: the engine's last thing made is this dialog's.
    sent: bool,
    /// The preview drawn: of which PDF, and its texture (or why it could not be drawn).
    preview: Option<(usize, Result<egui::TextureHandle, String>)>,
}

/// The Add a Reference with AI dialog.
pub(crate) struct ReferenceForm {
    query: String,
    /// The project's `.bib` files.
    bibs: Vec<String>,
    /// The one chosen; `None` for a new one, named in `new_bib`.
    bib: Option<String>,
    new_bib: String,
    focus: bool,
    sent: bool,
}

/// Why LaTeX cannot be made or a reference drafted now, if it cannot.
pub(crate) fn can_make(app: &App) -> Result<(), String> {
    if !crate::ai::offered(app) {
        return Err(tl!("Connect a model first (Settings › AI).").to_owned());
    }
    match app.session.current() {
        Some(p) if app.session.project().text(p).is_ok() => Ok(()),
        _ => Err(tl!("Open a text file first.").to_owned()),
    }
}

/// `edit.ai_make`: opens the dialog for the caret of the file shown.
pub(crate) fn open_make(app: &mut App) {
    if let Err(why) = can_make(app) {
        app.view.status = Some((why, true));
        return;
    }
    crate::editor::send_edits(app);
    let Some(path) = app.session.current().cloned() else { return };
    let at = crate::suggest::to_source(app, &path, app.editor.source.caret());
    let form = MakeForm { kind: MakeKind::Table, description: String::new(), path, at, focus: true, sent: false, preview: None };
    app.view.dialog = Some(Dialog::AiMake(Box::new(form)));
}

/// `edit.ai_reference`: opens the dialog, on the project's first `.bib`.
pub(crate) fn open_reference(app: &mut App) {
    if let Err(why) = can_make(app) {
        app.view.status = Some((why, true));
        return;
    }
    let bibs: Vec<String> = app.session.project().paths().filter(|p| p.extension() == "bib").map(|p| p.as_str().to_owned()).collect();
    let form =
        ReferenceForm { query: String::new(), bib: bibs.first().cloned(), bibs, new_bib: "references.bib".to_owned(), focus: true, sent: false };
    app.view.dialog = Some(Dialog::AiReference(Box::new(form)));
}

/// A request from the open dialog went: what the engine reports from now is the dialog's.
pub(crate) fn started(app: &mut App) {
    match &mut app.view.dialog {
        Some(Dialog::AiMake(f)) => {
            f.sent = true;
            f.preview = None;
        }
        Some(Dialog::AiReference(f)) => f.sent = true,
        _ => {}
    }
}

/// What the Make dialog's buttons asked for.
enum MakeAct {
    None,
    Make { again: bool },
    Insert { anyway: bool },
    Stop,
    Close,
}

/// The Make with AI dialog, while it is open.
pub(crate) fn make_dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let busy = app.session.ai_busy();
    let made = match &app.view.dialog {
        Some(Dialog::AiMake(f)) if f.sent => app.session.ai_made().cloned(),
        Some(Dialog::AiMake(_)) => None,
        _ => return,
    };
    let Some(Dialog::AiMake(form)) = &mut app.view.dialog else { return };
    let mut act = MakeAct::None;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-ai-make")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(PREVIEW_WIDTH + 16.0);
        title(ui, &t, tl!("Make with AI"));
        let running = matches!(made, Some(Made::Running));
        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (kind, name, _) in KINDS {
                    ui.radio_value(&mut form.kind, kind, crate::i18n::t(name));
                }
            });
            ui.add_space(t.unit * 2.0);
            let label = ui.label(RichText::new(tl!("Describe it")).color(t.text_muted));
            let example = KINDS.iter().find(|(k, _, _)| *k == form.kind).map_or("", |(_, _, e)| e);
            let edit =
                egui::TextEdit::multiline(&mut form.description).hint_text(crate::i18n::t(example)).desired_rows(2).desired_width(f32::INFINITY);
            let edit = ui.add(edit).labelled_by(label.id);
            if std::mem::take(&mut form.focus) {
                edit.request_focus();
            }
        });
        ui.add_space(t.unit * 2.0);
        match &made {
            Some(Made::Running) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    let said = ui.label(RichText::new(tl!("The model is writing it; then it is typeset here…")).color(t.text_muted));
                    crate::status::announce(ui, &said, tl!("The model is writing it; then it is typeset here…"));
                });
            }
            Some(Made::Done { latex, packages, model, preview, .. }) => done(ui, &t, form, latex, packages, model, preview),
            Some(Made::Failed(e)) => {
                let said = tl!("Nothing was made: {error}").replace("{error}", e);
                let label = ui.add(egui::Label::new(RichText::new(&said).color(t.error)).wrap());
                crate::status::announce(ui, &label, &said);
            }
            Some(Made::Cancelled) => {
                ui.label(RichText::new(tl!("Stopped: nothing was made.")).color(t.text_muted));
            }
            None => {
                ui.label(
                    RichText::new(tl!("The model's LaTeX is typeset here before you insert it, and goes in as a suggested change at the caret."))
                        .font(t.small())
                        .color(t.text_muted),
                );
            }
        }
        ui.add_space(t.unit * 3.0);
        let ready = !form.description.trim().is_empty() && !busy;
        button_row(ui, |ui| {
            let primary =
                |label: &str| egui::Button::new(RichText::new(label).color(t.on_accent).font(t.strong())).fill(t.accent).stroke(egui::Stroke::NONE);
            match &made {
                Some(Made::Running) => {
                    if ui.add(egui::Button::new(tl!("Stop")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                        act = MakeAct::Stop;
                    }
                }
                Some(Made::Done { preview: Preview::Typeset(_), inserted, .. }) if inserted.is_empty() => {
                    if ui.add(primary(tl!("Insert")).min_size(egui::vec2(88.0, 30.0))).clicked() {
                        act = MakeAct::Insert { anyway: false };
                    }
                    if ui.add_enabled(ready, egui::Button::new(tl!("Make Again")).min_size(egui::vec2(96.0, 30.0))).clicked() {
                        act = MakeAct::Make { again: false };
                    }
                }
                Some(Made::Done { preview: Preview::NotTypeset(_), inserted, .. }) if inserted.is_empty() => {
                    let again = ui.add_enabled(ready, primary(tl!("Ask Again with the Error")).min_size(egui::vec2(160.0, 30.0)));
                    if again.clicked() {
                        act = MakeAct::Make { again: true };
                    }
                    if ui.add(egui::Button::new(tl!("Insert Anyway")).min_size(egui::vec2(110.0, 30.0))).clicked() {
                        act = MakeAct::Insert { anyway: true };
                    }
                }
                _ => {
                    if ui.add_enabled(ready, primary(tl!("Make")).min_size(egui::vec2(88.0, 30.0))).clicked() {
                        act = MakeAct::Make { again: false };
                    }
                }
            }
            if ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                act = MakeAct::Close;
            }
        });
    });
    if modal.should_close() {
        act = MakeAct::Close;
    }
    match act {
        MakeAct::None => {}
        MakeAct::Stop => {
            app.run("ai.cancel", json!({}));
        }
        MakeAct::Close => {
            if matches!(made, Some(Made::Running)) {
                app.run("ai.cancel", json!({}));
            }
            app.view.dialog = None;
        }
        MakeAct::Make { again } => {
            let Some(Dialog::AiMake(f)) = app.view.dialog.take() else { return };
            let target = json!({
                "kind": f.kind.id(), "description": f.description.trim(), "path": f.path.as_str(), "at": f.at, "insert": false, "again": again,
            });
            crate::ai::request(app, Kind::Make, target, Some(Dialog::AiMake(f)));
        }
        MakeAct::Insert { anyway } => {
            if let Some(done) = app.run("ai.insert_made", json!({"anyway": anyway})) {
                app.view.dialog = None;
                let said = match done["note"].as_str() {
                    Some(note) => {
                        format!("{} {note}", tl!("Inserted as a suggested change: accept or reject it beside the text or in the Review panel."))
                    }
                    None => tl!("Inserted as a suggested change: accept or reject it beside the text or in the Review panel.").to_owned(),
                };
                app.view.status = Some((said, false));
            }
        }
    }
}

/// The made LaTeX: its preview or why it does not typeset, the code, the packages it needs.
fn done(ui: &mut egui::Ui, t: &Tokens, form: &mut MakeForm, latex: &str, packages: &[String], model: &str, preview: &Preview) {
    ui.label(RichText::new(tl!("Made by AI ({model})").replace("{model}", model)).font(t.small()).color(t.text_muted));
    match preview {
        Preview::Typeset(pdf) => {
            let key = Arc::as_ptr(pdf) as usize;
            if form.preview.as_ref().is_none_or(|(k, _)| *k != key) {
                form.preview = Some((key, texture(ui.ctx(), pdf)));
            }
            match form.preview.as_ref().map(|(_, t)| t) {
                Some(Ok(texture)) => {
                    let size = texture.size_vec2() / 2.0;
                    let scale = (PREVIEW_WIDTH / size.x).min(PREVIEW_HEIGHT / size.y).min(1.5);
                    egui::Frame::new()
                        .fill(egui::Color32::WHITE)
                        .stroke(t.hairline())
                        .corner_radius(t.corner())
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            let r = ui.add(egui::Image::new(texture).fit_to_exact_size(size * scale));
                            let name = tl!("The LaTeX typeset");
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name));
                        });
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(tl!("It typeset, but the preview could not be drawn: {error}").replace("{error}", e)).color(t.text_muted));
                }
                None => {}
            }
            let said = ui.label(RichText::new(tl!("It typesets.")).font(t.small()).color(t.text));
            crate::status::announce(ui, &said, tl!("It typesets."));
        }
        Preview::NotTypeset(messages) => {
            let said = tl!("It does not typeset: {messages}").replace("{messages}", &messages.join("; "));
            let label = ui.add(egui::Label::new(RichText::new(&said).color(t.error)).wrap().selectable(true));
            crate::status::announce(ui, &label, &said);
        }
    }
    if !packages.is_empty() {
        let said = tl!("It needs {packages}: inserting it adds them before \\begin{document} too, as a second suggested change.")
            .replace("{packages}", &packages.join(", "));
        ui.label(RichText::new(said).font(t.small()).color(t.text));
    }
    ui.add_space(t.unit);
    egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let code = ui.add(egui::Label::new(RichText::new(latex).font(t.code_font()).color(t.text)).wrap().selectable(true));
            let name = tl!("The LaTeX made");
            code.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, name));
        });
    });
}

/// The first page of `pdf`, cropped to its ink with a margin, as a texture.
fn texture(ctx: &egui::Context, pdf: &Arc<Vec<u8>>) -> Result<egui::TextureHandle, String> {
    let doc = texcraft_preview::Document::open(Arc::clone(pdf)).map_err(|e| e.to_string())?;
    let image = doc.render(0, 2.0).map_err(|e| e.to_string())?;
    let (w, h) = (image.width as usize, image.height as usize);
    let ink = |x: usize, y: usize| image.rgba.get((y * w + x) * 4..(y * w + x) * 4 + 3).is_some_and(|p| p.iter().any(|c| *c < 245));
    let rows: Vec<usize> = (0..h).filter(|y| (0..w).any(|x| ink(x, *y))).collect();
    let cols: Vec<usize> = (0..w).filter(|x| rows.iter().any(|y| ink(*x, *y))).collect();
    let (Some(&top), Some(&bottom), Some(&left), Some(&right)) = (rows.first(), rows.last(), cols.first(), cols.last()) else {
        return Err(tl!("the page is blank").to_owned());
    };
    let margin = 12;
    let (x0, y0) = (left.saturating_sub(margin), top.saturating_sub(margin));
    let (x1, y1) = ((right + margin).min(w - 1), (bottom + margin).min(h - 1));
    let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut rgba = Vec::with_capacity(cw * ch * 4);
    for y in y0..=y1 {
        rgba.extend_from_slice(image.rgba.get((y * w + x0) * 4..(y * w + x1 + 1) * 4).unwrap_or_default());
    }
    let color = egui::ColorImage::from_rgba_premultiplied([cw, ch], &rgba);
    Ok(ctx.load_texture("texcraft-ai-make-preview", color, egui::TextureOptions::LINEAR))
}

/// The Add a Reference with AI dialog, while it is open.
pub(crate) fn reference_dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let busy = app.session.ai_busy();
    let drafted = match &app.view.dialog {
        Some(Dialog::AiReference(f)) if f.sent => app.session.ai_referenced().cloned(),
        Some(Dialog::AiReference(_)) => None,
        _ => return,
    };
    let Some(Dialog::AiReference(form)) = &mut app.view.dialog else { return };
    let mut draft = false;
    let mut close = false;
    let mut stop = false;
    let mut another = false;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-ai-reference")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(520.0);
        title(ui, &t, tl!("Add a Reference with AI"));
        egui::Frame::new().fill(t.accent_soft).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(tl!(UNVERIFIED)).color(t.text)).wrap());
        });
        ui.add_space(t.unit * 2.0);
        let running = matches!(drafted, Some(Referenced::Running));
        let done = matches!(drafted, Some(Referenced::Done { .. }));
        ui.add_enabled_ui(!running && !done, |ui| {
            let label = ui.label(RichText::new(tl!("The work's title or DOI")).color(t.text_muted));
            let edit =
                ui.add(egui::TextEdit::singleline(&mut form.query).hint_text("10.1000/xyz123").desired_width(f32::INFINITY)).labelled_by(label.id);
            if std::mem::take(&mut form.focus) {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !form.query.trim().is_empty() {
                draft = true;
            }
            ui.add_space(t.unit * 2.0);
            let label = ui.label(RichText::new(tl!("The bibliography file it goes in")).color(t.text_muted));
            let shown = form.bib.clone().unwrap_or_else(|| tl!("A new file").to_owned());
            let combo = craft_ui::ComboBox::from_id_salt("ai-reference-bib").width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
                for bib in &form.bibs {
                    if ui.selectable_label(form.bib.as_ref() == Some(bib), bib).clicked() {
                        form.bib = Some(bib.clone());
                    }
                }
                if ui.selectable_label(form.bib.is_none(), tl!("A new file")).clicked() {
                    form.bib = None;
                }
            });
            let _ = combo.response.labelled_by(label.id);
            if form.bib.is_none() {
                let label = ui.label(RichText::new(tl!("The new file's path")).color(t.text_muted));
                ui.add(egui::TextEdit::singleline(&mut form.new_bib).desired_width(f32::INFINITY)).labelled_by(label.id);
            }
        });
        ui.add_space(t.unit * 2.0);
        match &drafted {
            Some(Referenced::Running) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(RichText::new(tl!("The model is drafting the entry…")).color(t.text_muted));
                });
            }
            Some(Referenced::Done { entry, bib, created, .. }) => {
                let mut said =
                    tl!("The draft is a suggested change at the end of {bib}: check every field before you accept it.").replace("{bib}", bib);
                if *created {
                    said.push(' ');
                    said.push_str(
                        &tl!("{bib} is new: name it in the document's \\bibliography or \\addbibresource to use it.").replace("{bib}", bib),
                    );
                }
                let label = ui.add(egui::Label::new(RichText::new(&said).color(t.text)).wrap());
                crate::status::announce(ui, &label, &said);
                egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(
                    ui,
                    |ui| {
                        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(entry).font(t.code_font()).color(t.text)).wrap().selectable(true));
                        });
                    },
                );
            }
            Some(Referenced::Failed(e)) => {
                let said = tl!("No reference was drafted: {error}").replace("{error}", e);
                let label = ui.add(egui::Label::new(RichText::new(&said).color(t.error)).wrap());
                crate::status::announce(ui, &label, &said);
            }
            Some(Referenced::Cancelled) | None => {}
        }
        ui.add_space(t.unit * 3.0);
        button_row(ui, |ui| {
            let primary =
                |label: &str| egui::Button::new(RichText::new(label).color(t.on_accent).font(t.strong())).fill(t.accent).stroke(egui::Stroke::NONE);
            if running {
                if ui.add(egui::Button::new(tl!("Stop")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    stop = true;
                }
            } else if done {
                if ui.add(primary(tl!("Done")).min_size(egui::vec2(88.0, 30.0))).clicked() {
                    close = true;
                }
                if ui.add(egui::Button::new(tl!("Draft Another")).min_size(egui::vec2(110.0, 30.0))).clicked() {
                    another = true;
                }
            } else {
                let ready = !form.query.trim().is_empty() && !busy && (form.bib.is_some() || form.new_bib.trim().ends_with(".bib"));
                if ui.add_enabled(ready, primary(tl!("Draft Entry")).min_size(egui::vec2(100.0, 30.0))).clicked() {
                    draft = true;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !done && ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    close = true;
                }
            });
        });
    });
    if modal.should_close() {
        close = true;
    }
    if another && let Some(Dialog::AiReference(f)) = &mut app.view.dialog {
        f.sent = false;
        f.query.clear();
        f.focus = true;
        f.bibs = app.session.project().paths().filter(|p| p.extension() == "bib").map(|p| p.as_str().to_owned()).collect();
    }
    if stop {
        app.run("ai.cancel", json!({}));
    }
    if close {
        if matches!(drafted, Some(Referenced::Running)) {
            app.run("ai.cancel", json!({}));
        }
        app.view.dialog = None;
    } else if draft {
        let Some(Dialog::AiReference(f)) = app.view.dialog.take() else { return };
        let bib = f.bib.clone().unwrap_or_else(|| f.new_bib.trim().to_owned());
        let target: Value = json!({"query": f.query.trim(), "bib": bib});
        crate::ai::request(app, Kind::Reference, target, Some(Dialog::AiReference(f)));
    }
}
