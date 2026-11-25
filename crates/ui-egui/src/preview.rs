//! The preview pane (docs/design.md, Regions): the build toolbar and either the PDF or the build
//! log, which takes the PDF's place until "Back to PDF".

use egui::{Align, Layout, RichText, Sense, vec2};
use serde_json::json;
use texcraft_engine::Export;
use texcraft_engine::project::ProjectPath;
use texcraft_engine::{Diagnostic, Severity, Status};

use crate::pdf_view::Fit;
use crate::theme::Tokens;
use crate::{App, icons, top_bar, ui_cmd, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    toolbar(app, ui);
    widgets::hairline(ui, &t);
    let building = app.session.is_building().then(|| {
        let progress = app.view.progress.as_ref();
        crate::pdf_view::Building {
            now: progress.map_or_else(|| tl!("Compiling…").to_owned(), |(p, _)| crate::status::progress_now(p)),
            detail: progress.map(|(p, s)| crate::status::progress_detail(p, *s)).unwrap_or_default(),
            ratio: progress.and_then(|(p, _)| crate::status::progress_ratio(p)),
            // The project's first build of the session: say why it takes longer.
            first: app.session.last_outcome().is_none(),
            draft: app.view.draft.as_ref().map(|d| d.pass),
        }
    });
    // A build over a PDF already shown: a thin bar along the pane's top, full as far as the pass
    // is (or a moving segment when that is not known).
    if let Some(crate::pdf_view::Building { ratio, .. }) = &building {
        let top = ui.available_rect_before_wrap();
        let line = egui::Rect::from_min_size(top.min, vec2(top.width(), 2.0));
        let fill = match ratio {
            Some(r) => egui::Rect::from_min_size(line.min, vec2(line.width() * r, 2.0)),
            None => {
                let phase = (ui.input(|i| i.time) * 0.6).fract() as f32;
                let w = line.width() * 0.25;
                egui::Rect::from_min_size(egui::pos2(line.left() + (line.width() + w) * phase - w, line.top()), vec2(w, 2.0)).intersect(line)
            }
        };
        ui.painter().rect_filled(fill, 0.0, t.accent);
    }
    if app.view.logs {
        logs(app, ui);
    } else {
        // While a build's later passes run, its draft; otherwise the last PDF a build wrote, so a
        // draft never outlives its build (finished, failed or stopped).
        app.pdf.sync(app.view.draft.as_ref().map(|d| &d.pdf).or(app.session.pdf()));
        app.pdf.show(ui, &t, building.as_ref());
        if let Some((page, x, y)) = app.pdf.clicked.take() {
            go_to_source(app, page, x, y);
        }
    }
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0.0, t.frame);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(t.unit * 2.0);
        let building = app.session.is_building();
        let label = if building { tl!("Compiling…") } else { tl!("Compile") };
        let (main, menu) = widgets::primary_split(ui, &t, "play", label, building);
        app.region(crate::Region::Preview, main.id);
        if main.clicked() {
            app.run("build.compile", json!({}));
        }
        let tip = app.keys.shortcut_text("build.compile").map_or_else(|| label.to_owned(), |k| format!("{label} ({k})"));
        let _ = main.on_hover_text(tip);
        egui::Popup::menu(&menu).show(|ui| compile_menu(app, ui));
        ui.add_space(t.unit * 2.0);
        let (errors, warnings) = app.session.last_outcome().map_or((0, 0), |o| (o.errors(), o.warnings()));
        let log_label = if app.view.logs { tl!("Back to PDF") } else { tl!("Build log") };
        let log = widgets::icon_toggle(ui, &t, "scroll-text", log_label, app.view.logs);
        let badge = if errors > 0 { (errors, t.error) } else { (warnings, t.warning) };
        widgets::badge(ui, &t, log.rect.right_top() + vec2(-4.0, 6.0), badge.0, badge.1);
        if log.clicked() {
            app.view.logs = !app.view.logs;
        }
        let ready = cmd_enabled(app, "build.save_pdf").or_else(|_| cmd_enabled(app, "build.export"));
        let save = widgets::icon_button(ui, &t, "download", tl!("Export…"), ready.is_ok());
        if let Err(reason) = &ready {
            let _ = save.on_disabled_hover_text(reason.as_str());
        } else {
            egui::Popup::menu(&save).show(|ui| export_menu(app, ui));
        }
        // What the build does now, dim, in the room the buttons leave: beside the PDF it builds.
        // The status bar announces it, so it is not announced here too.
        let progress = building.then(|| app.view.progress.as_ref().map(|(p, _)| crate::status::progress_line(p))).flatten();
        // Without a PDF yet, the pane's middle says it (`pdf_view::Building`).
        let progress = progress.filter(|_| app.view.logs || app.pdf.has_pdf());
        let show_progress = |ui: &mut egui::Ui| {
            if let Some(text) = &progress
                && ui.available_width() > 4.0 * t.unit
            {
                ui.add_space(t.unit * 2.0);
                ui.add(egui::Label::new(RichText::new(text).font(t.small()).color(t.text_muted)).truncate());
            }
        };
        if app.view.logs || !app.pdf.has_pdf() {
            show_progress(ui);
            return;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(t.unit * 2.0);
            if widgets::icon_button(ui, &t, "zoom-in", tl!("Zoom In"), true).clicked() {
                app.pdf.zoom_by(1.25);
            }
            zoom_menu(app, ui);
            if widgets::icon_button(ui, &t, "zoom-out", tl!("Zoom Out"), true).clicked() {
                app.pdf.zoom_by(0.8);
            }
            ui.add_space(t.unit * 2.0);
            let (page, count) = (app.pdf.page, app.pdf.page_count());
            if widgets::icon_button(ui, &t, "chevron-right", tl!("Next page"), page + 1 < count).clicked() {
                app.pdf.go_to_page(page + 1);
            }
            ui.label(RichText::new(format!("{} / {count}", page + 1)).font(t.small()).color(t.text_muted));
            if widgets::icon_button(ui, &t, "chevron-left", tl!("Previous page"), page > 0).clicked() {
                app.pdf.go_to_page(page.saturating_sub(1));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), show_progress);
        });
    });
}

fn cmd_enabled(app: &App, id: &str) -> Result<(), String> {
    texcraft_engine::cmd::find(id).map_or_else(|| Err(String::new()), |c| (c.enabled)(&app.session))
}

fn compile_menu(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.set_min_width(260.0);
    let mut auto = app.session.settings.auto_compile;
    if ui.checkbox(&mut auto, tl!("Compile automatically")).changed() {
        app.run("build.auto", json!({"on": auto}));
    }
    ui.separator();
    let options = app.session.settings.build;
    let mut draft = options.draft_images;
    if ui.checkbox(&mut draft, tl!("Draft images (framed boxes)")).changed() {
        app.run("build.options", json!({"draft_images": draft}));
    }
    let mut halt = options.halt_on_error;
    if ui.checkbox(&mut halt, tl!("Stop at the first error")).changed() {
        app.run("build.options", json!({"halt_on_error": halt}));
    }
    ui.separator();
    if top_bar::item(ui, &t, Some("eraser"), tl!("Delete Build Files"), None, cmd_enabled(app, "build.clean"))
        && app.run("build.clean", json!({})).is_some()
    {
        app.view.status = Some((tl!("Build files deleted: the next build starts from scratch.").to_owned(), false));
    }
    let stop = cmd_enabled(app, "build.stop");
    if top_bar::item(ui, &t, Some("x"), tl!("Stop Compiling"), None, stop) {
        app.run("build.stop", json!({}));
    }
}

/// The formats the Export menu lists, in its order: PDF, then the presentation formats first for
/// a beamer document and the text formats first otherwise, then HTML, Markdown and SVG.
pub(crate) fn export_order(presentation: bool) -> [Export; 8] {
    use Export::{Docx, Html, Markdown, Odp, Odt, Pdf, Pptx, Svg};
    if presentation { [Pdf, Pptx, Odp, Docx, Odt, Html, Markdown, Svg] } else { [Pdf, Docx, Odt, Pptx, Odp, Html, Markdown, Svg] }
}

/// A format's name in the Export menu, translated.
pub(crate) fn export_label(format: Export) -> &'static str {
    match format {
        Export::Pdf => tl!("PDF"),
        Export::Odt => tl!("OpenDocument Text (.odt)"),
        Export::Docx => tl!("Word Document (.docx)"),
        Export::Odp => tl!("OpenDocument Presentation (.odp)"),
        Export::Pptx => tl!("PowerPoint Presentation (.pptx)"),
        Export::Html => tl!("Web Page (.html)"),
        Export::Markdown => tl!("Markdown (.md)"),
        Export::Svg => tl!("SVG Image (.svg)"),
    }
}

fn export_menu(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.set_min_width(260.0);
    let convert = cmd_enabled(app, "build.export")
        .and_then(|()| if app.session.can_convert() { Ok(()) } else { Err(tl!("This typesetting engine exports only the PDF.").to_owned()) });
    for (i, format) in export_order(app.session.is_presentation()).into_iter().enumerate() {
        if i == 1 || i == 5 {
            ui.separator();
        }
        let enabled = if format == Export::Pdf { cmd_enabled(app, "build.save_pdf") } else { convert.clone() };
        if top_bar::item(ui, &t, None, export_label(format), None, enabled) {
            ui.close();
            ui_cmd::export(app, format);
        }
    }
}

fn zoom_menu(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let label = match app.pdf.fit() {
        Fit::Width => tl!("Fit width").to_owned(),
        Fit::Page => tl!("Fit page").to_owned(),
        Fit::Zoom(_) => format!("{:.0} %", app.pdf.zoom() * 100.0),
    };
    let r = widgets::text_button(ui, &t, &label, true);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(140.0);
        let mut choose = |ui: &mut egui::Ui, text: &str, fit: Fit| {
            if ui.add(egui::Button::selectable(app.pdf.fit() == fit, text)).clicked() {
                app.pdf.set_fit(fit);
                ui.close();
            }
        };
        choose(ui, tl!("Fit width"), Fit::Width);
        choose(ui, tl!("Fit page"), Fit::Page);
        ui.separator();
        for z in [50, 75, 100, 125, 150, 200, 300, 400] {
            choose(ui, &format!("{z} %"), Fit::Zoom(z as f32 / 100.0));
        }
    });
}

/// Which diagnostics the log lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Filter {
    #[default]
    All,
    Errors,
    Warnings,
    Notes,
}

impl Filter {
    fn keeps(self, s: Severity) -> bool {
        match self {
            Filter::All => true,
            Filter::Errors => s == Severity::Error,
            Filter::Warnings => s == Severity::Warning,
            Filter::Notes => s <= Severity::Note,
        }
    }
}

fn logs(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0.0, t.canvas);
    let Some(outcome) = app.session.last_outcome().cloned() else {
        ui.add_space(t.unit * 6.0);
        ui.vertical_centered(|ui| ui.label(RichText::new(tl!("Nothing has been compiled yet.")).color(t.text_muted)));
        return;
    };
    let count = |f: Filter| outcome.diagnostics.iter().filter(|d| f.keeps(d.severity)).count();
    let filters = [Filter::All, Filter::Errors, Filter::Warnings, Filter::Notes];
    let labels: Vec<String> =
        filters.iter().zip([tl!("All"), tl!("Errors"), tl!("Warnings"), tl!("Notes")]).map(|(f, name)| format!("{name} {}", count(*f))).collect();
    let mut goto = None;
    // Text to put on the clipboard, and what the status line says then.
    let mut copied: Option<(String, &str)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(t.unit * 2.0, t.unit * 2.0);
            let shown: Vec<&Diagnostic> = outcome.diagnostics.iter().filter(|d| app.view.log_filter.keeps(d.severity)).collect();
            ui.horizontal(|ui| {
                let options: Vec<(&str, Option<&str>)> = labels.iter().map(|l| (l.as_str(), None)).collect();
                let selected = filters.iter().position(|f| *f == app.view.log_filter).unwrap_or(0);
                if let Some(i) = widgets::segmented(ui, &t, "log-filter", &options, selected) {
                    app.view.log_filter = filters.get(i).copied().unwrap_or_default();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button(ui, &t, "copy", tl!("Copy the messages shown"), !shown.is_empty()).clicked() {
                        let text: Vec<String> = shown.iter().copied().map(diagnostic_text).collect();
                        copied = Some((text.join("\n"), tl!("The messages are copied.")));
                    }
                });
            });
            if let Status::Unavailable(reason) = &outcome.status {
                let c = Card { icon: "circle-exclamation-point", color: t.error, title: tl!("Not compiled"), message: reason, help: "" };
                if let CardAction::Copy = card(ui, &t, c, None) {
                    copied = Some((reason.clone(), tl!("The message is copied.")));
                }
            }
            if shown.is_empty() && !matches!(outcome.status, Status::Unavailable(_)) {
                let message = tl!("The last build reported no messages of this kind.");
                card(ui, &t, Card { icon: "circle-check", color: t.success, title: tl!("Nothing to report"), message, help: "" }, None);
            }
            for d in shown.iter().copied() {
                let (icon, color, word) = match d.severity {
                    Severity::Error => ("circle-exclamation-point", t.error, tl!("Error")),
                    Severity::Warning => ("triangle-exclamation-point", t.warning, tl!("Warning")),
                    Severity::Note => ("info", t.note, tl!("Note")),
                    Severity::Hint => ("info", t.text_muted, tl!("Hint")),
                };
                let place = d.file.as_ref().map(|f| (f.clone(), d.line));
                let help = d.help.join("\n");
                let title = match &d.origin {
                    Some(o) => format!("{word} · {o}"),
                    None => word.to_owned(),
                };
                match card(ui, &t, Card { icon, color, title: &title, message: &d.message, help: &help }, place.as_ref()) {
                    CardAction::Open(target) => goto = Some(target),
                    CardAction::Copy => copied = Some((diagnostic_text(d), tl!("The message is copied."))),
                    CardAction::None => {}
                }
            }
            egui::CollapsingHeader::new(RichText::new(tl!("Raw log")).font(t.strong())).show(ui, |ui| {
                let has_log = !outcome.log.is_empty();
                if widgets::text_button(ui, &t, tl!("Copy the Raw Log"), has_log).clicked() {
                    copied = Some((outcome.log.clone(), tl!("The raw log is copied.")));
                }
                egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner_large()).inner_margin(egui::Margin::same(10)).show(
                    ui,
                    |ui| {
                        ui.add(egui::Label::new(RichText::new(outcome.log.as_str()).monospace().size(t.small_size)).wrap().selectable(true));
                    },
                );
            });
        });
    });
    if let Some((text, said)) = copied {
        ui.ctx().copy_text(text);
        app.view.status = Some((said.to_owned(), false));
    }
    if let Some((file, line)) = goto
        && let Ok(path) = ProjectPath::new(&file)
    {
        app.run("file.open", json!({"path": path.as_str()}));
        if let Some(line) = line {
            app.view.goto = Some((path, line));
        }
    }
}

/// What a log card shows.
struct Card<'a> {
    icon: &'a str,
    color: egui::Color32,
    title: &'a str,
    message: &'a str,
    /// How to fix it, muted under the message; may be empty.
    help: &'a str,
}

/// What was done with a log card.
enum CardAction {
    None,
    /// Its place was clicked: open it in the editor.
    Open((String, Option<u32>)),
    /// Its copy button was clicked.
    Copy,
}

/// One message as a card: its text can be selected, and its button copies it.
fn card(ui: &mut egui::Ui, t: &Tokens, c: Card<'_>, place: Option<&(String, Option<u32>)>) -> CardAction {
    let Card { icon, color, title, message, help } = c;
    let mut action = CardAction::None;
    egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner_large()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.add(icons::image(icon, t.icon_size, color));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = t.unit;
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(title).font(t.strong()).color(color)).selectable(true));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::icon_button(ui, t, "copy", tl!("Copy the message"), true).clicked() {
                            action = CardAction::Copy;
                        }
                        if let Some((file, line)) = place {
                            let text = match line {
                                Some(l) => format!("{file}:{l}"),
                                None => file.clone(),
                            };
                            let link = ui.add(egui::Label::new(RichText::new(text).font(t.small()).color(t.accent)).sense(Sense::click()));
                            let link = link.on_hover_text(tl!("Open in the editor"));
                            if link.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if link.clicked() {
                                action = CardAction::Open((file.clone(), *line));
                            }
                        }
                    });
                });
                ui.add(egui::Label::new(RichText::new(message).color(t.text)).selectable(true));
                if !help.is_empty() {
                    ui.add(egui::Label::new(RichText::new(help).font(t.small()).color(t.text_muted)).selectable(true));
                }
            });
        });
    });
    action
}

/// A diagnostic as one copies it, as compilers print them: `file:line: severity: message`, then
/// its help indented. The words are not translated, so tools and searches can read them.
fn diagnostic_text(d: &Diagnostic) -> String {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
        Severity::Hint => "hint",
    };
    let place = match (&d.file, d.line) {
        (Some(f), Some(l)) => format!("{f}:{l}: "),
        (Some(f), None) => format!("{f}: "),
        (None, _) => String::new(),
    };
    let origin = d.origin.as_ref().map(|o| format!("{o}: ")).unwrap_or_default();
    let mut text = format!("{place}{severity}: {origin}{}", d.message);
    for h in &d.help {
        text.push_str("\n    ");
        text.push_str(h);
    }
    text
}

/// A point of the PDF double-clicked or Ctrl+clicked: its source line, shown in the editor.
fn go_to_source(app: &mut App, page: usize, x: f32, y: f32) {
    match app.session.source_at(page as u32, x, y) {
        Some((path, line)) => {
            if app.session.current() != Some(&path) {
                let _ = app.run("file.open", json!({"path": path.as_str()}));
            }
            app.view.goto = Some((path, line));
            app.editor.focus = true;
        }
        None => app.view.status = Some((tl!("No source line is known for that place; compile the project first.").to_owned(), false)),
    }
}

/// Shows the place of `line` (from 1) of `path` in the PDF, marked for a moment.
pub(crate) fn show_in_pdf(app: &mut App, path: &texcraft_engine::project::ProjectPath, line: u32) {
    let rects = app.session.locate(path, line);
    if rects.is_empty() {
        app.view.status = Some((tl!("This line is not in the PDF yet; compile the project first.").to_owned(), false));
        return;
    }
    if app.view.layout == crate::Layout::EditorOnly {
        app.view.layout = crate::Layout::Split;
    }
    app.view.logs = false;
    let now = app.ctx.input(|i| i.time);
    app.pdf.show_place(rects, now);
}
