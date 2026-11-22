//! The editor pane (docs/design.md, Regions): tabs of the open files, the editor toolbar, and the
//! source with a gutter of line numbers and error markers and a band on the caret's line.
//!
//! The text surface is still egui's multi-line text field with a highlighting layouter. It will
//! be replaced by TeXCraft's own editor widget (rope buffer, folding, completion, IME through
//! craft-ui's `ime::Composer`, an editable AccessKit node with caret and selection), docs/plan.md
//! phase 2. Toolbar actions already edit through `file.edit`, as the new widget will.

use std::collections::HashMap;
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Align, Align2, Layout, Rect, Sense, vec2};
use serde_json::json;
use texcraft_engine::Severity;
use texcraft_engine::project::{Contents, ProjectPath};

use crate::theme::{Tokens, blend};
use crate::{App, icons, widgets};

/// The text being edited and where it came from.
#[derive(Default)]
pub(crate) struct Editor {
    path: Option<ProjectPath>,
    text: String,
    /// The session revision `text` matches.
    revision: u64,
    /// Select this character range in the text field next frame (after a toolbar edit).
    select: Option<(usize, usize)>,
}

impl Editor {
    /// Reloads the text when another file is shown or the file changed outside the editor.
    fn sync(&mut self, path: Option<&ProjectPath>, revision: u64, project: &texcraft_engine::project::Project) {
        if self.path.as_ref() == path && self.revision == revision {
            return;
        }
        let same_file = self.path.as_ref() == path;
        self.path = path.cloned();
        self.revision = revision;
        let fresh = match path.and_then(|p| project.get(p)) {
            Some(Contents::Text(t)) => t.clone(),
            _ => String::new(),
        };
        if !same_file || fresh != self.text {
            self.text = fresh;
        }
    }
}

/// Diagnostics by line (from 0): the most severe one's severity and message.
type Markers = HashMap<u32, (Severity, String)>;

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.surface);
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        tabs(app, ui);
        widgets::hairline(ui, &t);
        let current = app.session.current().cloned();
        app.editor.sync(current.as_ref(), app.session.revision(), app.session.project());
        let is_text = current.as_ref().is_some_and(|p| matches!(app.session.project().get(p), Some(Contents::Text(_))));
        if is_text {
            toolbar(app, ui);
            widgets::hairline(ui, &t);
        }
        match current {
            None => empty(ui, &t, "file-text", tl!("No file is open"), tl!("Choose a file in the project files to edit it.")),
            Some(path) if !is_text => {
                let bytes = app.session.project().get(&path).map_or(0, Contents::len);
                let detail = tl!("{name} is not a text file ({size} bytes); it cannot be edited here.")
                    .replace("{name}", path.file_name())
                    .replace("{size}", &bytes.to_string());
                empty(ui, &t, "image", tl!("Binary file"), &detail);
            }
            Some(path) => source(app, ui, &path),
        }
    });
}

fn empty(ui: &mut egui::Ui, t: &Tokens, icon: &str, title: &str, detail: &str) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.add(icons::image(icon, 32.0, t.text_muted.gamma_multiply(0.6)));
        ui.add_space(t.unit * 3.0);
        ui.label(egui::RichText::new(title).font(t.strong()));
        ui.add_space(t.unit);
        ui.label(egui::RichText::new(detail).color(t.text_muted));
    });
}

fn tabs(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let open: Vec<ProjectPath> = app.session.open_files().to_vec();
    let current = app.session.current().cloned();
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.tab_height), Sense::hover());
    ui.painter().rect_filled(strip, 0.0, t.frame);
    let mut x = strip.left();
    let mut action = None;
    for path in open {
        let selected = current.as_ref() == Some(&path);
        let name = path.file_name();
        let font = if selected { t.strong() } else { t.body() };
        let text_w = ui.painter().layout_no_wrap(name.to_owned(), font.clone(), t.text).size().x;
        let width = 3.0 * t.unit + t.icon_size + 2.0 * t.unit + text_w + 2.0 * t.unit + 20.0 + 2.0 * t.unit;
        let rect = Rect::from_min_size(egui::pos2(x, strip.top()), vec2(width, t.tab_height));
        x += width;
        let r = ui.interact(rect, ui.scope_id().with(("tab", path.as_str())), Sense::click());
        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, name));
        let fill = if selected {
            t.surface
        } else if r.hovered() {
            blend(t.frame, t.hover)
        } else {
            t.frame
        };
        ui.painter().rect_filled(rect, 0.0, fill);
        if selected {
            ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 2.0)), 0.0, t.accent);
        }
        ui.painter().line_segment([rect.right_top(), rect.right_bottom()], t.hairline());
        let icon_rect = Rect::from_min_size(rect.min + vec2(3.0 * t.unit, 0.0), vec2(t.icon_size, t.tab_height));
        icons::paint(ui, icon_rect, "file-code", t.icon_size - 2.0, if selected { t.accent } else { t.text_muted });
        let color = if selected { t.text } else { t.text_muted };
        ui.painter().text(egui::pos2(icon_rect.right() + 2.0 * t.unit, rect.center().y), Align2::LEFT_CENTER, name, font, color);
        let close = Rect::from_center_size(egui::pos2(rect.right() - 2.0 * t.unit - 10.0, rect.center().y), vec2(20.0, 20.0));
        let c = ui.interact(close, ui.scope_id().with(("tab-close", path.as_str())), Sense::click());
        c.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} {name}", tl!("Close"))));
        if selected || r.hovered() || c.hovered() {
            if c.hovered() {
                ui.painter().rect_filled(close, 4, blend(fill, t.hover));
            }
            icons::paint(ui, close, "x", 14.0, if c.hovered() { t.text } else { t.text_muted });
        }
        if c.clicked() {
            action = Some(("file.close", path.clone()));
        } else if r.clicked() {
            action = Some(("file.open", path.clone()));
        }
        let _ = r.on_hover_text(path.as_str());
    }
    if let Some((id, path)) = action {
        app.run(id, json!({"path": path.as_str()}));
    }
}

/// A toolbar edit: wrap the selection in `before`…`after`, or insert `before` + `placeholder` +
/// `after` and select the placeholder.
struct Wrap {
    before: &'static str,
    after: &'static str,
    placeholder: &'static str,
}

const fn wrap(before: &'static str, after: &'static str, placeholder: &'static str) -> Wrap {
    Wrap { before, after, placeholder }
}

const FIGURE: Wrap = wrap(
    "\\begin{figure}[htbp]\n  \\centering\n  \\includegraphics[width=0.8\\linewidth]{",
    "}\n  \\caption{Caption}\n  \\label{fig:label}\n\\end{figure}\n",
    "image",
);
const TABLE: Wrap = wrap(
    "\\begin{table}[htbp]\n  \\centering\n  \\begin{tabular}{ll}\n    ",
    " & \\\\\n     & \\\\\n  \\end{tabular}\n  \\caption{Caption}\n  \\label{tab:label}\n\\end{table}\n",
    "Cell",
);

/// An entry of a toolbar drop-down; returns whether it was chosen.
fn menu_entry(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> bool {
    let chosen = ui.add(egui::Button::new(text).frame_when_inactive(false)).clicked();
    if chosen {
        ui.close();
    }
    chosen
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut chosen: Option<Wrap> = None;
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0.0, t.surface);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(t.unit * 2.0);
        let formatted = tl!("The formatted view comes with docs/plan.md phase 3.");
        widgets::segmented(ui, &t, "mode", &[(tl!("Source"), None), (tl!("Formatted"), Some(formatted))], 0);
        divider(ui, &t);
        let heading = widgets::icon_button(ui, &t, "heading-1", tl!("Heading"), true);
        egui::Popup::menu(&heading).show(|ui| {
            ui.set_min_width(180.0);
            for (label, cmd, size) in [
                (tl!("Section"), "\\section{", 16.0),
                (tl!("Subsection"), "\\subsection{", 14.5),
                (tl!("Subsubsection"), "\\subsubsection{", 13.5),
                (tl!("Paragraph"), "\\paragraph{", 13.0),
            ] {
                if menu_entry(ui, egui::RichText::new(label).font(egui::FontId::new(size, crate::fonts::semibold()))) {
                    chosen = Some(Wrap { before: cmd, after: "}", placeholder: "Title" });
                }
            }
        });
        if letter_button(ui, &t, "B", tl!("Bold")).clicked() {
            chosen = Some(wrap("\\textbf{", "}", "text"));
        }
        if widgets::icon_button(ui, &t, "italic", tl!("Italic"), true).clicked() {
            chosen = Some(wrap("\\textit{", "}", "text"));
        }
        divider(ui, &t);
        let math = widgets::icon_button(ui, &t, "sigma", tl!("Math"), true);
        egui::Popup::menu(&math).show(|ui| {
            ui.set_min_width(180.0);
            if menu_entry(ui, tl!("Inline formula")) {
                chosen = Some(wrap("\\(", "\\)", "x"));
            }
            if menu_entry(ui, tl!("Displayed formula")) {
                chosen = Some(wrap("\\[\n  ", "\n\\]", "x"));
            }
            if menu_entry(ui, tl!("Numbered equation")) {
                chosen = Some(wrap("\\begin{equation}\n  ", "\n  \\label{eq:label}\n\\end{equation}", "x"));
            }
        });
        if widgets::icon_button(ui, &t, "tag", tl!("Cross-reference"), true).clicked() {
            chosen = Some(wrap("\\ref{", "}", "label"));
        }
        if widgets::icon_button(ui, &t, "book-open", tl!("Citation"), true).clicked() {
            chosen = Some(wrap("\\cite{", "}", "key"));
        }
        if widgets::icon_button(ui, &t, "link", tl!("Link"), true).clicked() {
            chosen = Some(wrap("\\href{https://}{", "}", "text"));
        }
        divider(ui, &t);
        if widgets::icon_button(ui, &t, "image", tl!("Figure"), true).clicked() {
            chosen = Some(FIGURE);
        }
        if widgets::icon_button(ui, &t, "table", tl!("Table"), true).clicked() {
            chosen = Some(TABLE);
        }
        if widgets::icon_button(ui, &t, "list", tl!("Bulleted list"), true).clicked() {
            chosen = Some(wrap("\\begin{itemize}\n  \\item ", "\n\\end{itemize}", "Item"));
        }
        if widgets::icon_button(ui, &t, "list-ordered", tl!("Numbered list"), true).clicked() {
            chosen = Some(wrap("\\begin{enumerate}\n  \\item ", "\n\\end{enumerate}", "Item"));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(t.unit * 2.0);
            let find = tl!("Find and replace comes with the new editor (docs/plan.md phase 2).");
            widgets::icon_button(ui, &t, "search", tl!("Find and replace"), false).on_disabled_hover_text(find);
        });
    });
    if let Some(w) = chosen {
        apply_wrap(app, ui.ctx(), &w);
    }
}

fn divider(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(t.unit);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, t.toolbar_height - 4.0 * t.unit), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.separator);
    ui.add_space(t.unit);
}

/// A toolbar button showing a letter, for actions whose icon is a letter (bold).
fn letter_button(ui: &mut egui::Ui, t: &Tokens, letter: &str, label: &str) -> egui::Response {
    let side = t.toolbar_height - 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if r.hovered() || r.has_focus() {
        ui.painter().rect_filled(rect, t.corner(), blend(t.surface, t.hover));
    }
    let color = if r.hovered() { t.text } else { t.text_muted };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, egui::FontId::new(15.0, crate::fonts::semibold()), color);
    widgets::focus_ring(ui, t, &r, rect, t.corner());
    r.on_hover_text(label)
}

fn editor_id(path: &ProjectPath) -> egui::Id {
    egui::Id::unique(("editor", path.as_str()))
}

/// Applies a toolbar edit at the editor's selection through `file.edit`.
fn apply_wrap(app: &mut App, ctx: &egui::Context, w: &Wrap) {
    let Some(path) = app.session.current().cloned() else { return };
    let text = &app.editor.text;
    let (a, b) = egui::text_edit::TextEditState::load(ctx, editor_id(&path)).and_then(|s| s.cursor.char_range()).map_or((0, 0), |r| {
        let (x, y): (usize, usize) = (r.primary.index.into(), r.secondary.index.into());
        (x.min(y), x.max(y))
    });
    let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
    let (start, end) = (byte(a), byte(b));
    let selected = text.get(start..end).unwrap_or_default();
    let inner = if selected.is_empty() { w.placeholder } else { selected };
    let replacement = format!("{}{inner}{}", w.before, w.after);
    let inner_chars = inner.chars().count();
    let select_from = a + w.before.chars().count();
    if app.run("file.edit", json!({"path": path.as_str(), "start": start, "end": end, "text": replacement})).is_some() {
        app.editor.select = Some((select_from, select_from + inner_chars));
        app.view.last_edit = Some(ctx.input(|i| i.time));
    }
}

/// Lines (from 0) of `path` with warnings or errors.
fn markers(app: &App, path: &ProjectPath) -> Markers {
    let mut out = Markers::new();
    let Some(outcome) = app.session.last_outcome() else { return out };
    for d in outcome.diagnostics.iter().filter(|d| d.file.as_deref() == Some(path.as_str()) && d.severity >= Severity::Warning) {
        let Some(line) = d.line.map(|l| l.saturating_sub(1)) else { continue };
        match out.get(&line) {
            Some((s, _)) if *s >= d.severity => {}
            _ => {
                out.insert(line, (d.severity, d.message.clone()));
            }
        }
    }
    out
}

fn highlight(text: &str, t: &Tokens, markers: &Markers) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = t.code_font();
    let mut line = 0u32;
    for token in texcraft_latex::lex(text) {
        let color = t.syntax(token.kind);
        let Some(run) = text.get(token.range.clone()) else { continue };
        for piece in run.split_inclusive('\n') {
            let background = match markers.get(&line) {
                Some((Severity::Error, _)) => t.error_line,
                _ => egui::Color32::TRANSPARENT,
            };
            let format = TextFormat { font_id: font.clone(), color, background, line_height: Some(t.code_line_height), ..Default::default() };
            job.append(piece, 0.0, format);
            if piece.ends_with('\n') {
                line += 1;
            }
        }
    }
    job
}

fn source(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    let t = app.tokens;
    let markers = markers(app, path);
    let id = editor_id(path);
    if let Some((goto_path, line)) = app.view.goto.take() {
        if &goto_path == path {
            let target = line.saturating_sub(1) as usize;
            let chars: usize = app.editor.text.split_inclusive('\n').take(target).map(|l| l.chars().count()).sum();
            app.editor.select = Some((chars, chars));
        } else {
            app.view.goto = Some((goto_path, line));
        }
    }
    if let Some((a, b)) = app.editor.select.take() {
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
        state.store(ui.ctx(), id);
        ui.memory_mut(|m| m.request_focus(id));
    }
    let lines = app.editor.text.matches('\n').count() + 1;
    let digits = lines.to_string().len().max(2) as f32;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&t.code_font(), '0'));
    let width = digits * char_w + 7.0 * t.unit;
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
        let mut job = highlight(text.as_str(), &t, &markers);
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let name = format!("{} {}", path.file_name(), tl!("source"));
    let mut changed = false;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let (gutter_rect, _) = ui.allocate_exact_size(vec2(width, 0.0), Sense::hover());
            let band = ui.painter().add(egui::Shape::Noop);
            let output = egui::TextEdit::multiline(&mut app.editor.text)
                .id(id)
                .code_editor()
                .font(t.code_font())
                .frame(egui::Frame::NONE)
                .margin(egui::Margin { left: 4, right: 12, top: 8, bottom: 40 })
                .desired_width(f32::INFINITY)
                .desired_rows(40)
                .layouter(&mut layouter)
                .show(ui);
            let response = output.response.response.clone().accessible_name(name);
            changed = response.changed();
            let caret = output.cursor_range.map(|r| r.primary.index).or_else(|| output.state.cursor.char_range().map(|r| r.primary.index));
            let caret_line = caret.map(|c| line_col(&app.editor.text, c.into()));
            app.view.cursor = caret_line.map(|(l, c)| (l + 1, c + 1));
            let gutter = Gutter { left: gutter_rect.left(), width, caret_line: caret_line.map(|(l, _)| l), band, focused: response.has_focus() };
            paint_gutter(ui, &t, &output, &gutter, &markers);
        });
    });
    if changed {
        let text = app.editor.text.clone();
        if let Some(r) = app.run("file.write", json!({"path": path.as_str(), "text": text})) {
            app.editor.revision = r.get("revision").and_then(serde_json::Value::as_u64).unwrap_or(app.editor.revision);
        }
        app.view.last_edit = Some(ui.ctx().input(|i| i.time));
    }
}

/// Line and column (from 0, in characters) of character index `c`.
fn line_col(text: &str, c: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for ch in text.chars().take(c) {
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Where the gutter is and what it marks.
struct Gutter {
    left: f32,
    width: f32,
    caret_line: Option<u32>,
    /// The shape slot, below the text, that the caret-line band is painted into.
    band: egui::layers::ShapeIdx,
    focused: bool,
}

fn paint_gutter(ui: &egui::Ui, t: &Tokens, output: &egui::text_edit::TextEditOutput, g: &Gutter, markers: &Markers) {
    let origin = output.galley_pos;
    let font = egui::FontId::new(t.code_size - 1.5, egui::FontFamily::Monospace);
    let right = g.left + g.width - 3.0 * t.unit;
    let full_width = output.response.response.rect.right() - g.left;
    let mut line = 0u32;
    let mut starts_line = true;
    for row in &output.galley.rows {
        let (top, bottom) = (origin.y + row.min_y(), origin.y + row.max_y());
        if starts_line {
            let is_caret = g.caret_line == Some(line);
            if is_caret && g.focused {
                let rect = Rect::from_min_max(egui::pos2(g.left, top), egui::pos2(g.left + full_width, bottom));
                ui.painter().set(g.band, egui::Shape::rect_filled(rect, 0.0, t.current_line));
            }
            let color = if is_caret { t.text } else { t.text_muted };
            ui.painter().text(egui::pos2(right, (top + bottom) / 2.0), Align2::RIGHT_CENTER, (line + 1).to_string(), font.clone(), color);
            if let Some((severity, message)) = markers.get(&line) {
                let (icon, color) =
                    if *severity == Severity::Error { ("circle-exclamation-point", t.error) } else { ("triangle-exclamation-point", t.warning) };
                let rect = Rect::from_center_size(egui::pos2(g.left + 2.5 * t.unit, (top + bottom) / 2.0), vec2(14.0, 14.0));
                icons::paint(ui, rect, icon, 13.0, color);
                let r = ui.interact(rect, ui.scope_id().with(("marker", line)), Sense::hover());
                let _ = r.on_hover_text(message.as_str());
            }
        }
        starts_line = row.ends_with_newline;
        if row.ends_with_newline {
            line += 1;
        }
    }
}
