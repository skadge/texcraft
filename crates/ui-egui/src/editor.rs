//! The editor pane (docs/design.md, Regions): tabs of the open files, the editor toolbar, and the
//! source with a gutter of line numbers and error markers and a band on the caret's line.
//!
//! The text is TeXCraft's own editor widget ([`crate::source`]). Each open file keeps its text,
//! caret and undo history while another is shown; edits go to the session with `file.write`, and
//! a change the session makes (an agent's edit, a reload) comes back as one undoable edit.

use std::collections::HashMap;

use egui::{Align, Align2, Layout, Rect, Sense, vec2};
use serde_json::json;
use texcraft_buffer::{EditKind, Selection, Transaction};
use texcraft_engine::Severity;
use texcraft_engine::project::{Contents, ProjectPath};
use texcraft_vim::Effect;

use crate::source::{Doc, Markers, SourceEditor};
use crate::theme::{Tokens, blend};
use crate::{App, icons, widgets};

/// The text being edited and where it came from.
#[derive(Default)]
pub(crate) struct Editor {
    path: Option<ProjectPath>,
    /// The session revision the shown text matches.
    revision: u64,
    pub source: SourceEditor,
    /// The buffer version last sent to the session (or taken from it).
    sent: u64,
    /// The other open files' text, caret and undo history.
    parked: HashMap<ProjectPath, Doc>,
    /// The find and replace bar, when open.
    pub find: Option<crate::find_bar::FindBar>,
    /// Give the editor the keyboard next frame.
    pub focus: bool,
    /// Vim's last message (an error when true) and when it came, for the Vim line.
    vim_message: Option<(String, bool, f64)>,
}

fn session_text(project: &texcraft_engine::project::Project, path: Option<&ProjectPath>) -> Option<String> {
    match path.and_then(|p| project.get(p)) {
        Some(Contents::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

impl Editor {
    /// Shows another file's text, or takes a change the session made to the shown one.
    fn sync(&mut self, path: Option<&ProjectPath>, revision: u64, project: &texcraft_engine::project::Project) {
        if self.path.as_ref() != path {
            if let Some(old) = self.path.take() {
                let doc = std::mem::take(&mut self.source.doc);
                self.parked.insert(old, doc);
            }
            self.parked.retain(|p, _| project.get(p).is_some());
            let fresh = session_text(project, path).unwrap_or_default();
            let mut doc = path.and_then(|p| self.parked.remove(p)).unwrap_or_else(|| Doc::new(&fresh));
            if doc.buffer.text() != fresh {
                let _ = doc.buffer.replace_all(&fresh, &doc.selection);
                doc.selection = doc.selection.clone().clamp(doc.buffer.len_chars());
            }
            self.source.set_doc(doc);
            self.sent = self.source.doc.buffer.version();
            self.path = path.cloned();
            self.revision = revision;
            return;
        }
        if self.revision == revision {
            return;
        }
        self.revision = revision;
        let fresh = session_text(project, path).unwrap_or_default();
        let doc = &mut self.source.doc;
        if doc.buffer.text() != fresh {
            let _ = doc.buffer.replace_all(&fresh, &doc.selection);
            doc.selection = doc.selection.clone().clamp(doc.buffer.len_chars());
        }
        self.sent = doc.buffer.version();
    }
}

/// How long Vim's messages stay on its line, in seconds.
const MESSAGE_SECONDS: f64 = 8.0;

/// Vim's line, at the bottom of the editor as in Vim: the command line being typed (`:`, `/`, `?`)
/// or Vim's last message on the left; the mode, the keys of a command in progress and the
/// register being recorded on the right. Both are announced when they change.
fn vim_line(app: &mut App, ui: &mut egui::Ui) {
    let Some(vim) = app.editor.source.vim.as_ref() else { return };
    let t = app.tokens;
    let now = ui.ctx().input(|i| i.time);
    if vim.command_line().is_some() || app.editor.vim_message.as_ref().is_some_and(|(_, _, at)| now - at > MESSAGE_SECONDS) {
        app.editor.vim_message = None;
    }
    let mut mode = vim.mode().label().to_owned();
    let pending = vim.pending();
    if !pending.is_empty() {
        mode.push(' ');
        mode.push_str(&pending);
    }
    if let Some(r) = vim.recording() {
        mode.push_str(&tl!(" · recording @{r}").replace("{r}", &r.to_string()));
    }
    let (left, color) = match (vim.command_line(), &app.editor.vim_message) {
        (Some(line), _) => (format!("{line}\u{258F}"), t.text),
        (None, Some((m, error, _))) => (m.clone(), if *error { t.error } else { t.text }),
        (None, None) => (String::new(), t.text),
    };
    let font = t.code_font();
    let frame = egui::Frame::new().fill(t.raised).inner_margin(egui::Margin::symmetric(10, 3));
    egui::Panel::bottom(egui::Id::unique("texcraft-vim-line")).exact_size(t.code_line_height + 8.0).frame(frame).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            let line = ui.add(egui::Label::new(egui::RichText::new(&left).font(font.clone()).color(color)).truncate());
            crate::status::announce(ui, &line, &left);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = ui.label(egui::RichText::new(&mode).font(font.clone()).color(t.text_muted));
                crate::status::announce(ui, &label, &mode);
            });
        });
    });
    if !left.is_empty() {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(MESSAGE_SECONDS));
    }
}

/// Before the window's shortcuts: Vim's Ctrl keys go to the shown file's editor.
pub(crate) fn claim_keys(app: &mut App, ctx: &egui::Context) {
    if let Some(path) = app.editor.path.clone() {
        app.editor.source.claim_keys(ctx, editor_id(&path));
    }
}

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
            crate::find_bar::show(app, ui);
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

/// The source edit of each `edit.*` command (Insert and Format menus, toolbar, shortcuts).
fn wrap_for(id: &str) -> Option<Wrap> {
    Some(match id {
        "edit.section" => wrap("\\section{", "}", "Title"),
        "edit.subsection" => wrap("\\subsection{", "}", "Title"),
        "edit.subsubsection" => wrap("\\subsubsection{", "}", "Title"),
        "edit.paragraph" => wrap("\\paragraph{", "}", "Title"),
        "edit.bold" => wrap("\\textbf{", "}", "text"),
        "edit.italic" => wrap("\\textit{", "}", "text"),
        "edit.math_inline" => wrap("\\(", "\\)", "x"),
        "edit.math_display" => wrap("\\[\n  ", "\n\\]", "x"),
        "edit.equation" => wrap("\\begin{equation}\n  ", "\n  \\label{eq:label}\n\\end{equation}", "x"),
        "edit.ref" => wrap("\\ref{", "}", "label"),
        "edit.cite" => wrap("\\cite{", "}", "key"),
        "edit.link" => wrap("\\href{https://}{", "}", "text"),
        "edit.figure" => FIGURE,
        "edit.table" => TABLE,
        "edit.itemize" => wrap("\\begin{itemize}\n  \\item ", "\n\\end{itemize}", "Item"),
        "edit.enumerate" => wrap("\\begin{enumerate}\n  \\item ", "\n\\end{enumerate}", "Item"),
        _ => return None,
    })
}

/// Runs an `edit.*` command on the shown file's selection; false when no text file is shown.
pub(crate) fn edit(app: &mut App, id: &str) -> bool {
    let shown_text = app.session.current().is_some_and(|p| matches!(app.session.project().get(p), Some(Contents::Text(_))));
    if !shown_text {
        return false;
    }
    let source = &mut app.editor.source;
    match id {
        "edit.find" | "edit.replace" => {
            let old = app.editor.find.take();
            app.editor.find = Some(crate::find_bar::FindBar::open(source, id == "edit.replace", old));
            return true;
        }
        "edit.undo" | "edit.redo" => {
            let sel = if id == "edit.undo" { source.doc.buffer.undo() } else { source.doc.buffer.redo() };
            if let Some(sel) = sel {
                let len = source.doc.buffer.len_chars();
                source.doc.selection = sel.clamp(len);
                source.reveal = true;
            }
            app.editor.focus = true;
            return true;
        }
        "edit.select_all" => {
            let len = source.doc.buffer.len_chars();
            source.select(0, len);
            app.editor.focus = true;
            return true;
        }
        "edit.goto_line" => {
            let prompt = crate::dialogs::Prompt {
                title: tl!("Go to Line"),
                label: tl!("Line number"),
                value: String::new(),
                action: tl!("Go"),
                command: "view.goto_line",
                params: Box::new(|_| json!({})),
                focus: true,
            };
            app.view.dialog = Some(crate::dialogs::Dialog::Prompt(prompt));
            return true;
        }
        _ => {}
    }
    match wrap_for(id) {
        Some(w) if shown_text => {
            let ctx = app.ctx.clone();
            apply_wrap(app, &ctx, &w);
            true
        }
        _ => false,
    }
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

/// The toolbar's groups after the mode switch, in the order they give way to the More menu when the
/// pane is narrow: each with its width and its entries for that menu.
const GROUPS: [(f32, &[(&str, &str)]); 3] = [
    (
        100.0,
        &[
            ("Section", "edit.section"),
            ("Subsection", "edit.subsection"),
            ("Subsubsection", "edit.subsubsection"),
            ("Paragraph", "edit.paragraph"),
            ("Bold", "edit.bold"),
            ("Italic", "edit.italic"),
        ],
    ),
    (
        130.0,
        &[
            ("Inline formula", "edit.math_inline"),
            ("Displayed formula", "edit.math_display"),
            ("Numbered equation", "edit.equation"),
            ("Cross-reference", "edit.ref"),
            ("Citation", "edit.cite"),
            ("Link", "edit.link"),
        ],
    ),
    (130.0, &[("Figure", "edit.figure"), ("Table", "edit.table"), ("Bulleted list", "edit.itemize"), ("Numbered list", "edit.enumerate")]),
];

/// A toolbar button that runs `id`.
fn tool(ui: &mut egui::Ui, t: &Tokens, (icon, label, id): (&str, &str, &'static str), chosen: &mut Option<&'static str>) {
    if widgets::icon_button(ui, t, icon, label, true).clicked() {
        *chosen = Some(id);
    }
}

fn group(ui: &mut egui::Ui, t: &Tokens, index: usize, chosen: &mut Option<&'static str>) {
    divider(ui, t);
    match index {
        0 => {
            let heading = widgets::icon_button(ui, t, "heading-1", tl!("Heading"), true);
            egui::Popup::menu(&heading).show(|ui| {
                ui.set_min_width(180.0);
                for (label, id, size) in [
                    (tl!("Section"), "edit.section", 16.0),
                    (tl!("Subsection"), "edit.subsection", 14.5),
                    (tl!("Subsubsection"), "edit.subsubsection", 13.5),
                    (tl!("Paragraph"), "edit.paragraph", 13.0),
                ] {
                    if menu_entry(ui, egui::RichText::new(label).font(egui::FontId::new(size, crate::fonts::semibold()))) {
                        *chosen = Some(id);
                    }
                }
            });
            if letter_button(ui, t, "B", tl!("Bold")).clicked() {
                *chosen = Some("edit.bold");
            }
            tool(ui, t, ("italic", tl!("Italic"), "edit.italic"), chosen);
        }
        1 => {
            let math = widgets::icon_button(ui, t, "sigma", tl!("Math"), true);
            egui::Popup::menu(&math).show(|ui| {
                ui.set_min_width(180.0);
                for (label, id) in [
                    (tl!("Inline formula"), "edit.math_inline"),
                    (tl!("Displayed formula"), "edit.math_display"),
                    (tl!("Numbered equation"), "edit.equation"),
                ] {
                    if menu_entry(ui, label) {
                        *chosen = Some(id);
                    }
                }
            });
            tool(ui, t, ("tag", tl!("Cross-reference"), "edit.ref"), chosen);
            tool(ui, t, ("book-open", tl!("Citation"), "edit.cite"), chosen);
            tool(ui, t, ("link", tl!("Link"), "edit.link"), chosen);
        }
        _ => {
            tool(ui, t, ("image", tl!("Figure"), "edit.figure"), chosen);
            tool(ui, t, ("table", tl!("Table"), "edit.table"), chosen);
            tool(ui, t, ("list", tl!("Bulleted list"), "edit.itemize"), chosen);
            tool(ui, t, ("list-ordered", tl!("Numbered list"), "edit.enumerate"), chosen);
        }
    }
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut chosen: Option<&'static str> = None;
    // The pane may be wider than what is shown of it: lay out for what is visible.
    let visible = ui.available_width().min(ui.clip_rect().right() - ui.cursor().left());
    ui.allocate_ui_with_layout(vec2(visible, t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.set_max_width(visible);
        ui.painter().rect_filled(ui.max_rect(), 0.0, t.surface);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(t.unit * 2.0);
        let formatted = tl!("The formatted view comes with docs/plan.md phase 3.");
        widgets::segmented(ui, &t, "mode", &[(tl!("Source"), None), (tl!("Formatted"), Some(formatted))], 0);
        // Room for Find and the More menu on the right.
        let mut room = ui.max_rect().right() - ui.cursor().left() - 80.0;
        let mut hidden: Vec<usize> = Vec::new();
        for (i, (width, _)) in GROUPS.iter().enumerate() {
            if hidden.is_empty() && *width <= room {
                group(ui, &t, i, &mut chosen);
                room -= width;
            } else {
                hidden.push(i);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(t.unit * 2.0);
            if widgets::icon_button(ui, &t, "search", tl!("Find and replace"), true).clicked() {
                chosen = Some("edit.replace");
            }
            if !hidden.is_empty() {
                let more = widgets::icon_button(ui, &t, "ellipsis", tl!("More formatting"), true);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(180.0);
                    for (n, i) in hidden.iter().enumerate() {
                        if n > 0 {
                            ui.separator();
                        }
                        for (label, id) in GROUPS[*i].1 {
                            if menu_entry(ui, crate::i18n::t(label)) {
                                chosen = Some(id);
                            }
                        }
                    }
                });
            }
        });
    });
    if let Some(id) = chosen {
        crate::ui_cmd::run(app, id);
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

/// Applies a toolbar edit at the editor's selection, as one undoable edit. The text goes to the
/// session with the editor's next frame.
fn apply_wrap(app: &mut App, _ctx: &egui::Context, w: &Wrap) {
    let source = &mut app.editor.source;
    let doc = &mut source.doc;
    let r = doc.selection.primary();
    let (a, b) = (r.start(), r.end());
    let selected = doc.buffer.slice(a, b);
    let inner = if selected.is_empty() { w.placeholder.to_owned() } else { selected };
    let replacement = format!("{}{inner}{}", w.before, w.after);
    let select_from = a + w.before.chars().count();
    let after = Selection::single(select_from, select_from + inner.chars().count());
    let before = doc.selection.clone();
    if doc.buffer.edit(Transaction::replace(a, b, &replacement), &before, &after, EditKind::Other).is_ok() {
        doc.buffer.seal();
        source.select(select_from, select_from + inner.chars().count());
        source.reveal = true;
    }
}

/// Lines (from 0) of `path` with warnings or errors.
/// The gutter's markers: the last build's errors and warnings, and the language server's hints
/// on the text as it is now.
fn markers(app: &mut App, path: &ProjectPath) -> Markers {
    let mut out = Markers::new();
    for hint in app.session.lint(path) {
        let line = hint.line as u32;
        if out.get(&line).is_none_or(|(s, _)| *s < hint.severity) {
            out.insert(line, (hint.severity, hint.message.clone()));
        }
    }
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

fn source(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    let t = app.tokens;
    let markers = markers(app, path);
    let id = editor_id(path);
    app.region(crate::Region::Editor, id);
    if let Some((goto_path, line)) = app.view.goto.take() {
        if &goto_path == path {
            let b = &app.editor.source.doc.buffer;
            let at = b.line_to_char(line.saturating_sub(1) as usize);
            app.editor.source.select(at, at);
            ui.memory_mut(|m| m.request_focus(id));
        } else {
            app.view.goto = Some((goto_path, line));
        }
    }
    if std::mem::take(&mut app.editor.focus) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    // The hints' text, underlined.
    let b = &app.editor.source.doc.buffer;
    let hints = app
        .session
        .lint(path)
        .iter()
        .map(|h| {
            let at = |line: usize, col: usize| {
                (b.line_to_char(line) + col).min(b.line_end(line.min(b.len_lines().saturating_sub(1))).max(b.line_to_char(line)))
            };
            (at(h.line, h.column), at(h.end_line, h.end_column))
        })
        .collect();
    app.editor.source.hints = hints;
    app.editor.source.set_vim(app.prefs.vim);
    app.editor.source.auto_close = app.prefs.auto_close;
    vim_line(app, ui);
    let name = format!("{} {}", path.file_name(), tl!("source"));
    let out = app.editor.source.show(ui, &t, id, &markers, &name, "  ");
    app.view.cursor = Some((out.caret.0 as u32 + 1, out.caret.1 as u32 + 1));
    // Typing, and edits made outside the widget (the toolbar, the Insert menu).
    let version = app.editor.source.doc.buffer.version();
    if out.changed || version != app.editor.sent {
        app.editor.sent = version;
        let text = app.editor.source.doc.buffer.text();
        if let Some(r) = app.run("file.write", json!({"path": path.as_str(), "text": text})) {
            app.editor.revision = r.get("revision").and_then(serde_json::Value::as_u64).unwrap_or(app.editor.revision);
        }
        app.view.last_edit = Some(ui.ctx().input(|i| i.time));
    }
    // After the text went to the session, so the language server sees what was typed.
    if let Some((line, column)) = out.complete_at {
        let items = app.session.complete(path, line, column);
        app.editor.source.offer(items);
        ui.ctx().request_repaint();
    }
    for effect in out.effects {
        match effect {
            Effect::Save | Effect::SaveAll => crate::ui_cmd::run(app, "project.save"),
            Effect::Close { .. } => {
                let _ = app.run("file.close", json!({"path": path.as_str()}));
            }
            Effect::Open(name) => {
                let _ = app.run("file.open", json!({"path": name}));
            }
            // Vim's messages show on its line, at the bottom of the editor.
            Effect::Message(m) => app.editor.vim_message = Some((m, false, ui.ctx().input(|i| i.time))),
            Effect::Error(m) => app.editor.vim_message = Some((m, true, ui.ctx().input(|i| i.time))),
            Effect::Copy(_) | Effect::Scroll(_) => {}
        }
    }
}
