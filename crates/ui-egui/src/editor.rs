//! The editor pane (docs/design.md, Regions): tabs of the open files, the editor toolbar, and the
//! source with a gutter of line numbers and error markers and a band on the caret's line.
//!
//! The text is TeXCraft's own editor widget ([`crate::source`]). Each open file keeps its text,
//! caret and undo history while another is shown. Edits go to the session as edits (`file.edit`,
//! with their origin), and the session's edits come back from its edit log: an agent's as an
//! undoable edit, a peer's or one made outside TeXCraft without entering the undo history.

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
    /// The session revisions of the edits this editor sent since it last caught up.
    mine: Vec<u64>,
    /// The other open files' text, caret and undo history, with the revision each matches.
    parked: HashMap<ProjectPath, (Doc, u64)>,
    /// The find and replace bar, when open.
    pub find: Option<crate::find_bar::FindBar>,
    /// Give the editor the keyboard next frame.
    pub focus: bool,
    /// Vim's last message (an error when true) and when it came, for the Vim line.
    vim_message: Option<(String, bool, f64)>,
    /// Which files are shown formatted, and that view's state.
    pub formatted: crate::formatted::Formatted,
    /// Suggest mode: edits become suggestions instead of changing the file.
    pub suggest: bool,
    /// In Suggest mode, the text shown: the file with its suggestions in it.
    pub shown: Option<crate::suggest::Shown>,
    /// Where the caret goes once the suggestions are shown again.
    pub caret_hint: Option<crate::suggest::CaretHint>,
    /// The caret before the editor's last frame, to tell a deletion forwards from one backwards.
    pub caret_before: usize,
    /// The spellings offered for the last word asked about (the context menu asks every frame).
    pub spelling_suggestions: Option<(String, Vec<String>)>,
}

fn session_text(project: &texcraft_engine::project::Project, path: Option<&ProjectPath>) -> Option<String> {
    match path.and_then(|p| project.get(p)) {
        Some(Contents::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

impl Editor {
    /// Shows another file's text, or takes the changes the session made to the shown one. The
    /// shown file's own edits must have gone to the session first ([`send_edits`]).
    fn sync(&mut self, path: Option<&ProjectPath>, session: &texcraft_engine::Session) {
        let project = session.project();
        // The text shown in Suggest mode is not the file's: it is made again from the session
        // (`suggest::refresh`) while suggesting, and gives way to the file's otherwise.
        if let Some(shown) = &self.shown {
            let still = self.suggest && path == Some(&shown.path) && self.path.as_ref() == path && !self.formatted.shows(&shown.path);
            if still {
                self.revision = session.revision();
                self.mine.clear();
                return;
            }
            let fresh = session_text(project, Some(&shown.path)).unwrap_or_default();
            crate::suggest::show_text(&mut self.source.doc, &fresh);
            self.shown = None;
            self.revision = session.revision();
            self.mine.clear();
        }
        if self.revision != session.revision() {
            if let Some(shown) = self.path.as_ref().filter(|p| project.get(p).is_some()) {
                catch_up(&mut self.source.doc, shown, self.revision, &self.mine, session);
            }
            self.mine.clear();
            self.revision = session.revision();
        }
        if self.path.as_ref() == path {
            return;
        }
        if let Some(old) = self.path.take() {
            let doc = std::mem::take(&mut self.source.doc);
            self.parked.insert(old, (doc, self.revision));
        }
        self.parked.retain(|p, _| project.get(p).is_some());
        let mut doc = match path.and_then(|p| self.parked.remove(p)) {
            Some((mut doc, since)) => {
                if let Some(path) = path {
                    catch_up(&mut doc, path, since, &[], session);
                }
                doc
            }
            None => Doc::new(&session_text(project, path).unwrap_or_default()),
        };
        doc.buffer.take_edits();
        self.source.set_doc(doc);
        self.path = path.cloned();
    }
}

/// Makes the session's edits of `path` after `since` in `doc` (except the editor's own), then
/// anything else that changed the file (a reload) as one edit.
fn catch_up(doc: &mut Doc, path: &ProjectPath, since: u64, mine: &[u64], session: &texcraft_engine::Session) {
    for e in session.edits_since(path, since).unwrap_or_default().into_iter().filter(|e| !mine.contains(&e.revision)) {
        let after = doc.selection.map(&e.changes);
        let made = if e.origin.is_foreign() {
            doc.buffer.receive(e.changes.clone(), e.origin)
        } else {
            doc.buffer.edit(e.changes.clone(), &doc.selection, &after, EditKind::Other)
        };
        if made.is_err() {
            break;
        }
        doc.selection = after;
    }
    let fresh = session_text(session.project(), Some(path)).unwrap_or_default();
    if doc.buffer.text() != fresh {
        let tx = Transaction::diff(&doc.buffer.text(), &fresh);
        let after = doc.selection.map(&tx);
        let _ = doc.buffer.edit(tx, &doc.selection, &after, EditKind::Other);
        doc.selection = after.clamp(doc.buffer.len_chars());
    }
    // What came from the session does not go back to it.
    doc.buffer.take_edits();
}

/// Sends the shown file's new edits to the session (`file.edit`); true when there were some.
pub(crate) fn send_edits(app: &mut App) -> bool {
    if let Some(sent) = crate::suggest::send(app) {
        return sent;
    }
    let Some(path) = app.editor.path.clone() else { return false };
    let edits = app.editor.source.doc.buffer.take_edits();
    let mut sent = false;
    for (tx, origin) in edits.into_iter().filter(|(_, o)| !o.is_foreign()) {
        let list: Vec<serde_json::Value> = tx.changes().iter().map(|c| json!({"start": c.start, "end": c.end, "text": c.text})).collect();
        let params = json!({"path": path.as_str(), "edits": list, "origin": origin.name()});
        if let Some(r) = app.run("file.edit", params).and_then(|r| r.get("revision").and_then(serde_json::Value::as_u64)) {
            app.editor.mine.push(r);
        }
        sent = true;
    }
    sent
}

/// After the session edited `path` (an edit the user asked for, such as made LaTeX inserted):
/// the editor takes the edit now, undoable, and selects characters `start..end` of the file.
pub(crate) fn select_after_edit(app: &mut App, path: &ProjectPath, start: usize, end: usize) {
    if app.editor.path.as_ref() != Some(path) {
        return;
    }
    let current = app.session.current().cloned();
    app.editor.sync(current.as_ref(), &app.session);
    let (from, to) = (crate::suggest::to_view(app, path, start), crate::suggest::to_view_end(app, path, end));
    let len = app.editor.source.doc.buffer.len_chars();
    app.editor.source.select(from.min(len), to.min(len));
    app.editor.source.reveal = true;
    app.editor.focus = true;
}

/// Whether an image can go into the shown file now ([`insert_image`]), or why not.
pub(crate) fn can_insert(app: &App) -> Result<(), String> {
    if app.editor.suggest {
        return Err(tl!("Images go into the file itself: stop suggesting to insert one.").to_owned());
    }
    match app.editor.path.as_ref().filter(|p| app.session.current() == Some(*p)) {
        Some(p) if app.session.project().text(p).is_ok() => Ok(()),
        _ => Err(tl!("Open a text file in the editor to insert the image into.").to_owned()),
    }
}

/// Puts `image` into the shown file at `at` (characters), or at the caret, with
/// `file.insert_image`; says on the status line when the main document does not load graphicx.
pub(crate) fn insert_image(app: &mut App, image: &ProjectPath, at: Option<usize>) {
    if let Err(reason) = can_insert(app) {
        app.view.status = Some((reason, true));
        return;
    }
    let Some(path) = app.editor.path.clone() else { return };
    // The text the position is in must be the session's.
    send_edits(app);
    let at = at.unwrap_or_else(|| app.editor.source.caret()).min(app.editor.source.doc.buffer.len_chars());
    let params = json!({"path": path.as_str(), "image": image.as_str(), "at": at});
    let Some(r) = app.run("file.insert_image", params) else { return };
    let main = r.get("main").and_then(serde_json::Value::as_str).unwrap_or_default();
    let (message, warn) = if r.get("graphicx").and_then(serde_json::Value::as_bool) == Some(false) {
        (tl!("Inserted {image}, but {main} does not load graphicx: add \\usepackage{graphicx} to its preamble.").replace("{main}", main), true)
    } else {
        (tl!("Inserted {image} in {file}.").replace("{file}", path.file_name()), false)
    };
    app.view.status = Some((message.replace("{image}", image.file_name()), warn));
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
        (None, Some((m, error, _))) => (crate::i18n::msg(m), if *error { t.error } else { t.text }),
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
        if send_edits(app) {
            app.view.last_edit = Some(ui.ctx().input(|i| i.time));
        }
        app.editor.sync(current.as_ref(), &app.session);
        if let Some(path) = &current {
            crate::suggest::refresh(app, path);
        }
        let is_text = current.as_ref().is_some_and(|p| matches!(app.session.project().get(p), Some(Contents::Text(_))));
        let as_image = current.as_ref().is_some_and(|p| crate::figure_view::is_svg(p) && !app.figure.as_text.contains(p));
        let formatted = current.as_ref().is_some_and(|p| app.editor.formatted.shows(p));
        if is_text && !as_image {
            toolbar(app, ui);
            widgets::hairline(ui, &t);
            if app.editor.suggest && !formatted {
                crate::suggest::banner(app, ui);
                widgets::hairline(ui, &t);
            }
            if !formatted {
                crate::find_bar::show(app, ui);
            }
        }
        match current {
            None => empty(ui, &t, "file-text", tl!("No file is open"), tl!("Choose a file in the project files to edit it.")),
            Some(path) if crate::figure_view::is_svg(&path) && !app.figure.as_text.contains(&path) => crate::figure_view::show(app, ui, &path),
            Some(path) if !is_text && crate::figure_view::is_figure(&path) => crate::figure_view::show(app, ui, &path),
            Some(path) if !is_text => {
                let bytes = app.session.project().get(&path).map_or(0, Contents::len);
                let detail = tl!("{name} is not a text file ({size} bytes); it cannot be edited here.")
                    .replace("{name}", path.file_name())
                    .replace("{size}", &bytes.to_string());
                empty(ui, &t, "image", tl!("Binary file"), &detail);
            }
            Some(path) => {
                if crate::figure_view::is_svg(&path) {
                    crate::figure_view::svg_bar(app, ui, &path);
                }
                if formatted {
                    crate::formatted::show(app, ui, &path);
                } else {
                    source(app, ui, &path);
                }
            }
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
        "edit.chapter" => wrap("\\chapter{", "}", "Title"),
        "edit.section" => wrap("\\section{", "}", "Title"),
        "edit.subsection" => wrap("\\subsection{", "}", "Title"),
        "edit.subsubsection" => wrap("\\subsubsection{", "}", "Title"),
        "edit.paragraph" => wrap("\\paragraph{", "}", "Title"),
        "edit.bold" => wrap("\\textbf{", "}", "text"),
        "edit.italic" => wrap("\\textit{", "}", "text"),
        "edit.code" => wrap("\\texttt{", "}", "code"),
        "edit.footnote" => wrap("\\footnote{", "}", "text"),
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

/// Runs an `edit.*` command with its parameters: `{key}` for a citation or a cross-reference, `{url,
/// text?}` for a link, `{text}` for a footnote, `{latex}` for a formula, `{image}` for a figure.
pub(crate) fn edit_with(app: &mut App, id: &str, params: &serde_json::Value) -> bool {
    let shown_text = app.session.current().is_some_and(|p| matches!(app.session.project().get(p), Some(Contents::Text(_))));
    if !shown_text {
        return false;
    }
    // The formatted view makes its own edits; the others show the source they change.
    if let Some(done) = crate::formatted_edit::command(app, id, params) {
        return done;
    }
    if let Some(path) = app.session.current().cloned() {
        app.editor.formatted.set(&path, false);
    }
    // The paragraph styles are the document's sectioning levels.
    let heading = match id {
        "edit.heading_1" => Some(0),
        "edit.heading_2" => Some(1),
        "edit.heading_3" => Some(2),
        _ => None,
    };
    let resolved;
    let id = match heading {
        Some(level) => {
            resolved = format!("edit.{}", crate::formatted_bar::heading_commands(app)[level]);
            resolved.as_str()
        }
        None => id,
    };
    if let Some(done) = structure_in_source(app, id, params) {
        return done;
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
        "edit.quick_fix" => {
            if !source.quick_fix() {
                app.view.status = Some((tl!("There is no problem on this line.").to_owned(), false));
            }
            app.editor.focus = true;
            return true;
        }
        "edit.comment" => {
            crate::review::start(app);
            // As after a Vim operator: the selection was used, Visual mode ends.
            app.editor.source.end_visual();
            return true;
        }
        "edit.cut" | "edit.copy" => {
            let ctx = app.ctx.clone();
            if !source.copy(&ctx, id == "edit.cut") {
                app.view.status = Some((tl!("Select some text first.").to_owned(), false));
            }
            app.editor.focus = true;
            return true;
        }
        "edit.paste" => {
            if let Err(reason) = crate::ui_cmd::enabled(app, id) {
                app.view.status = Some((reason, false));
                return true;
            }
            // The window reads the clipboard and sends its text to the editor, which has the
            // keyboard again by then.
            let now = app.ctx.input(|i| i.time);
            app.editor.source.ask_paste(now);
            app.ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            app.editor.focus = true;
            return true;
        }
        "edit.suggest_mode" => {
            crate::suggest::toggle(app);
            return true;
        }
        "edit.accept_all" | "edit.reject_all" => {
            let command = if id == "edit.accept_all" { "review.accept_all" } else { "review.reject_all" };
            let Some(path) = app.session.current().cloned() else { return false };
            if let Some(r) = app.run(command, json!({"path": path.as_str()})) {
                let (n, outdated) = (r["accepted"].as_u64().or(r["rejected"].as_u64()).unwrap_or(0), r["outdated"].as_array().map_or(0, Vec::len));
                let mut said = if id == "edit.accept_all" {
                    crate::i18n::trn(n as usize, "{n} suggestion accepted", "{n} suggestions accepted")
                } else {
                    crate::i18n::trn(n as usize, "{n} suggestion rejected", "{n} suggestions rejected")
                };
                if outdated > 0 {
                    said.push_str(&crate::i18n::trn(
                        outdated,
                        "; {n} overlapped another and is outdated",
                        "; {n} overlapped others and are outdated",
                    ));
                }
                app.view.status = Some((said, outdated > 0));
            }
            return true;
        }
        "edit.next_comment" | "edit.previous_comment" => {
            crate::review::step(app, id == "edit.next_comment");
            return true;
        }
        "edit.fold" | "edit.unfold" | "edit.fold_all" | "edit.unfold_all" => {
            let action = match id {
                "edit.fold" => texcraft_vim::Fold::Close,
                "edit.unfold" => texcraft_vim::Fold::Open,
                "edit.fold_all" => texcraft_vim::Fold::CloseAll,
                _ => texcraft_vim::Fold::OpenAll,
            };
            let said = source.fold(action);
            app.view.status = Some((said, false));
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

/// The structural commands and those with parameters, on the source at the caret: what the
/// formatted view does, made in the source. `None` for the other commands.
fn structure_in_source(app: &mut App, id: &str, params: &serde_json::Value) -> Option<bool> {
    use texcraft_latex::edit;
    let text = app.editor.source.doc.buffer.text();
    let at = crate::formatted_edit::caret_byte(app);
    let param = |k: &str| params.get(k).and_then(serde_json::Value::as_str).map(str::to_owned);
    let made = match id {
        "edit.body" => match edit::set_body(&text, at) {
            Some(changes) => Ok((edit::map_offset(&changes, at), changes)),
            None => Err(tl!("The caret is in body text already.")),
        },
        "edit.indent" | "edit.outdent" => {
            let e = if id == "edit.indent" { edit::indent(&text, at) } else { edit::outdent(&text, at) };
            match e {
                Some(e) => Ok((e.caret, e.changes)),
                None if id == "edit.indent" => Err(tl!("Put the caret in a list item after the first to indent it.")),
                None => Err(tl!("Put the caret in a list item to outdent it.")),
            }
        }
        _ => {
            let inserted = match (id, param("key"), param("url"), param("text"), param("latex")) {
                ("edit.cite", Some(key), ..) => format!("\\cite{{{key}}}"),
                ("edit.ref", Some(key), ..) => format!("\\{}{{{key}}}", crate::formatted_edit::ref_command(app)),
                ("edit.link", _, Some(url), text, _) => match text.filter(|t| !t.is_empty()) {
                    Some(t) => format!("\\href{{{url}}}{{{}}}", edit::escape(&t)),
                    None => format!("\\url{{{url}}}"),
                },
                ("edit.footnote", _, _, Some(t), _) => format!("\\footnote{{{}}}", edit::escape(&t)),
                ("edit.math_inline", .., Some(m)) => format!("\\({m}\\)"),
                ("edit.math_display", .., Some(m)) => format!("\\[\n  {m}\n\\]"),
                ("edit.equation", .., Some(m)) => format!("\\begin{{equation}}\n  {m}\n\\end{{equation}}"),
                _ => return None,
            };
            let sel = app.editor.source.doc.selection.primary();
            let (a, b) = (sel.start(), sel.end());
            let b0 = app.editor.source.doc.buffer.char_to_byte(a);
            let b1 = app.editor.source.doc.buffer.char_to_byte(b);
            Ok((b0 + inserted.len(), vec![edit::Change { range: b0..b1, text: inserted }]))
        }
    };
    match made {
        Ok((caret, changes)) => {
            let source = &mut app.editor.source;
            let doc = &mut source.doc;
            let list = changes
                .iter()
                .map(|c| texcraft_buffer::Change {
                    start: doc.buffer.byte_to_char(c.range.start),
                    end: doc.buffer.byte_to_char(c.range.end),
                    text: c.text.clone(),
                })
                .collect();
            let new = edit::apply(&text, &changes);
            let caret = new.get(..caret).map_or(0, |s| s.chars().count());
            let before = doc.selection.clone();
            if let Ok(tx) = Transaction::new(list)
                && doc.buffer.edit(tx, &before, &Selection::single(caret, caret), EditKind::Other).is_ok()
            {
                doc.buffer.seal();
                source.select(caret, caret);
                source.reveal = true;
            }
        }
        Err(why) => app.view.status = Some((why.to_owned(), false)),
    }
    app.editor.focus = true;
    Some(true)
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
            (tn!("Section"), "edit.section"),
            (tn!("Subsection"), "edit.subsection"),
            (tn!("Subsubsection"), "edit.subsubsection"),
            (tn!("Paragraph Heading"), "edit.paragraph"),
            (tn!("Bold"), "edit.bold"),
            (tn!("Italic"), "edit.italic"),
        ],
    ),
    (
        160.0,
        &[
            (tn!("Inline Formula"), "edit.math_inline"),
            (tn!("Displayed Formula"), "edit.math_display"),
            (tn!("Numbered Equation"), "edit.equation"),
            (tn!("Symbols…"), "edit.symbols"),
            (tn!("Cross-Reference"), "edit.ref"),
            (tn!("Citation"), "edit.cite"),
            (tn!("Link"), "edit.link"),
        ],
    ),
    (
        130.0,
        &[
            (tn!("Figure"), "edit.figure"),
            (tn!("Table"), "edit.table"),
            (tn!("Bulleted List"), "edit.itemize"),
            (tn!("Numbered List"), "edit.enumerate"),
        ],
    ),
];

/// The label of command `id` (`label`, its English), translated: a toolbar entry is named as the
/// command is in the menus and the shortcuts list.
fn cmd(id: &str, label: &'static str) -> &'static str {
    crate::i18n::tid(id, label)
}

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
            crate::widgets::keyboard_menu(ui, &heading);
            egui::Popup::menu(&heading).show(|ui| {
                ui.set_min_width(180.0);
                for (label, id, size) in [
                    (cmd("edit.section", tn!("Section")), "edit.section", 16.0),
                    (cmd("edit.subsection", tn!("Subsection")), "edit.subsection", 14.5),
                    (cmd("edit.subsubsection", tn!("Subsubsection")), "edit.subsubsection", 13.5),
                    (cmd("edit.paragraph", tn!("Paragraph Heading")), "edit.paragraph", 13.0),
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
            let math = widgets::icon_button(ui, t, "sigma", tl!("Formula"), true);
            crate::widgets::keyboard_menu(ui, &math);
            egui::Popup::menu(&math).show(|ui| {
                ui.set_min_width(180.0);
                for (label, id) in [
                    (cmd("edit.math_inline", tn!("Inline Formula")), "edit.math_inline"),
                    (cmd("edit.math_display", tn!("Displayed Formula")), "edit.math_display"),
                    (cmd("edit.equation", tn!("Numbered Equation")), "edit.equation"),
                ] {
                    if menu_entry(ui, label) {
                        *chosen = Some(id);
                    }
                }
            });
            tool(ui, t, ("omega", tl!("Symbols"), "edit.symbols"), chosen);
            tool(ui, t, ("tag", cmd("edit.ref", tn!("Cross-Reference")), "edit.ref"), chosen);
            tool(ui, t, ("book-open", tl!("Citation"), "edit.cite"), chosen);
            tool(ui, t, ("link", tl!("Link"), "edit.link"), chosen);
        }
        _ => {
            tool(ui, t, ("image", tl!("Figure"), "edit.figure"), chosen);
            tool(ui, t, ("table", tl!("Table"), "edit.table"), chosen);
            tool(ui, t, ("list", cmd("edit.itemize", tn!("Bulleted List")), "edit.itemize"), chosen);
            tool(ui, t, ("list-ordered", cmd("edit.enumerate", tn!("Numbered List")), "edit.enumerate"), chosen);
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
        let path = app.session.current().cloned();
        let can = path.as_ref().is_some_and(crate::formatted::available);
        let on = path.as_ref().is_some_and(|p| app.editor.formatted.shows(p));
        let only_tex = tl!("Only LaTeX files (.tex) have a formatted view.");
        let options = [(tl!("Source"), None), (tl!("Formatted"), if can { None } else { Some(only_tex) })];
        if let (Some(choice), Some(path)) = (widgets::segmented(ui, &t, "mode", &options, usize::from(on)), path.as_ref()) {
            app.editor.formatted.set(path, choice == 1);
            if choice == 0 {
                app.editor.focus = true;
            }
        }
        // What the right end needs: Find and the More menu.
        let right_end = 2.0 * t.toolbar_height + 4.0 * t.unit;
        if !on {
            ui.add_space(t.unit);
            // Its label only where it fits with the right end: long words (German, French) leave
            // the pencil, still named and explained on hover.
            let room = ui.max_rect().right() - ui.cursor().left() - right_end;
            if crate::suggest::switch(app, ui, room) {
                chosen = Some("edit.suggest_mode");
            }
        }
        if on {
            crate::formatted_bar::show(app, ui, &mut chosen);
            return;
        }
        // Room for Find and the More menu on the right.
        let mut room = ui.max_rect().right() - ui.cursor().left() - right_end;
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
                crate::widgets::keyboard_menu(ui, &more);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(180.0);
                    for (n, i) in hidden.iter().enumerate() {
                        if n > 0 {
                            ui.separator();
                        }
                        for (label, id) in GROUPS[*i].1 {
                            if menu_entry(ui, cmd(id, label)) {
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
/// The problem popup's AI part: for a build message, when a model is connected, its explanation.
fn explanation_in_popup(app: &mut App, path: &ProjectPath) {
    let offered = crate::ai::offered(app);
    let blocked = crate::ai::explain_blocked(app);
    let Some(popup) = app.editor.source.popup.as_mut() else { return };
    popup.ai = None;
    if !(offered && popup.problem.build) {
        return;
    }
    let line = u32::try_from(popup.problem.line + 1).ok();
    let message = app.session.last_outcome().and_then(|o| {
        o.diagnostics.iter().find(|d| d.file.as_deref() == Some(path.as_str()) && d.line == line && d.message == popup.problem.message).cloned()
    });
    if let Some(d) = message {
        popup.ai = Some((app.session.ai_explanation(&d).cloned(), blocked));
    }
}

/// The problems in the shown file's text: the language server's hints, and the last build's
/// errors and warnings, placed on the word at their column (or the line's text).
fn problems(app: &mut App, path: &ProjectPath, ctx: &egui::Context) -> Vec<crate::source::Problem> {
    let b = &app.editor.source.doc.buffer;
    let last = b.len_lines().saturating_sub(1);
    let at = |line: usize, col: usize| {
        let line = line.min(last);
        (b.line_to_char(line) + col).min(b.line_end(line))
    };
    let mut out: Vec<crate::source::Problem> = app
        .session
        .lint(path)
        .iter()
        .map(|h| crate::source::Problem {
            start: at(h.line, h.column),
            end: at(h.end_line, h.end_column),
            line: h.line,
            severity: h.severity,
            message: h.message.clone(),
            help: Vec::new(),
            fixes: Vec::new(),
            build: false,
            spelling: false,
            grammar: None,
        })
        .collect();
    let b = &app.editor.source.doc.buffer;
    if let Some(outcome) = app.session.last_outcome() {
        for d in outcome.diagnostics.iter().filter(|d| d.file.as_deref() == Some(path.as_str()) && d.severity >= Severity::Warning) {
            let Some(line) = d.line.map(|l| (l.saturating_sub(1) as usize).min(last)) else { continue };
            let text: Vec<char> = b.line(line).chars().collect();
            let span = d
                .column
                .zip(d.end_column)
                .map(|(c, e)| ((c.saturating_sub(1) as usize).min(text.len()), (e.saturating_sub(1) as usize).min(text.len())));
            let (start, end) = match (span, d.column.map(|c| (c.saturating_sub(1) as usize).min(text.len()))) {
                (Some((c, e)), _) if e > c => (c, e),
                (_, Some(c)) => {
                    let end = (c + 1..=text.len()).find(|i| *i == text.len() || text[*i].is_whitespace()).unwrap_or(text.len());
                    (c, end.max(c + 1).min(text.len().max(c)))
                }
                _ => (text.iter().take_while(|c| c.is_whitespace()).count(), text.len()),
            };
            let base = b.line_to_char(line);
            // The build's fixes: columns from 1 on the message's line.
            let fixes = d
                .fixes
                .iter()
                .enumerate()
                .map(|(i, f)| texcraft_engine::Fix {
                    title: if let Some(title) = &f.title {
                        title.clone()
                    } else if f.replacement.is_empty() {
                        tl!("Remove it").to_owned()
                    } else {
                        tl!("Change to “{text}”").replace("{text}", &f.replacement)
                    },
                    edits: vec![texcraft_engine::TextEdit {
                        line,
                        column: f.column.saturating_sub(1) as usize,
                        end_line: line,
                        end_column: f.end_column.saturating_sub(1) as usize,
                        text: f.replacement.clone(),
                    }],
                    preferred: i == 0,
                    problem: Some(d.message.clone()),
                })
                .collect();
            out.push(crate::source::Problem {
                start: base + start,
                end: base + end,
                line,
                severity: d.severity,
                message: d.message.clone(),
                help: d.help.clone(),
                fixes,
                build: true,
                spelling: false,
                grammar: None,
            });
        }
    }
    out.extend(crate::spell::problems(app, path));
    out.extend(crate::grammar::problems(app, path, ctx));
    out.sort_by_key(|p| p.start);
    out
}

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
    // Where TeX noticed a problem that starts elsewhere: marked too, saying where it starts.
    for d in outcome.diagnostics.iter().filter(|d| d.severity >= Severity::Warning) {
        let (Some(noticed), Some(file)) = (&d.noticed, &d.file) else { continue };
        let Some(line) = noticed.line.filter(|_| noticed.file == path.as_str()).map(|l| l.saturating_sub(1)) else { continue };
        if out.get(&line).is_none_or(|(s, _)| *s < d.severity) {
            let start = d.line.map_or_else(|| file.clone(), |l| format!("{file}:{l}"));
            out.insert(line, (d.severity, tf!("{message} (it starts at {place})", message = d.message, place = start)));
        }
    }
    out
}

/// The text's context menu, opened by a right-click or from the keyboard (Shift+F10, the Menu
/// key, which give the menu's first entry the keyboard): the clipboard, Select All, and the
/// commands on the selection or the caret's place. Each entry runs its Edit menu command and
/// shows that command's shortcut.
fn context_menu(app: &mut App, ui: &mut egui::Ui, r: &egui::Response) {
    let t = app.tokens;
    // Opened from the keyboard: whether its first entry is still to get the keyboard.
    let by_key = r.id.with("menu-by-key");
    let mut menu = egui::Popup::context_menu(r);
    if r.has_focus()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10) || i.consume_key(egui::Modifiers::NONE, egui::Key::Menu))
    {
        ui.data_mut(|d| d.insert_temp(by_key, true));
        menu = menu.open_memory(egui::SetOpenCommand::Bool(true));
    }
    let keyed = ui.data(|d| d.get_temp::<bool>(by_key));
    // The selection as the menu opened: Add a Comment is on it, whatever the frames until an entry
    // is chosen do to the editor's selection (the Review panel opening, the pointer's press).
    let kept_id = r.id.with("menu-selection");
    if r.secondary_clicked() || keyed == Some(true) {
        let p = app.editor.source.doc.selection.primary();
        ui.data_mut(|d| d.insert_temp(kept_id, (p.start(), p.end())));
    }
    let kept = ui.data(|d| d.get_temp::<(usize, usize)>(kept_id)).filter(|(start, end)| start < end);
    if keyed.is_some() {
        let source = &app.editor.source;
        let at = source.screen_rect(source.caret()).map_or(r.rect.left_top(), |c| c.left_bottom());
        menu = menu.at_position(at);
    }
    let mut entries = vec![
        Some(("edit.cut", tl!("Cut"))),
        Some(("edit.copy", tl!("Copy"))),
        Some(("edit.paste", tl!("Paste"))),
        Some(("edit.select_all", tl!("Select All"))),
        None,
    ];
    if app.editor.source.selected().is_some() || kept.is_some() {
        entries.push(Some(("edit.comment", tl!("Add a Comment"))));
    }
    if app.editor.source.on_problem() {
        entries.push(Some(("edit.quick_fix", tl!("Quick Fix"))));
    }
    entries.push(Some(("view.show_in_pdf", tl!("Show in PDF"))));
    if crate::ai::offered(app) {
        entries.push(Some(("edit.ai_grammar", tl!("Check Grammar with AI"))));
    }
    // On a grammar problem: its message, its replacements, Ignore and Turn Off This Rule.
    let grammar = app.editor.source.grammar_at_caret().cloned();
    // On a misspelled word: its spellings, Add to Dictionary and Ignore come first.
    let misspelled = app.editor.source.misspelling_at_caret().map(|p| app.editor.source.doc.buffer.slice(p.start, p.end));
    let spelling = misspelled.map(|w| crate::spell::suggestions(app, &w));
    let mut chosen = None;
    let mut spell_choice: Option<(&'static str, serde_json::Value)> = None;
    let rewrite_offered = app.editor.source.selected().is_some() && crate::ai::offered(app);
    let mut rewrite = None;
    let shown = menu.show(|ui| {
        ui.set_min_width(220.0);
        let mut focus = keyed == Some(true);
        if let Some(offered) = &spelling {
            if offered.is_empty() {
                ui.label(egui::RichText::new(tl!("No spellings to offer")).font(t.small()).color(t.text_muted));
            }
            let changes = offered.iter().map(|s| ("edit.spelling.replace", tl!("Change to “{text}”").replace("{text}", s), json!({"text": s})));
            let others = [
                ("edit.spelling.add", tl!("Add to Dictionary").to_owned(), json!({})),
                ("edit.spelling.ignore", tl!("Ignore").to_owned(), json!({})),
            ];
            for (id, label, params) in changes.chain(others) {
                let icon = if id == "edit.spelling.replace" { Some("pencil") } else { crate::top_bar::icon_for(id) };
                let item = crate::top_bar::item_response(ui, &t, icon, &label, None, Ok(()));
                if std::mem::take(&mut focus) {
                    item.request_focus();
                }
                if item.clicked() {
                    spell_choice = Some((id, params));
                }
            }
            ui.separator();
        }
        if let Some(problem) = &grammar {
            ui.label(egui::RichText::new(&problem.message).font(t.small()).color(t.text_muted));
            let changes = problem.fixes.iter().map(|f| {
                let text = f.edits.first().map(|e| e.text.clone()).unwrap_or_default();
                ("edit.grammar.replace", f.title.clone(), json!({"text": text}))
            });
            let mut others = vec![("edit.grammar.ignore", tl!("Ignore").to_owned(), json!({}))];
            if problem.grammar.as_ref().is_some_and(|g| g.rule.is_some()) {
                others.push(("edit.grammar.rule_off", tl!("Turn Off This Rule").to_owned(), json!({})));
            }
            for (id, label, params) in changes.chain(others) {
                let icon = if id == "edit.grammar.replace" { Some("pencil") } else { crate::top_bar::icon_for(id) };
                let item = crate::top_bar::item_response(ui, &t, icon, &label, None, Ok(()));
                if std::mem::take(&mut focus) {
                    item.request_focus();
                }
                if item.clicked() {
                    spell_choice = Some((id, params));
                }
            }
            ui.separator();
        }
        for entry in &entries {
            let Some((id, label)) = *entry else {
                ui.separator();
                continue;
            };
            let enabled = crate::ui_cmd::enabled(app, id);
            let can = enabled.is_ok();
            let item = crate::top_bar::item_response(ui, &t, crate::top_bar::icon_for(id), label, app.keys.shortcut_text(id), enabled);
            if focus && can {
                item.request_focus();
                focus = false;
            }
            if item.clicked() {
                chosen = Some(id);
            }
        }
        if rewrite_offered {
            ui.separator();
            ui.menu_image_text_button(crate::icons::image(crate::ai::ICON, 14.0, t.text_muted), tl!("Rewrite with AI"), |ui| {
                ui.set_min_width(220.0);
                rewrite = crate::ai::rewrite_menu(app, ui);
            });
        }
        if chosen.is_some() || spell_choice.is_some() || rewrite.is_some() {
            ui.close();
        }
    });
    if keyed == Some(true) {
        ui.data_mut(|d| d.insert_temp(by_key, false));
    }
    if shown.is_none() || chosen.is_some() || spell_choice.is_some() || rewrite.is_some() {
        ui.data_mut(|d| d.remove::<(usize, usize)>(kept_id));
    }
    if let Some(choice) = rewrite {
        ui.data_mut(|d| d.remove::<bool>(by_key));
        app.editor.focus = true;
        crate::ai::run_rewrite_choice(app, &choice);
        return;
    }
    if shown.is_none() || chosen.is_some() || spell_choice.is_some() {
        ui.data_mut(|d| d.remove::<bool>(by_key));
        // Closed from the keyboard without a choice: the keyboard goes back to the text.
        if keyed.is_some() && shown.is_none() {
            app.editor.focus = true;
        }
    }
    if let Some((id, params)) = spell_choice {
        app.editor.focus = true;
        crate::ui_cmd::run_with(app, id, &params);
    }
    if let Some(id) = chosen {
        if id != "edit.comment" {
            app.editor.focus = true;
        } else if let Some((start, end)) = kept
            && app.editor.source.selected().is_none()
        {
            app.editor.source.select(start, end);
        }
        crate::ui_cmd::run(app, id);
    }
}

/// Vim's `:w` (and `:wq`, `:x`, `ZZ`): saves the project as File › Save does, then compiles, as
/// Compile does, so what was written is typeset at once. A save that failed builds nothing: its
/// error stays on the status line. Without a folder yet, Save As asks for one and the build runs.
fn write_and_compile(app: &mut App) {
    let saved = if app.session.dir().is_some() {
        app.run("project.save", json!({})).is_some()
    } else {
        crate::ui_cmd::run(app, "project.save");
        true
    };
    if saved {
        app.run("build.compile", json!({}));
    }
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
            app.editor.source.flash_line(line.saturating_sub(1) as usize, ui.input(|i| i.time));
            app.editor.focus = true;
        } else {
            app.view.goto = Some((goto_path, line));
        }
    }
    // While a dialog is open, and the frame after it closes, egui keeps the keyboard in the
    // dialog's layer and would take it back from the editor: the request waits for the frame the
    // editor can have it (closing a dialog draws that frame).
    if app.editor.focus && ui.memory(|m| m.allows_interaction(ui.layer_id())) {
        app.editor.focus = false;
        ui.memory_mut(|m| m.request_focus(id));
    }
    app.editor.source.problems = problems(app, path, ui.ctx());
    explanation_in_popup(app, path);
    app.editor.source.set_vim(app.prefs.vim);
    app.editor.source.auto_close = app.prefs.auto_close;
    vim_line(app, ui);
    let name = format!("{} {}", path.file_name(), tl!("source"));
    app.editor.source.accept_drop = egui::DragAndDrop::has_payload_of_type::<crate::side::ImageDrag>(ui.ctx());
    app.editor.caret_before = app.editor.source.caret();
    let out = crate::review::with_margin(app, ui, path, |app, ui| app.editor.source.show(ui, &t, id, &markers, &name, "  "));
    crate::review::follow_caret(app, path);
    if let Some(response) = &out.response {
        context_menu(app, ui, response);
    }
    app.view.cursor = Some((out.caret.0 as u32 + 1, out.caret.1 as u32 + 1));
    // Typing, and edits made outside the widget (the toolbar, the Insert menu).
    let edited = send_edits(app);
    if edited {
        app.view.last_edit = Some(ui.ctx().input(|i| i.time));
    }
    crate::suggest::refresh(app, path);
    let focused = ui.memory(|m| m.has_focus(id));
    crate::ai::typing(app, ui.ctx(), path, edited, out.ghost == Some(false), focused);
    // Completion, hover and definitions are of the file's lines and columns: not of the text
    // shown with suggestions in it.
    let out = if crate::suggest::view(app, path).is_some_and(|v| !v.is_plain()) {
        crate::source::Output { complete_at: None, hover_at: None, definition_at: None, ..out }
    } else {
        out
    };
    // After the text went to the session, so the language server sees what was typed.
    if let Some((line, column)) = out.complete_at {
        let items = app.session.complete(path, line, column);
        app.editor.source.offer(items);
        ui.ctx().request_repaint();
    }
    if let Some((line, column)) = out.hover_at {
        let text = app.session.hover(path, line, column).unwrap_or_default();
        if let Some((_, slot)) = app.editor.source.hover.as_mut() {
            *slot = text;
        }
    }
    if let Some((line, column)) = out.definition_at {
        match app.session.definition(path, line, column) {
            Some((file, l, c)) if &file == path => {
                let b = &app.editor.source.doc.buffer;
                let at = b.line_to_char(l) + c;
                app.editor.source.select(at, at);
                app.editor.source.doc.buffer.seal();
            }
            Some((file, l, _)) => {
                let _ = app.run("file.open", json!({"path": file.as_str()}));
                app.view.goto = Some((file, l as u32 + 1));
            }
            // Nothing to define here: Ctrl+click shows the line's place in the PDF.
            None => crate::preview::show_in_pdf(app, path, line as u32 + 1),
        }
    }
    let grammar_fixes = out.fixes_for.and(app.editor.source.popup.as_ref()).filter(|p| p.problem.grammar.is_some()).map(|p| p.problem.fixes.clone());
    if let Some(fixes) = grammar_fixes {
        if let Some(popup) = app.editor.source.popup.as_mut() {
            popup.fixes = Some(fixes);
        }
    } else if let Some(problem) = out.fixes_for.and(app.editor.source.popup.as_ref()).map(|p| p.problem.clone()).filter(|p| p.spelling) {
        let fixes = crate::spell::fixes(app, &problem);
        if let Some(popup) = app.editor.source.popup.as_mut() {
            popup.fixes = Some(fixes);
        }
    } else if let Some(line) = out.fixes_for {
        let message = app.editor.source.popup.as_ref().map(|p| p.problem.message.clone());
        let fixes: Vec<texcraft_engine::Fix> =
            app.session.fixes(path, line).into_iter().filter(|f| f.problem.is_none() || f.problem == message).collect();
        // "Fix all" only when there is a fix of the problem itself.
        let own = fixes.iter().any(|f| f.problem.is_some());
        let fixes = fixes.into_iter().filter(|f| own && (f.problem.is_some() || f.edits.len() > 1)).collect();
        if let Some(popup) = app.editor.source.popup.as_mut() {
            popup.fixes = Some(fixes);
        }
    }
    if let Some(at) = out.dropped_at
        && let Some(image) = egui::DragAndDrop::take_payload::<crate::side::ImageDrag>(ui.ctx())
    {
        insert_image(app, &image.0, Some(at));
        ui.memory_mut(|m| m.request_focus(id));
    }
    if let Some(said) = out.announce {
        app.view.status = Some((said, false));
    }
    if let Some(fix) = out.apply_fix {
        app.editor.source.apply_fix(&fix);
    }
    if let Some(line) = out.explain {
        crate::ai::explain(app, json!({"path": path.as_str(), "line": line + 1}));
    }
    if out.cancel_ai {
        app.run("ai.cancel", json!({}));
    }
    for effect in out.effects {
        match effect {
            Effect::Save | Effect::SaveAll => write_and_compile(app),
            Effect::Close { .. } => {
                let _ = app.run("file.close", json!({"path": path.as_str()}));
            }
            Effect::Open(name) => {
                let _ = app.run("file.open", json!({"path": name}));
            }
            // Vim's messages show on its line, at the bottom of the editor.
            Effect::Message(m) => app.editor.vim_message = Some((m, false, ui.ctx().input(|i| i.time))),
            Effect::Error(m) => app.editor.vim_message = Some((m, true, ui.ctx().input(|i| i.time))),
            Effect::Comment { start, end, text } => {
                let (start, end) = (crate::suggest::to_source(app, path, start), crate::suggest::to_source(app, path, end));
                crate::review::from_vim(app, path, start, end, &text)
            }
            Effect::Suggest(on) => {
                if app.editor.suggest != on {
                    crate::suggest::toggle(app);
                }
            }
            Effect::Copy(_) | Effect::Scroll(_) | Effect::Fold(_) => {}
        }
    }
}
