//! The preview pane (docs/design.md, Regions): the build toolbar and either the PDF or the build
//! log, which takes the PDF's place until "Back to PDF".

use egui::{Align, Layout, RichText, Sense, vec2};
use serde_json::json;
use texcraft_engine::Export;
use texcraft_engine::project::ProjectPath;
use texcraft_engine::{Diagnostic, Outcome, Output, PageRect, Severity, Status};

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
        sync(app);
        app.pdf.show(ui, &t, building.as_ref());
        if let Some((page, x, y)) = app.pdf.clicked.take() {
            go_to_source(app, page, x, y);
        }
        if let Some(text) = app.pdf.copied.take() {
            let message = if text.trim().is_empty() {
                tl!("The selected part of the PDF has no text to copy: its font does not say which characters it draws.").to_owned()
            } else {
                tl!("Copied {n} characters from the PDF.").replace("{n}", &text.chars().count().to_string())
            };
            app.view.status = Some((message, false));
        }
        match app.pdf.followed.take() {
            Some(crate::pdf_view::Followed::Opened(uri)) => {
                app.view.status = Some((tl!("Opening {address} in your browser.").replace("{address}", uri.trim()), false));
            }
            Some(crate::pdf_view::Followed::Refused(uri)) => {
                let text = tl!("Not opened: {address} is not a web or mail address. Links from the PDF open only those.");
                app.view.status = Some((text.replace("{address}", uri.trim()), true));
            }
            None => {}
        }
    }
}

/// The PDF's own window, while the PDF is out of the main one.
pub(crate) const WINDOW: &str = "texcraft-pdf";

/// The PDF pane in its own window (`view.pdf_window`): an immediate viewport on the desktop, a
/// floating window inside the main one where the shell cannot open another (tests). The window's
/// keys run the same shortcuts; closing it puts the PDF back in the main window.
pub(crate) fn window(app: &mut App, ctx: &egui::Context) {
    let title = tl!("{project}: PDF").replace("{project}", &app.project_name());
    let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size([720.0, 900.0]).with_min_inner_size([320.0, 240.0]);
    let closed = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(WINDOW), builder, |ui, _class| {
        for id in app.keys.dispatch(ui.ctx()) {
            ui_cmd::run(app, &id);
        }
        let frame = egui::Frame::new().fill(app.tokens.frame);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| show(app, ui));
        ui.input(|i| i.viewport().close_requested())
    });
    if closed && app.view.pdf_window {
        ui_cmd::run(app, "view.pdf_window");
    }
}

/// Shows the PDF to show: the running build's draft, or the last build's.
pub(crate) fn sync(app: &mut App) {
    app.pdf.sync(app.view.draft.as_ref().map(|d| &d.pdf).or(app.session.pdf()));
}

/// Presentation mode: the page presented takes the whole window. The arrows, Space, Enter,
/// Page Up and Page Down, Home and End move through the pages; Escape leaves.
pub(crate) fn present(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    sync(app);
    let keys = ui.input_mut(|i| {
        let mut pressed = |keys: &[egui::Key]| keys.iter().filter(|k| i.consume_key(egui::Modifiers::NONE, **k)).count() > 0;
        let next = pressed(&[egui::Key::ArrowRight, egui::Key::ArrowDown, egui::Key::Space, egui::Key::PageDown, egui::Key::Enter]);
        let previous = pressed(&[egui::Key::ArrowLeft, egui::Key::ArrowUp, egui::Key::PageUp, egui::Key::Backspace]);
        let first = pressed(&[egui::Key::Home]);
        let last = pressed(&[egui::Key::End]);
        let end = pressed(&[egui::Key::Escape]);
        (next, previous, first, last, end)
    });
    match keys {
        (true, ..) => ui_cmd::run(app, "view.present.next"),
        (_, true, ..) => ui_cmd::run(app, "view.present.previous"),
        (_, _, true, ..) => app.pdf.present_step(-(app.pdf.page_count() as isize)),
        (_, _, _, true, _) => app.pdf.present_step(app.pdf.page_count() as isize),
        _ => {}
    }
    if keys.4 {
        ui_cmd::run(app, "view.present.end");
        return;
    }
    app.pdf.show_slide(ui, &t);
    // Where it is, and how to leave, said once the page changes; shown for a moment at the start.
    let Some(page) = app.pdf.presenting() else { return };
    let text = tl!("Page {n} of {count}. Escape leaves the presentation.")
        .replace("{n}", &(page + 1).to_string())
        .replace("{count}", &app.pdf.page_count().to_string());
    let now = ui.input(|i| i.time);
    let shown = now - app.pdf.presented_since(now) < 3.0;
    let strip = egui::Rect::from_min_max(egui::pos2(ui.max_rect().left(), ui.max_rect().bottom() - t.unit * 14.0), ui.max_rect().max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::bottom_up(egui::Align::Center)));
    child.add_space(t.unit * 3.0);
    let chip = egui::Frame::new()
        .fill(if shown { t.raised } else { egui::Color32::TRANSPARENT })
        .corner_radius(t.corner())
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(&mut child, |ui| {
            let color = if shown { t.text } else { egui::Color32::TRANSPARENT };
            ui.add(egui::Label::new(RichText::new(&text).font(t.small()).color(color)).sense(Sense::hover()))
        })
        .inner;
    crate::status::announce(ui, &chip, &text);
    if shown {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    // Whether the pages and the zoom go on a second row: in a narrow window, or when the buttons
    // before them leave no room (longer languages).
    let second_row = ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0.0, t.frame);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(t.unit * 2.0);
        let building = app.session.is_building();
        let auto = app.session.settings.auto_compile;
        // Compiling automatically, the pill says so; clicking it still compiles now.
        let (label, name, tip) = match (building, auto) {
            (true, _) => (tl!("Compiling…"), tl!("Compiling…"), tl!("Compiling…")),
            (false, true) => (tl!("Auto-Compile"), tl!("Auto-Compile: compile now"), tl!("Compiles as you type; click to compile now")),
            (false, false) => (tl!("Compile"), tl!("Compile"), tl!("Compile")),
        };
        let (main, menu) = widgets::primary_split(ui, &t, "play", label, name, building);
        app.region(crate::Region::Preview, main.id);
        if main.clicked() {
            app.run("build.compile", json!({}));
        }
        let tip = app.keys.shortcut_text("build.compile").map_or_else(|| tip.to_owned(), |k| format!("{tip} ({k})"));
        let _ = main.on_hover_text(tip);
        crate::widgets::keyboard_menu(ui, &menu);
        egui::Popup::menu(&menu).show(|ui| compile_menu(app, ui));
        ui.add_space(t.unit * 2.0);
        let (errors, warnings) = app.session.last_outcome().map_or((0, 0), |o| (o.errors(), app.session.warnings_shown()));
        // The count the badge draws is said in the button's name too.
        let log_label = match (app.view.logs, errors, warnings) {
            (true, _, _) => tl!("Back to PDF").to_owned(),
            (false, 0, 0) => tl!("Build log").to_owned(),
            (false, 0, n) => crate::i18n::trn(n, "Build log, {n} warning", "Build log, {n} warnings"),
            (false, n, _) => crate::i18n::trn(n, "Build log, {n} error", "Build log, {n} errors"),
        };
        let log = widgets::icon_toggle(ui, &t, "scroll-text", &log_label, app.view.logs);
        let badge = if errors > 0 { (errors, t.error) } else { (warnings, t.warning) };
        widgets::badge(ui, &t, log.rect.right_top() + vec2(-4.0, 6.0), badge.0, badge.1);
        if log.clicked() {
            app.view.logs = !app.view.logs;
        }
        // The browser cannot open a second window of the app: no button there (the command
        // says why).
        if ui_cmd::enabled(app, "view.pdf_window").is_ok() {
            let label = if app.view.pdf_window { tl!("Put the PDF back in the main window") } else { tl!("Open the PDF in its own window") };
            if widgets::icon_toggle(ui, &t, "window-out", label, app.view.pdf_window).clicked() {
                ui_cmd::run(app, "view.pdf_window");
            }
        }
        ui.add_space(t.unit * 2.0);
        // The PDF a click away: downloaded in the browser, saved where the user chooses elsewhere.
        let save_pdf = cmd_enabled(app, "build.save_pdf");
        let save_label = ui_cmd::menu_entry(app, "build.save_pdf").0;
        let save = widgets::icon_button(ui, &t, "download", save_label, save_pdf.is_ok());
        let save = match &save_pdf {
            Err(reason) => save.on_hover_text(crate::i18n::msg(reason)),
            Ok(()) => save.on_hover_text(save_label),
        };
        if save.clicked() {
            ui_cmd::run(app, "build.save_pdf");
        }
        let ready = save_pdf.or_else(|_| cmd_enabled(app, "build.export"));
        // While a file is exported, the pill turns and says so; the status line says when it is done.
        let exporting = app.session.exporting();
        let label = if exporting { tl!("Exporting…") } else { tl!("Export") };
        let save = widgets::pill_menu(ui, &t, "upload", label, exporting, ready.is_ok());
        match &ready {
            Err(reason) => {
                let _ = save.on_hover_text(reason.as_str());
            }
            Ok(()) if exporting => {}
            Ok(()) => {
                let save = save.on_hover_text(tl!("Export the document as PDF or another format"));
                crate::widgets::keyboard_menu(ui, &save);
                egui::Popup::menu(&save).show(|ui| export_menu(app, ui));
            }
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
        let fits = widgets::fits_before_right(ui, "pdf-tools");
        if app.view.logs || !app.pdf.has_pdf() || app.view.narrow || !fits {
            show_progress(ui);
            return !fits;
        }
        // The pages and the zoom at the bar's end, made in the order they read.
        let room = widgets::room_before_right(ui, "pdf-tools");
        ui.allocate_ui_with_layout(vec2(room, ui.available_height()), Layout::left_to_right(Align::Center), |ui| {
            ui.set_width(room);
            show_progress(ui);
        });
        widgets::right_aligned(ui, "pdf-tools", |ui| page_tools(app, ui));
        false
    });
    // A narrow window has no room for them on the bar: they are a second row.
    if (app.view.narrow || second_row.inner) && !app.view.logs && app.pdf.has_pdf() {
        ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, t.frame);
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(t.unit * 2.0);
            page_tools(app, ui);
        });
    }
}

/// The pages and the zoom of the PDF's toolbar.
fn page_tools(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let (page, count) = (app.pdf.page, app.pdf.page_count());
    if widgets::icon_button(ui, &t, "chevron-left", tl!("Previous page"), page > 0).clicked() {
        app.pdf.go_to_page(page.saturating_sub(1));
    }
    ui.label(RichText::new(format!("{} / {count}", page + 1)).font(t.small()).color(t.text_muted));
    if widgets::icon_button(ui, &t, "chevron-right", tl!("Next page"), page + 1 < count).clicked() {
        app.pdf.go_to_page(page + 1);
    }
    ui.add_space(t.unit * 2.0);
    if widgets::icon_button(ui, &t, "zoom-out", tl!("Zoom Out"), true).clicked() {
        ui_cmd::run(app, "view.zoom_out");
    }
    zoom_menu(app, ui);
    if widgets::icon_button(ui, &t, "zoom-in", tl!("Zoom In"), true).clicked() {
        ui_cmd::run(app, "view.zoom_in");
    }
    // The margin at the bar's end, counted in the controls' width.
    ui.allocate_space(vec2(t.unit * 2.0 - ui.spacing().item_spacing.x, 1.0));
}

fn cmd_enabled(app: &App, id: &str) -> Result<(), String> {
    ui_cmd::command_enabled(app, id)
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
    // The kind of PDF; Settings › Building has the print options.
    let kinds = [
        (Output::Screen, tl!("PDF for the screen")),
        (Output::Accessible, tl!("Accessible PDF (PDF/UA)")),
        (Output::Print, tl!("PDF for print (PDF/X)")),
    ];
    for (kind, label) in kinds {
        if ui.radio(options.output == kind, label).clicked() && options.output != kind {
            app.run("build.options", json!({"output": kind}));
        }
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
        Export::Docx => tl!("DOCX Document (.docx)"),
        Export::Odp => tl!("OpenDocument Presentation (.odp)"),
        Export::Pptx => tl!("PPTX Presentation (.pptx)"),
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
        let (enabled, label) = if format == Export::Pdf {
            (cmd_enabled(app, "build.save_pdf"), ui_cmd::menu_entry(app, "build.save_pdf").0)
        } else {
            (convert.clone(), export_label(format))
        };
        if top_bar::item(ui, &t, None, label, None, enabled) {
            ui.close();
            ui_cmd::export(app, format);
        }
    }
}

/// The zoom menu: the zoom now, and the fits and fixed zooms, each a command.
fn zoom_menu(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let label = match app.pdf.fit() {
        Fit::Width => tl!("Fit width").to_owned(),
        Fit::Page => tl!("Fit page").to_owned(),
        Fit::Zoom(_) => format!("{:.0} %", app.pdf.zoom() * 100.0),
    };
    let r = widgets::text_button(ui, &t, &label, true);
    let _ = r.clone().on_hover_text(tl!("Zoom"));
    crate::widgets::keyboard_menu(ui, &r);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(140.0);
        let mut choose = |ui: &mut egui::Ui, text: &str, fit: Fit, id: &str, params: serde_json::Value| {
            let mut button = egui::Button::selectable(app.pdf.fit() == fit, text);
            if let Some(k) = app.keys.shortcut_text(id).filter(|_| params.is_null()) {
                button = button.shortcut_text(k);
            }
            if ui.add(button).clicked() {
                ui_cmd::run_with(app, id, &params);
                ui.close();
            }
        };
        choose(ui, tl!("Fit width"), Fit::Width, "view.fit_width", serde_json::Value::Null);
        choose(ui, tl!("Fit page"), Fit::Page, "view.fit_page", serde_json::Value::Null);
        ui.separator();
        for z in [50, 75, 100, 125, 150, 200, 300, 400] {
            let text = format!("{z} %");
            if z == 100 {
                choose(ui, &text, Fit::Zoom(1.0), "view.actual_size", serde_json::Value::Null);
            } else {
                choose(ui, &text, Fit::Zoom(z as f32 / 100.0), "view.zoom", json!({"percent": z}));
            }
        }
        ui.separator();
        let mut present = egui::Button::new(tl!("Present"));
        if let Some(k) = app.keys.shortcut_text("view.present") {
            present = present.shortcut_text(k);
        }
        if ui.add(present).on_hover_text(tl!("Show the PDF a page at a time, in the whole window.")).clicked() {
            ui.close();
            ui_cmd::run(app, "view.present");
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
    /// The accessibility checks' findings.
    Accessibility,
    /// The print checks and the PDF/X's messages.
    Print,
}

impl Filter {
    fn keeps(self, d: &Diagnostic) -> bool {
        let s = d.severity;
        match self {
            Filter::All => true,
            Filter::Errors => s == Severity::Error,
            Filter::Warnings => s == Severity::Warning,
            Filter::Notes => s <= Severity::Note,
            Filter::Accessibility => d.is_accessibility(),
            Filter::Print => d.is_print(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Filter::All => tl!("All"),
            Filter::Errors => tl!("Errors"),
            Filter::Warnings => tl!("Warnings"),
            Filter::Notes => tl!("Notes"),
            Filter::Accessibility => tl!("Accessibility"),
            Filter::Print => crate::i18n::tc("messages", "Print"),
        }
    }
}

/// The filters the log offers for `outcome`: by severity, then Accessibility and Print for a
/// build of that kind or one with such findings.
pub(crate) fn filters(outcome: &Outcome) -> Vec<Filter> {
    let mut filters = vec![Filter::All, Filter::Errors, Filter::Warnings, Filter::Notes];
    if outcome.output == Output::Accessible || outcome.accessibility().next().is_some() {
        filters.push(Filter::Accessibility);
    }
    if outcome.preflight.is_some() || outcome.diagnostics.iter().any(Diagnostic::is_print) {
        filters.push(Filter::Print);
    }
    filters
}

/// A print build's summary, at the top of the log: ready for print, or how many problems.
fn print_summary(ui: &mut egui::Ui, t: &Tokens, outcome: &Outcome) -> CardAction {
    let (Some(preflight), Some(problems)) = (&outcome.preflight, outcome.print_problems()) else { return CardAction::None };
    let standard = preflight.standard.name();
    let (icon, color, title, message) = if problems == 0 {
        let message = tl!("The {standard} meets its standard's rules, and the print checks found nothing to fix.").replace("{standard}", standard);
        ("circle-check", t.success, tl!("Ready for print").to_owned(), message)
    } else {
        let title = crate::i18n::trn(problems, "{n} problem", "{n} problems");
        let message = if preflight.pdfx.is_none() {
            tl!("No {standard} was written: see the Print messages.").replace("{standard}", standard)
        } else {
            tl!("Fix them before sending the {standard} to the printer: the Print filter lists them.").replace("{standard}", standard)
        };
        ("triangle-exclamation-point", t.warning, title, message)
    };
    card(ui, t, Card { icon, color, title: &title, message: &message, help: "", ai: None, noticed: None, hide: None }, None, None)
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
    // Messages of a kind the user hid are left out, unless asked for (to show them again).
    let hidden_count = app.session.hidden_count();
    let with_hidden = app.view.log_hidden && hidden_count > 0;
    let listed = |d: &Diagnostic| with_hidden || !app.session.is_hidden(d);
    let count = |f: Filter| outcome.diagnostics.iter().filter(|d| f.keeps(d) && listed(d)).count();
    let filters = filters(&outcome);
    if !filters.contains(&app.view.log_filter) {
        app.view.log_filter = Filter::All;
    }
    let labels: Vec<String> = filters.iter().map(|f| format!("{} {}", f.label(), count(*f))).collect();
    let mut goto = None;
    // A print check's area, to show on its page.
    let mut on_page = None;
    // Text to put on the clipboard, and what the status line says then.
    let mut copied: Option<(String, &str)> = None;
    // A message to explain (its index in the build's messages), or the explanation to cancel.
    let mut explain = None;
    let mut cancel_ai = false;
    let ai_offered = crate::ai::offered(app);
    let ai_blocked = crate::ai::explain_blocked(app);
    let outputs = app.session.outputs();
    let browser = app.session.library().is_some();
    let file_manager = ui_cmd::enabled(app, "view.show_output").is_ok();
    let mut output_action = None;
    // Hide the messages of this one's kind (its index in the build's messages), or show them again.
    let mut hide_kind: Option<(usize, &str)> = None;
    let mut show_again: Option<String> = None;
    let mut toggle_hidden = false;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(t.unit * 2.0, t.unit * 2.0);
            let shown: Vec<&Diagnostic> = outcome.diagnostics.iter().filter(|d| app.view.log_filter.keeps(d) && listed(d)).collect();
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
            if hidden_count > 0 {
                ui.horizontal(|ui| {
                    let said = crate::i18n::trn(hidden_count, "{n} hidden message", "{n} hidden messages");
                    ui.label(RichText::new(&said).font(t.small()).color(t.text_muted));
                    let label = if with_hidden { tl!("Hide Them") } else { tl!("Show") };
                    let r = widgets::text_button(ui, &t, label, true);
                    let r = r.on_hover_text(tl!("Messages of the kinds hidden in Settings › Building"));
                    if r.clicked() {
                        toggle_hidden = true;
                    }
                });
            }
            if let CardAction::Copy = print_summary(ui, &t, &outcome) {
                copied = Some((print_summary_text(&outcome), tl!("The message is copied.")));
            }
            if let Status::Unavailable(reason) = &outcome.status {
                let c = Card {
                    icon: "circle-exclamation-point",
                    color: t.error,
                    title: tl!("Not compiled"),
                    message: reason,
                    help: "",
                    ai: None,
                    noticed: None,
                    hide: None,
                };
                if let CardAction::Copy = card(ui, &t, c, None, None) {
                    copied = Some((reason.clone(), tl!("The message is copied.")));
                }
            }
            if shown.is_empty() && !matches!(outcome.status, Status::Unavailable(_)) {
                let message = tl!("The last build reported no messages of this kind.");
                card(
                    ui,
                    &t,
                    Card {
                        icon: "circle-check",
                        color: t.success,
                        title: tl!("Nothing to report"),
                        message,
                        help: "",
                        ai: None,
                        noticed: None,
                        hide: None,
                    },
                    None,
                    None,
                );
            }
            for d in shown.iter().copied() {
                let (icon, color, word) = match d.severity {
                    Severity::Error => ("circle-exclamation-point", t.error, tl!("Error")),
                    Severity::Warning => ("triangle-exclamation-point", t.warning, tl!("Warning")),
                    Severity::Note => ("info", t.note, tl!("Note")),
                    Severity::Hint => ("info", t.text_muted, crate::i18n::tc("severity", "Hint")),
                };
                let place = d.file.as_ref().map(|f| (f.clone(), d.line));
                let help = d.help.join("\n");
                let title = match &d.origin {
                    Some(o) => format!("{word} · {o}"),
                    None => word.to_owned(),
                };
                let page = outcome.place_on_page(d).map(|c| PageRect { page: c.page, x: c.x, y: c.y, width: c.width, height: c.height });
                // Errors and warnings can be explained, when a model is connected.
                let ai = (ai_offered && d.severity >= Severity::Warning).then(|| (app.session.ai_explanation(d), ai_blocked.clone()));
                let noticed = d.noticed.as_ref().map(|p| (p.file.clone(), p.line));
                // Errors are never hidden.
                let hide = (d.severity < Severity::Error).then(|| app.session.is_hidden(d));
                match card(
                    ui,
                    &t,
                    Card { icon, color, title: &title, message: &d.message, help: &help, ai, noticed: noticed.as_ref(), hide },
                    place.as_ref(),
                    page,
                ) {
                    CardAction::Hide(scope) => hide_kind = outcome.diagnostics.iter().position(|x| std::ptr::eq(x, d)).map(|i| (i, scope)),
                    CardAction::ShowAgain => show_again = Some(texcraft_engine::hidden::kind(d)),
                    CardAction::Explain => explain = outcome.diagnostics.iter().position(|x| std::ptr::eq(x, d)),
                    CardAction::CancelAi => cancel_ai = true,
                    CardAction::Open(target) => goto = Some(target),
                    CardAction::ShowOnPage(rect) => on_page = Some(rect),
                    CardAction::Copy => copied = Some((diagnostic_text(d), tl!("The message is copied."))),
                    CardAction::None => {}
                }
            }
            if !outputs.is_empty() {
                egui::CollapsingHeader::new(RichText::new(tl!("Output files")).font(t.strong())).default_open(true).show(ui, |ui| {
                    output_action = output_files(ui, &t, &outputs, browser, file_manager);
                });
            }
            egui::CollapsingHeader::new(RichText::new(tl!("Raw log")).font(t.strong())).show(ui, |ui| {
                if raw_log(ui, &t, &mut app.view.log_search, &outcome.log) {
                    copied = Some((outcome.log.clone(), tl!("The raw log is copied.")));
                }
            });
        });
    });
    if toggle_hidden {
        app.view.log_hidden = !app.view.log_hidden;
    }
    if let Some((index, scope)) = hide_kind
        && let Some(done) = app.run("build.hide_messages", json!({"index": index, "scope": scope}))
    {
        let said = match done["kept"].as_str() {
            Some("user") => tl!("Messages like this are hidden in all projects: Settings › Building shows them again."),
            Some("session") => tl!("Messages like this are hidden in this project until TeXCraft closes: it has no folder to keep the choice in."),
            _ => tl!("Messages like this are hidden in this project: Settings › Building shows them again."),
        };
        app.view.status = Some((said.to_owned(), false));
        app.prefs_changed |= scope == "user";
    }
    if let Some(kind) = show_again
        && app.run("build.show_messages", json!({"kind": kind})).is_some()
    {
        app.view.status = Some((tl!("Messages like this are shown again.").to_owned(), false));
        app.prefs_changed = true;
    }
    match output_action {
        Some((kind, OutputAction::Save)) => ui_cmd::run_with(app, "build.save_output", &json!({"output": kind})),
        Some((kind, OutputAction::Show)) => ui_cmd::run_with(app, "view.show_output", &json!({"output": kind})),
        None => {}
    }
    if let Some((text, said)) = copied {
        ui.ctx().copy_text(text);
        app.view.status = Some((said.to_owned(), false));
    }
    if let Some(rect) = on_page {
        show_on_page(app, rect);
    }
    if let Some(index) = explain {
        crate::ai::explain(app, json!({"index": index}));
    }
    if cancel_ai {
        app.run("ai.cancel", json!({}));
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

/// The raw log's search: what is looked for, the match shown, and whether to scroll to it.
#[derive(Debug, Default)]
pub(crate) struct LogSearch {
    pub query: String,
    /// The match shown, from 0.
    pub current: usize,
    /// Scroll the match shown into view when the log is next drawn.
    pub scroll: bool,
}

/// At most this many matches are marked in the raw log.
const MAX_LOG_MATCHES: usize = 10_000;

/// Where `needle` is in `log`, ignoring the case of ASCII letters: byte ranges, in order, not
/// overlapping, at most [`MAX_LOG_MATCHES`].
pub(crate) fn log_matches(log: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    let (hay, pattern) = (log.as_bytes(), needle.as_bytes());
    let mut found = Vec::new();
    if pattern.is_empty() {
        return found;
    }
    let mut i = 0;
    while i + pattern.len() <= hay.len() && found.len() < MAX_LOG_MATCHES {
        let end = i + pattern.len();
        if hay.get(i..end).is_some_and(|w| w.eq_ignore_ascii_case(pattern)) && log.is_char_boundary(i) && log.is_char_boundary(end) {
            found.push(i..end);
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// The raw log: its search field, the count of matches and the buttons to go through them, a
/// button to copy it, and the log with the matches marked. Returns whether Copy was clicked.
fn raw_log(ui: &mut egui::Ui, t: &Tokens, search: &mut LogSearch, log: &str) -> bool {
    let matches = log_matches(log, &search.query);
    if search.current >= matches.len() {
        search.current = 0;
    }
    let mut copy = false;
    ui.horizontal(|ui| {
        let (enter, shift) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift));
        let field = egui::TextEdit::singleline(&mut search.query).hint_text(tl!("Search the raw log")).desired_width(220.0).font(t.code_font());
        let r = ui.add(field).accessible_name(tl!("Search the raw log"));
        if r.changed() {
            search.current = 0;
            search.scroll = true;
        }
        let step = |search: &mut LogSearch, forward: bool| {
            let n = log_matches(log, &search.query).len();
            if n > 0 {
                search.current = if forward { (search.current + 1) % n } else { (search.current + n - 1) % n };
                search.scroll = true;
            }
        };
        // Enter goes to the next match, Shift+Enter to the one before; the field keeps the keyboard.
        if r.lost_focus() && enter {
            step(search, !shift);
            r.request_focus();
        }
        let count = match matches.len() {
            0 if search.query.is_empty() => String::new(),
            0 => tl!("No results").to_owned(),
            n => tl!("{i} of {n}").replace("{i}", &(search.current + 1).to_string()).replace("{n}", &n.to_string()),
        };
        let label = ui.add(egui::Label::new(RichText::new(&count).font(t.small()).color(t.text_muted)));
        crate::status::announce(ui, &label, &count);
        let any = !matches.is_empty();
        if widgets::icon_button(ui, t, "chevron-up", tl!("Previous Match"), any).clicked() {
            step(search, false);
        }
        if widgets::icon_button(ui, t, "chevron-down", tl!("Next Match"), any).clicked() {
            step(search, true);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            copy = widgets::text_button(ui, t, tl!("Copy the Raw Log"), !log.is_empty()).clicked();
        });
    });
    egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner_large()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        let job = log_layout(t, log, &matches, search.current, ui.available_width());
        let galley = ui.painter().layout_job(job);
        let r = ui.add(egui::Label::new(galley.clone()).selectable(true));
        if std::mem::take(&mut search.scroll)
            && let Some(m) = matches.get(search.current)
        {
            let chars = log.get(..m.start).map_or(0, |s| s.chars().count());
            let at = galley.pos_from_cursor(egui::text::CCursor::new(chars));
            ui.scroll_to_rect(at.translate(r.rect.min.to_vec2()), Some(Align::Center));
        }
    });
    copy
}

/// The raw log laid out with its matches marked: each on the search colour, the one shown on the
/// selection colour and underlined (so it is told apart without colour).
fn log_layout(t: &Tokens, log: &str, matches: &[std::ops::Range<usize>], current: usize, width: f32) -> egui::text::LayoutJob {
    let font = egui::FontId::monospace(t.small_size);
    let plain = egui::TextFormat { font_id: font.clone(), color: t.text, ..Default::default() };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    let mut at = 0;
    for (i, m) in matches.iter().enumerate() {
        job.append(log.get(at..m.start).unwrap_or_default(), 0.0, plain.clone());
        let marked = if i == current {
            egui::TextFormat { background: t.selection, underline: egui::Stroke::new(1.5, t.text), ..plain.clone() }
        } else {
            egui::TextFormat { background: t.search_match, ..plain.clone() }
        };
        job.append(log.get(m.clone()).unwrap_or_default(), 0.0, marked);
        at = m.end;
    }
    job.append(log.get(at..).unwrap_or_default(), 0.0, plain);
    job
}

/// What was asked of an output file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputAction {
    Save,
    Show,
}

/// A file size, for people: bytes, kB or MB.
fn size_text(bytes: usize) -> String {
    crate::i18n::file_size(bytes as u64)
}

/// The last build's files, each with Save As… (Download in the browser) and, on the desktop once
/// saved, Show in Folder.
fn output_files(
    ui: &mut egui::Ui,
    t: &Tokens,
    outputs: &[texcraft_engine::BuildOutput],
    browser: bool,
    file_manager: bool,
) -> Option<(&'static str, OutputAction)> {
    let mut action = None;
    for o in outputs {
        let what = match o.kind {
            "pdf" => tl!("PDF"),
            "pdfx" => tl!("PDF for print"),
            "synctex" => tl!("SyncTeX data"),
            _ => tl!("Raw log"),
        };
        ui.horizontal(|ui| {
            ui.add(icons::image(if o.kind == "log" { "scroll-text" } else { "file-text" }, t.icon_size, t.text_muted));
            ui.label(RichText::new(&o.name).font(t.code_font()));
            ui.label(RichText::new(format!("{what} · {}", size_text(o.bytes))).font(t.small()).color(t.text_muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if file_manager && !browser {
                    let label = tl!("Show in Folder");
                    let saved = o.saved.is_some();
                    let r = widgets::icon_button(ui, t, "folder-open", label, saved);
                    let _ = r.clone().accessible_name(format!("{label} {}", o.name));
                    if !saved {
                        let _ = r.clone().on_disabled_hover_text(tl!("Save it first: the build keeps its files in memory."));
                    }
                    if r.clicked() {
                        action = Some((o.kind, OutputAction::Show));
                    }
                }
                let label = if browser { tl!("Download") } else { tl!("Save As…") };
                let r = widgets::text_button(ui, t, label, true);
                let _ = r.clone().accessible_name(format!("{label} {}", o.name));
                if r.clicked() {
                    action = Some((o.kind, OutputAction::Save));
                }
            });
        });
    }
    action
}

/// What a log card shows.
struct Card<'a> {
    icon: &'a str,
    color: egui::Color32,
    title: &'a str,
    message: &'a str,
    /// How to fix it, muted under the message; may be empty.
    help: &'a str,
    /// For a message AI can explain: its explanation, if asked for, and whether a request is in
    /// flight.
    ai: Option<(Option<&'a texcraft_engine::ai::Explanation>, Option<String>)>,
    /// Where TeX noticed it, when it is shown where it starts: a second link.
    noticed: Option<&'a (String, Option<u32>)>,
    /// For a message that can be hidden (not an error): whether its kind is hidden now. It offers
    /// to hide its kind, or to show it again.
    hide: Option<bool>,
}

/// What was done with a log card.
enum CardAction {
    None,
    /// Its place was clicked: open it in the editor.
    Open((String, Option<u32>)),
    /// Its copy button was clicked.
    Copy,
    /// Its page was clicked: show its area there.
    ShowOnPage(PageRect),
    /// Its Explain button was clicked.
    Explain,
    /// Hide the messages of its kind: in this project (`project`) or all of them (`user`).
    Hide(&'static str),
    /// Show the messages of its kind again.
    ShowAgain,
    /// Its explanation's Cancel was clicked.
    CancelAi,
}

/// One message as a card: its text can be selected, and its button copies it.
/// `page`: the area on the PDF's page it is about, for a print check.
fn card(ui: &mut egui::Ui, t: &Tokens, c: Card<'_>, place: Option<&(String, Option<u32>)>, page: Option<PageRect>) -> CardAction {
    let Card { icon, color, title, message, help, ai, noticed, hide } = c;
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
                        match hide {
                            Some(false) => {
                                let r = widgets::text_button(ui, t, tl!("Hide Like This"), true);
                                let r = r.on_hover_text(tl!("Hide the messages of this kind, here or in all projects"));
                                crate::widgets::keyboard_menu(ui, &r);
                                egui::Popup::menu(&r).show(|ui| {
                                    if top_bar::item(ui, t, None, tl!("In This Project"), None, Ok(())) {
                                        action = CardAction::Hide("project");
                                    }
                                    if top_bar::item(ui, t, None, tl!("In All Projects"), None, Ok(())) {
                                        action = CardAction::Hide("user");
                                    }
                                });
                            }
                            Some(true) if widgets::text_button(ui, t, tl!("Show Again"), true).clicked() => action = CardAction::ShowAgain,
                            Some(true) | None => {}
                        }
                        if let Some((file, line)) = place {
                            let text = match line {
                                Some(l) => format!("{file}:{l}"),
                                None => file.clone(),
                            };
                            let link = ui.add(egui::Link::new(RichText::new(text).font(t.small()).color(t.accent)));
                            let link = link.on_hover_text(tl!("Open in the editor"));
                            if link.clicked() {
                                action = CardAction::Open((file.clone(), *line));
                            }
                        }
                        if let Some(rect) = page {
                            let text = tl!("Page {n}").replace("{n}", &(rect.page + 1).to_string());
                            let link = ui.add(egui::Link::new(RichText::new(text).font(t.small()).color(t.accent)));
                            let link = link.on_hover_text(tl!("Show it on the page"));
                            if link.clicked() {
                                action = CardAction::ShowOnPage(rect);
                            }
                        }
                    });
                });
                ui.add(egui::Label::new(RichText::new(message).color(t.text)).selectable(true));
                if !help.is_empty() {
                    ui.add(egui::Label::new(RichText::new(help).font(t.small()).color(t.text_muted)).selectable(true));
                }
                // Where TeX noticed it, when that is not where it starts: both are places to look.
                if let Some((file, line)) = noticed {
                    let place = line.map_or_else(|| file.clone(), |l| format!("{file}:{l}"));
                    let text = tf!("TeX noticed it at {place}", place = place);
                    let link = ui.add(egui::Link::new(RichText::new(text).font(t.small()).color(t.accent)));
                    if link.on_hover_text(tl!("Open in the editor")).clicked() {
                        action = CardAction::Open((file.clone(), *line));
                    }
                }
                if let Some((explanation, blocked)) = ai {
                    match crate::ai::explanation(ui, t, explanation, blocked.as_deref()) {
                        crate::ai::Act::Explain => action = CardAction::Explain,
                        crate::ai::Act::Cancel => action = CardAction::CancelAi,
                        crate::ai::Act::None => {}
                    }
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
    if let Some(p) = &d.noticed {
        let line = p.line.map(|l| format!(":{l}")).unwrap_or_default();
        text.push_str(&format!("\n    noticed at {}{line}", p.file));
    }
    for h in &d.help {
        text.push_str("\n    ");
        text.push_str(h);
    }
    text
}

/// The print summary as one copies it, untranslated as the messages are.
fn print_summary_text(outcome: &Outcome) -> String {
    let standard = outcome.preflight.as_ref().map_or("PDF/X", |p| p.standard.name());
    match outcome.print_problems() {
        Some(0) => format!("{standard}: ready for print"),
        Some(n) => format!("{standard}: {n} problem{}", if n == 1 { "" } else { "s" }),
        None => String::new(),
    }
}

/// Shows a print check's area on its page of the PDF, marked for a moment.
fn show_on_page(app: &mut App, rect: PageRect) {
    reveal(app);
    app.view.logs = false;
    let now = app.ctx.input(|i| i.time);
    app.pdf.show_place(vec![rect], now);
}

/// Brings the PDF into view: its own window forward, or the main window's split layout.
fn reveal(app: &mut App) {
    if app.view.pdf_window {
        app.ctx.send_viewport_cmd_to(egui::ViewportId::from_hash_of(WINDOW), egui::ViewportCommand::Focus);
    } else if app.view.narrow {
        app.view.pane = crate::Pane::Pdf;
    } else if app.view.layout == crate::Layout::EditorOnly {
        app.view.layout = crate::Layout::Split;
    }
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
            if app.view.pdf_window {
                app.ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            }
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
    reveal(app);
    app.view.logs = false;
    let now = app.ctx.input(|i| i.time);
    app.pdf.show_place(rects, now);
}
