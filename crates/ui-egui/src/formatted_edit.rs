//! Editing in the formatted view (docs/ROADMAP.md, F3): the caret and selection in its text, moved
//! by the keyboard and the pointer, and the keys' edits, each a minimal edit of the source
//! ([`texcraft_latex::edit`]).
//!
//! The view edits the source editor's buffer, so its edits go to the session as the source
//! editor's do (`file.edit`, with their origin, on the editor's next frame) and undo is shared
//! with source mode. The caret and selection are the buffer's, in the source: each frame finds
//! them in the lines drawn, so they survive the view being made again after an edit.
//!
//! - **Text** runs (paragraphs, headings, list items, captions) are caret stops character by
//!   character. Atoms (chips, formulas, what is shown as source) are one stop on each side:
//!   Backspace or Delete next to one selects it, a second press deletes its whole source.
//! - **Enter** ends a paragraph, opens one after a heading, and starts an item in a list. Typed and
//!   pasted text is escaped. Bold, Italic, the heading levels and the lists of the toolbar act on
//!   the selection's source.
//! - **Keys** are the standard ones, Vim's mode or not (the view says so). In Suggest mode the
//!   text is read-only here: suggestions are made in the source.
//! - **Accessibility:** the view is an editable multi-line text node with its value, caret and
//!   selection, one text run per line.

use craft_ui::ime::{Composer, Edit};
use egui::text::CCursor;
use egui::{Rect, Sense, pos2, vec2};
use texcraft_buffer::{EditKind, Selection, Transaction};
use texcraft_engine::project::ProjectPath;
use texcraft_latex::edit::{self, Change, Doc, Pos};

use crate::App;
use crate::formatted::{DrawnLine, span_rects, text_id};

/// The formatted view's keyboard and pointer state.
#[derive(Default)]
pub(crate) struct Editing {
    composer: Composer,
    /// Edits after one that changed the text in the same frame: made on the next frame's view.
    pending: Vec<Edit>,
    /// A toolbar or menu command for the next frame's view, with its parameters.
    commands: Vec<(String, serde_json::Value)>,
    /// The caret's line, to tell lines apart that meet at one source offset.
    caret_line: Option<usize>,
    /// The horizontal position kept by Up and Down, on screen.
    want_x: Option<f32>,
    dragging: bool,
    /// Bring the caret into view next frame.
    pub reveal: bool,
    /// Take the keyboard next frame.
    pub focus: bool,
    /// Give the keyboard to the toolbar of the table holding the caret, next frame.
    pub focus_toolbar: bool,
    /// A table's toolbar held the keyboard last frame.
    pub toolbar_focused: bool,
    /// The caret was in a table cell last frame: Tab moves between cells.
    in_cell: bool,
    /// The caret was in a list item last frame: Tab and Shift+Tab nest it.
    in_item: bool,
    /// Shift+F10 or the Menu key: open the context menu next frame.
    menu_by_key: bool,
    /// The view asked for the keyboard itself (its caret is where it wants it).
    ours: bool,
    was_focused: bool,
    /// The selection last put in the primary selection (X11, Wayland).
    primary_sent: Option<(Pos, Pos)>,
    /// The lines last drawn and the buffer's version they show, while the find bar is open (it
    /// finds only what they show).
    pub shown: Option<(u64, Doc)>,
}

/// The caret's source byte.
pub(crate) fn caret_byte(app: &App) -> usize {
    let d = &app.editor.source.doc;
    d.buffer.char_to_byte(d.selection.primary().head)
}

/// Before the view is drawn: the keyboard's edits of this frame (and those left from the last).
pub(crate) fn begin(app: &mut App, ctx: &egui::Context, id: egui::Id, focused: bool) -> Vec<Edit> {
    let e = &mut app.editor.formatted.editing;
    let tab = e.in_cell || e.in_item;
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(id, egui::EventFilter { tab, horizontal_arrows: true, vertical_arrows: true, escape: false });
    });
    // Escape in a table cell (which takes the keyboard from the text) goes to the table's toolbar.
    if e.was_focused && !focused && e.in_cell && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        e.focus_toolbar = true;
    }
    if std::mem::take(&mut e.focus) {
        e.ours = true;
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let mut edits = std::mem::take(&mut e.pending);
    edits.extend(e.composer.update(ctx, focused));
    if !focused {
        edits.clear();
    }
    edits
}

/// Runs an `edit.*` command on the formatted view's selection, when the view is shown and the
/// command acts on it; `None` otherwise. Those that need something first (a citation's key, a
/// link's address) ask for it, then run again with it.
pub(crate) fn command(app: &mut App, id: &str, params: &serde_json::Value) -> Option<bool> {
    let path = app.session.current()?.clone();
    if !app.editor.formatted.shows(&path) {
        return None;
    }
    let has = |k: &str| params.get(k).and_then(serde_json::Value::as_str).is_some();
    match id {
        "edit.bold" | "edit.italic" | "edit.code" | "edit.section" | "edit.subsection" | "edit.subsubsection" | "edit.paragraph" | "edit.chapter"
        | "edit.body" | "edit.heading_1" | "edit.heading_2" | "edit.heading_3" | "edit.itemize" | "edit.enumerate" | "edit.indent"
        | "edit.outdent" | "edit.table" | "edit.undo" | "edit.redo" | "edit.select_all" | "edit.cut" | "edit.copy" => {}
        "edit.cite" | "edit.ref" if !has("key") => {
            crate::insert_pick::open(app, if id == "edit.cite" { crate::insert_pick::Kind::Citation } else { crate::insert_pick::Kind::Reference });
            return Some(true);
        }
        "edit.find" | "edit.replace" => {
            // The bar over the view, finding what it shows.
            let old = app.editor.find.take();
            app.editor.find = Some(crate::find_bar::FindBar::open(&app.editor.source, id == "edit.replace", old));
            return Some(true);
        }
        "edit.figure" if !has("image") => {
            crate::insert_pick::open(app, crate::insert_pick::Kind::Image);
            return Some(true);
        }
        "edit.figure" => {
            let image = params["image"].as_str().unwrap_or_default().to_owned();
            app.figure_for_drop(&image);
            app.editor.formatted.editing.focus = true;
            return Some(true);
        }
        "edit.link" if !has("url") => {
            let text = selected_text(app);
            ask(
                app,
                (tl!("Link"), tl!("The address the text links to"), tl!("Make Link")),
                "edit.link",
                "https://".to_owned(),
                move |v| serde_json::json!({"url": v, "text": text}),
            );
            return Some(true);
        }
        "edit.footnote" if !has("text") => {
            ask(
                app,
                (tl!("Footnote"), tl!("The footnote's text"), tl!("Add Footnote")),
                "edit.footnote",
                String::new(),
                |v| serde_json::json!({"text": v}),
            );
            return Some(true);
        }
        "edit.math_inline" | "edit.math_display" | "edit.equation" if !has("latex") => {
            // A field with the formula's typeset preview under it.
            let start = selected_text(app);
            crate::formatted::in_place::ask_formula(app, id, start);
            return Some(true);
        }
        "edit.cite" | "edit.ref" | "edit.link" | "edit.footnote" | "edit.math_inline" | "edit.math_display" | "edit.equation" => {}
        "edit.paste" => {
            if let Err(reason) = crate::ui_cmd::enabled(app, id) {
                app.view.status = Some((reason, false));
                return Some(true);
            }
            // The clipboard's text comes back as a paste, which the view takes with the keyboard.
            app.ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            app.editor.formatted.editing.focus = true;
            return Some(true);
        }
        _ => return None,
    }
    app.editor.formatted.editing.commands.push((id.to_owned(), params.clone()));
    app.editor.formatted.editing.focus = true;
    app.ctx.request_repaint();
    Some(true)
}

/// Asks for one line, then runs `command` with the parameters `params` makes of it.
fn ask(
    app: &mut App,
    (title, label, action): (&'static str, &'static str, &'static str),
    command: &'static str,
    value: String,
    params: impl Fn(&str) -> serde_json::Value + 'static,
) {
    let prompt = crate::dialogs::Prompt { title, label, value, action, command, params: Box::new(params), focus: true };
    app.view.dialog = Some(crate::dialogs::Dialog::Prompt(prompt));
}

/// The source of the selection, when it is all text (a formula's source to start from, a link's
/// text): the characters it shows.
fn selected_text(app: &App) -> String {
    let d = &app.editor.source.doc;
    let r = d.selection.primary();
    d.buffer.slice(r.start(), r.end())
}

/// The cross-reference command: `\cref` where the main document loads cleveref, else `\ref`.
pub(crate) fn ref_command(app: &App) -> &'static str {
    let project = app.session.project();
    let main = project.main().and_then(|m| project.text(m).ok()).unwrap_or_default();
    if texcraft_latex::refs::loads(main, "cleveref") { "cref" } else { "ref" }
}

/// What the frame's edits work on.
struct Frame<'a> {
    text: &'a str,
    doc: Doc,
    lines: &'a [DrawnLine],
    now: f64,
}

impl Frame<'_> {
    fn chars(&self, line: usize) -> Vec<char> {
        self.lines.get(line).map(|d| d.galley.text().chars().collect()).unwrap_or_default()
    }
}

/// After the view is drawn: the pointer and the keys move the caret and edit the text, then the
/// selection and caret are painted and told to assistive technology.
pub(crate) fn finish(
    app: &mut App,
    ui: &mut egui::Ui,
    (path, text): (&ProjectPath, &str),
    lines: &[DrawnLine],
    (painter, response): (&egui::Painter, &egui::Response),
    edits: Vec<Edit>,
) {
    let ctx = ui.ctx().clone();
    let id = text_id(path);
    let focused = ctx.memory(|m| m.has_focus(id));
    let f = Frame { text, doc: Doc { lines: lines.iter().map(|d| d.line.clone()).collect() }, lines, now: ctx.input(|i| i.time) };
    let version = app.editor.source.doc.buffer.version();
    app.editor.formatted.editing.shown = app.editor.find.is_some().then(|| (version, f.doc.clone()));
    // A caret that came from elsewhere (the source) goes to the nearest place the view shows.
    let e = &mut app.editor.formatted.editing;
    let gained = focused && !e.was_focused;
    e.was_focused = focused;
    if gained
        && !std::mem::take(&mut e.ours)
        && let Some((a, h)) = selection(app, &f)
    {
        set_selection(app, &f, a, h);
    }
    pointer(app, &ctx, &f);
    context_menu(app, ui, &f, response, focused);
    let commands = std::mem::take(&mut app.editor.formatted.editing.commands);
    if !commands.is_empty() && !focused {
        // A toolbar button took the keyboard as it was pressed: the text takes it back.
        app.editor.formatted.editing.focus = true;
        ctx.request_repaint();
    }
    for (c, params) in commands {
        if app.editor.source.doc.buffer.version() != version {
            app.editor.formatted.editing.commands.push((c, params));
            continue;
        }
        run_command(app, &ctx, &f, &c, &params);
    }
    for edit in edits {
        if app.editor.source.doc.buffer.version() != version {
            app.editor.formatted.editing.pending.push(edit);
            continue;
        }
        input(app, &ctx, &f, edit);
    }
    if app.editor.source.doc.buffer.version() != version {
        // The lines are of the text before: the caret shows on the view made again next frame.
        app.editor.formatted.editing.reveal = true;
        app.editor.formatted.editing.caret_line = None;
        ctx.request_repaint();
        return;
    }
    let Some((a, h)) = selection(app, &f) else {
        app.editor.formatted.editing.in_cell = false;
        return;
    };
    app.editor.formatted.editing.in_cell = focused && lines.get(h.line).is_some_and(|d| d.cell.is_some());
    let in_item = focused && crate::formatted_bar::here(app).list.is_some();
    app.editor.formatted.editing.in_item = in_item;
    if focused {
        offer_primary(app, &ctx, &f, (a, h));
    }
    paint(app, &ctx, &f, painter, (a, h), focused);
    accessibility(ui, response, path, &f, (a, h));
}

/// The selection's anchor and head in the view.
fn selection(app: &App, f: &Frame<'_>) -> Option<(Pos, Pos)> {
    let d = &app.editor.source.doc;
    let r = d.selection.primary();
    let hint = app.editor.formatted.editing.caret_line;
    let a = f.doc.locate(f.text, d.buffer.char_to_byte(r.anchor), hint)?;
    let h = f.doc.locate(f.text, d.buffer.char_to_byte(r.head), hint)?;
    Some((a, h))
}

fn set_selection(app: &mut App, f: &Frame<'_>, a: Pos, h: Pos) {
    let (Some(ab), Some(hb)) = (f.doc.source(a), f.doc.source(h)) else { return };
    set_bytes(app, (ab, hb), Some(h.line));
}

fn set_bytes(app: &mut App, (a, h): (usize, usize), line: Option<usize>) {
    let d = &mut app.editor.source.doc;
    d.selection = Selection::single(d.buffer.byte_to_char(a), d.buffer.byte_to_char(h));
    d.buffer.seal();
    let e = &mut app.editor.formatted.editing;
    e.caret_line = line;
    e.reveal = true;
}

/// Makes `changes` (bytes of `f.text`) in the buffer as one edit, with the selection `after` (bytes
/// of the text after them).
fn commit(app: &mut App, f: &Frame<'_>, changes: &[Change], after: (usize, usize), kind: EditKind) -> bool {
    if !editable(app) {
        return false;
    }
    // An empty style the caret stood in and the edit leaves goes with it.
    let collapsed = app.editor.source.doc.selection.primary();
    let (changes, after) = if collapsed.anchor == collapsed.head {
        edit::leaving_empty_markup(f.text, caret_byte(app), changes, after)
    } else {
        (changes.to_vec(), after)
    };
    // Nothing joins a control word to the letters after it (`\item` and an unbolded word).
    let (changes, added) = edit::kept_apart(f.text, &changes);
    let after = (edit::kept_apart_offset(&added, after.0), edit::kept_apart_offset(&added, after.1));
    let changes = changes.as_slice();
    let d = &mut app.editor.source.doc;
    let list = changes
        .iter()
        .map(|c| texcraft_buffer::Change {
            start: d.buffer.byte_to_char(c.range.start),
            end: d.buffer.byte_to_char(c.range.end),
            text: c.text.clone(),
        })
        .collect();
    let Ok(tx) = Transaction::new(list) else { return false };
    let new = edit::apply(f.text, changes);
    // Not inside a dash or a quote the view shows as one character with the one typed.
    let after = if after.0 == after.1 { (edit::out_of_ligature(&new, after.0), edit::out_of_ligature(&new, after.1)) } else { after };
    let chars = |b: usize| new.get(..b).map_or(0, |s| s.chars().count());
    let after = Selection::single(chars(after.0), chars(after.1));
    if kind == EditKind::Other {
        d.buffer.seal();
    }
    let before = d.selection.clone();
    if d.buffer.edit(tx, &before, &after, kind).is_err() {
        return false;
    }
    d.selection = after;
    if kind == EditKind::Other {
        d.buffer.seal();
    }
    true
}

/// Whether the text can be edited here, saying why not when it cannot.
fn editable(app: &mut App) -> bool {
    if app.editor.suggest {
        app.view.status = Some((tl!("Suggestions are made in the source: switch to it to suggest a change.").to_owned(), false));
        return false;
    }
    true
}

/// Where `p` (on screen) falls in line `d`: its nearest caret stop.
fn stop_at(d: &DrawnLine, p: egui::Pos2) -> usize {
    let v = d.galley.cursor_from_pos(p - d.origin).index.0;
    d.line.nearest_stop(v).unwrap_or(0)
}

fn pointer(app: &mut App, ctx: &egui::Context, f: &Frame<'_>) {
    let (pressed, down, shift) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift));
    // A finger: its drag scrolled the view (`touch::scroll`); its tap places the caret, its long
    // press selects the word, and the finger, still down, then extends the selection.
    let finger = crate::touch::is_touch(ctx);
    for (i, d) in f.lines.iter().enumerate() {
        let r = &d.response;
        let starts = if finger { r.clicked() || r.long_touched() } else { pressed && r.is_pointer_button_down_on() };
        if !starts {
            continue;
        }
        let Some(p) = r.interact_pointer_pos() else { continue };
        let head = Pos { line: i, at: stop_at(d, p) };
        let anchor = if shift { selection(app, f).map_or(head, |s| s.0) } else { head };
        let (anchor, head) = if finger && r.long_touched() { word_at(f, head).unwrap_or((anchor, head)) } else { (anchor, head) };
        set_selection(app, f, anchor, head);
        let e = &mut app.editor.formatted.editing;
        // A tap only places the caret; a long press goes on selecting while the finger is down.
        e.dragging = !finger || r.long_touched();
        e.want_x = None;
        e.focus = true;
        ctx.request_repaint();
    }
    // A double-click selects the word, a triple-click the paragraph (Ctrl or Cmd with a
    // double-click goes to the source instead: `formatted::paragraph`).
    let command = ctx.input(|i| i.modifiers.command);
    for (i, d) in f.lines.iter().enumerate().filter(|_| !finger && !command) {
        let r = &d.response;
        let Some(p) = r.interact_pointer_pos().filter(|_| r.double_clicked() || r.triple_clicked()) else { continue };
        let chosen = if r.triple_clicked() {
            d.line.first_stop().zip(d.line.last_stop()).map(|(a, b)| (Pos { line: i, at: a }, Pos { line: i, at: b }))
        } else {
            word_at(f, Pos { line: i, at: stop_at(d, p) })
        };
        if let Some((a, h)) = chosen {
            set_selection(app, f, a, h);
            app.editor.formatted.editing.dragging = false;
            ctx.request_repaint();
        }
    }
    if !app.editor.formatted.editing.dragging {
        return;
    }
    if !down {
        app.editor.formatted.editing.dragging = false;
        return;
    }
    let Some(p) = ctx.pointer_interact_pos() else { return };
    // The line under the pointer, or the nearest one (table cells side by side are told apart
    // across too).
    let distance = |d: &DrawnLine| d.response.rect.distance_sq_to_pos(p);
    let nearest = f.lines.iter().enumerate().min_by(|x, y| distance(x.1).total_cmp(&distance(y.1)));
    if let (Some((i, d)), Some((anchor, head))) = (nearest, selection(app, f)) {
        let to = Pos { line: i, at: stop_at(d, p) };
        if to != head {
            set_selection(app, f, anchor, to);
            ctx.request_repaint();
        }
    }
}

fn input(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, edit: Edit) {
    match edit {
        Edit::Insert(s) => type_text(app, f, &s),
        Edit::LineBreak { .. } => enter(app, f, edit),
        Edit::Key { key, modifiers, .. } => key_press(app, ctx, f, key, modifiers, edit),
        Edit::Paste(s) if takes_structure(app, f) => {
            replace(app, f, "", EditKind::Other);
            app.editor.formatted.editing.pending.insert(0, Edit::Paste(s));
        }
        Edit::Paste(s) => {
            // As it reads where it goes.
            let at = insertion(app, f);
            let source = edit::paste(f.text, at, &s);
            let source = edit::joined(f.text, at, &source);
            replace_at(app, f, (&source, edit::paste_caret(&source)), EditKind::Other);
        }
        Edit::Copy => copy(app, ctx, f, false),
        Edit::Cut => copy(app, ctx, f, true),
        Edit::DeleteSurrounding { before_chars, after_chars } => delete_surrounding(app, f, (before_chars, after_chars)),
        Edit::DeleteSurroundingBytes { before_bytes, after_bytes } => {
            // Whole characters of the line inside the counts.
            let Some((_, h)) = selection(app, f) else { return };
            let chars = f.chars(h.line);
            let before: String = chars.get(..h.at).unwrap_or_default().iter().collect();
            let after: String = chars.get(h.at..).unwrap_or_default().iter().collect();
            delete_surrounding(app, f, craft_ui::ime::surrounding_chars(&before, &after, before_bytes, after_bytes));
        }
        Edit::Preedit { .. } => {}
    }
}

/// The characters an input method deletes around the caret (a phone's Backspace, a correction
/// replacing a word): those of the caret's line as one deletion; one next to an atom (a chip, a
/// formula) as Backspace or Delete there, which selects it.
fn delete_surrounding(app: &mut App, f: &Frame<'_>, (before, after): (usize, usize)) {
    let Some((a, h)) = selection(app, f) else { return };
    let Some(line) = f.doc.lines.get(h.line).filter(|_| a == h) else { return };
    let mut ranges = Vec::new();
    let mut v = h.at;
    for _ in 0..before {
        let Some(r) = line.char_before(v) else { break };
        ranges.push(r);
        v = line.prev_stop(v).unwrap_or(0);
    }
    let mut w = h.at;
    for _ in 0..after {
        let Some(r) = line.char_after(w) else { break };
        ranges.push(r);
        w = line.next_stop(w).unwrap_or(w);
    }
    if ranges.is_empty() {
        if before + after > 0 {
            delete(app, f, before > 0);
        }
        return;
    }
    ranges.sort_by_key(|r| r.start);
    let caret = ranges.first().map_or(0, |r| r.start);
    let changes: Vec<Change> = ranges.into_iter().map(|range| Change { range, text: String::new() }).collect();
    commit(app, f, &changes, (caret, caret), EditKind::Deleting { at: f.now });
}

fn run_command(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, id: &str, params: &serde_json::Value) {
    let at = caret_byte(app);
    let param = |k: &str| params.get(k).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    let names = crate::formatted_bar::heading_commands(app);
    let heading = match id {
        "edit.heading_1" => Some(names[0]),
        "edit.heading_2" => Some(names[1]),
        "edit.heading_3" => Some(names[2]),
        "edit.chapter" | "edit.section" | "edit.subsection" | "edit.subsubsection" | "edit.paragraph" => Some(id.trim_start_matches("edit.")),
        _ => None,
    };
    if let Some(name) = heading {
        let here = texcraft_latex::edit::here(f.text, at);
        if id.starts_with("edit.heading_") && here.heading.is_some() && here.heading == texcraft_latex::view::heading_level(name) {
            return;
        }
        match edit::heading(f.text, at, name) {
            Some(e) => {
                commit(app, f, &e.changes, (e.caret, e.caret), EditKind::Other);
            }
            None => say(app, tl!("Put the caret in a paragraph or a heading to make it a heading.")),
        }
        return;
    }
    match id {
        "edit.bold" | "edit.italic" | "edit.code" => {
            // What sets the style at the caret: a command is taken off, `\emph` too; a
            // declaration is the source's to change.
            let here = crate::formatted_bar::here(app);
            let (name, by) = match id {
                "edit.bold" => ("textbf", here.bold_by),
                "edit.italic" => ("textit", here.italic_by),
                _ => ("texttt", here.code_by),
            };
            match by {
                Some(edit::StyledBy::Declaration) => {
                    say(app, tl!("This style comes from a declaration such as \\bfseries or \\itshape in its group: change it in the source."));
                }
                Some(edit::StyledBy::Emph) => style(app, f, "emph"),
                _ => style(app, f, name),
            }
        }
        "edit.body" => match edit::body(f.text, at) {
            Some(e) => {
                commit(app, f, &e.changes, (e.caret, e.caret), EditKind::Other);
            }
            None => say(app, tl!("The caret is in body text already.")),
        },
        "edit.itemize" | "edit.enumerate" => {
            let name = id.trim_start_matches("edit.");
            let here = texcraft_latex::edit::here(f.text, at);
            let same = matches!(
                (here.list.map(|l| l.0), name),
                (Some(texcraft_latex::view::ListKind::Bullets), "itemize") | (Some(texcraft_latex::view::ListKind::Numbers), "enumerate")
            );
            if same {
                // Pressed again: the item leaves the list.
                structure(app, f, edit::outdent(f.text, at), tl!("Put the caret in a list item to outdent it."));
                return;
            }
            match edit::set_list(f.text, at, name) {
                Some(e) => {
                    commit(app, f, &e.changes, (e.caret, e.caret), EditKind::Other);
                }
                None => say(app, tl!("Put the caret in a paragraph, or in a list of the other kind.")),
            }
        }
        "edit.indent" => structure(app, f, edit::indent(f.text, at), tl!("Put the caret in a list item after the first to indent it.")),
        "edit.outdent" => structure(app, f, edit::outdent(f.text, at), tl!("Put the caret in a list item to outdent it.")),
        "edit.cite" => insert_atom(app, f, &format!("\\cite{{{}}}", param("key"))),
        "edit.ref" => insert_atom(app, f, &format!("\\{}{{{}}}", ref_command(app), param("key"))),
        "edit.footnote" => insert_after_selection(app, f, &format!("\\footnote{{{}}}", edit::escape(&param("text")))),
        "edit.link" => link(app, f, &param("url"), &param("text")),
        "edit.math_inline" => replace(app, f, &format!("\\({}\\)", param("latex")), EditKind::Other),
        "edit.math_display" if !edit::takes_block(f.text, insertion(app, f)) => {
            say(app, tl!("A displayed formula cannot go in a heading, a caption or a footnote: put the caret in a paragraph."));
        }
        "edit.math_display" => replace(app, f, &format!("\\[ {} \\]", param("latex")), EditKind::Other),
        "edit.equation" => new_block(app, f, &format!("\\begin{{equation}}\n  {}\n\\end{{equation}}", param("latex")), None),
        "edit.table" => new_block(app, f, TABLE, Some("Heading")),
        "edit.undo" | "edit.redo" => undo(app, id == "edit.redo"),
        "edit.select_all" => select_all(app, f),
        "edit.copy" | "edit.cut" => copy(app, ctx, f, id == "edit.cut"),
        _ => {}
    }
}

/// A new table: a header row and a row under it, ruled, with its caption and label.
const TABLE: &str = "\\begin{table}[htbp]\n  \\centering\n  \\begin{tabular}{ll}\n    \\hline\n    Heading & Heading \\\\\n    \\hline\n    Text & Text \\\\\n    \\hline\n  \\end{tabular}\n  \\caption{Caption}\n  \\label{tab:new}\n\\end{table}";

/// A structural edit (a list item nested or lifted), or why it cannot be made here.
fn structure(app: &mut App, f: &Frame<'_>, e: Option<edit::SourceEdit>, why: &str) {
    match e {
        Some(e) => {
            commit(app, f, &e.changes, (e.caret, e.caret), EditKind::Other);
        }
        None => say(app, why),
    }
}

/// `source` (a citation, a reference) at the caret, the selection kept before it; after a word,
/// with a no-break space before it, as LaTeX writes them.
fn insert_atom(app: &mut App, f: &Frame<'_>, source: &str) {
    let Some((a, h)) = selection(app, f) else { return };
    // At the caret itself when nothing is selected (in an empty style just made, Italic pressed).
    let end = edit::typing_point(f.text, if a == h { caret_byte(app) } else { f.doc.source(a.max(h)).unwrap_or_else(|| caret_byte(app)) });
    let after_word = f.text.get(..end).and_then(|s| s.chars().next_back()).is_some_and(char::is_alphanumeric);
    let inserted = if after_word { format!("~{source}") } else { source.to_owned() };
    let caret = end + inserted.len();
    commit(app, f, &[Change { range: end..end, text: inserted }], (caret, caret), EditKind::Other);
}

/// `source` after the selection (or at the caret), the caret after it.
fn insert_after_selection(app: &mut App, f: &Frame<'_>, source: &str) {
    let Some((a, h)) = selection(app, f) else { return };
    // At the caret itself when nothing is selected (in an empty style just made, Italic pressed).
    let end = edit::typing_point(f.text, if a == h { caret_byte(app) } else { f.doc.source(a.max(h)).unwrap_or_else(|| caret_byte(app)) });
    let caret = end + source.len();
    commit(app, f, &[Change { range: end..end, text: source.to_owned() }], (caret, caret), EditKind::Other);
}

/// A link: the selected text (in one run) made the link's text, or the address at the caret.
fn link(app: &mut App, f: &Frame<'_>, url: &str, text: &str) {
    let Some((a, h)) = selection(app, f) else { return };
    if a == h {
        let source = if text.is_empty() { format!("\\url{{{url}}}") } else { format!("\\href{{{url}}}{{{}}}", edit::escape(text)) };
        replace(app, f, &source, EditKind::Other);
        return;
    }
    match f.doc.pieces(f.text, a, h).as_slice() {
        // Text, not a chip or a formula (a label made a link breaks its float).
        [one]
            if !f
                .doc
                .lines
                .get(a.min(h).line)
                .is_some_and(|l| l.parts().iter().any(|p| matches!(p, edit::Part::Atom { range, .. } if range == one))) =>
        {
            let open = format!("\\href{{{url}}}{{");
            let changes = [Change { range: one.start..one.start, text: open.clone() }, Change { range: one.end..one.end, text: "}".to_owned() }];
            let caret = one.end + open.len() + 1;
            commit(app, f, &changes, (caret, caret), EditKind::Other);
        }
        _ => say(app, tl!("Select text in one paragraph and one style to make it a link.")),
    }
}

/// A block (a table, an equation) on lines of its own after the block holding the caret; the
/// first `select` of it selected, else the caret at its start.
fn new_block(app: &mut App, f: &Frame<'_>, source: &str, select: Option<&str>) {
    let caret = caret_byte(app);
    let at = edit::block_point(f.text, caret);
    let before = f.text.get(..at).unwrap_or_default();
    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let inserted = format!("{lead}{source}\n");
    let start = at + lead.len();
    let sel = select.and_then(|s| source.find(s).map(|i| (start + i, start + i + s.len()))).unwrap_or((start, start));
    // An empty style the caret stood in (Bold pressed, nothing typed) is not left behind.
    let (changes, sel) = edit::dropping_empty_style(f.text, caret, Change { range: at..at, text: inserted }, sel);
    commit(app, f, &changes, sel, EditKind::Other);
}

fn say(app: &mut App, message: &str) {
    app.view.status = Some((message.to_owned(), false));
}

fn type_text(app: &mut App, f: &Frame<'_>, s: &str) {
    let Some((a, h)) = selection(app, f) else { return };
    if s == " " && a == h {
        // Spaces collapse in print: a second one would show nothing.
        let chars = f.chars(h.line);
        let before = h.at.checked_sub(1).and_then(|i| chars.get(i));
        if before.is_none_or(|c| c.is_whitespace()) || chars.get(h.at).is_some_and(|c| *c == ' ') {
            return;
        }
    }
    if takes_structure(app, f) {
        // Whole list items go first; the text comes on the view without them.
        replace(app, f, "", EditKind::Other);
        app.editor.formatted.editing.pending.insert(0, Edit::Insert(s.to_owned()));
        return;
    }
    let kind = if a == h { EditKind::Typing { at: f.now } } else { EditKind::Other };
    let at = caret_byte(app);
    // After a command shown as text (`\dots`), a letter would lengthen its name and TeX would
    // skip a space: an empty group ends it (a space, after `\item`).
    let source = edit::joined(f.text, insertion(app, f), &edit::escape(s));
    let version = app.editor.source.doc.buffer.version();
    replace(app, f, &source, kind);
    if a == h && s.ends_with('.') && app.editor.source.doc.buffer.version() != version {
        typographic(app, at + source.len());
    }
}

/// Three dots typed become `\dots` ([`edit::ellipsis`]): an edit of its own after the typing, so
/// Undo brings the dots back.
fn typographic(app: &mut App, at: usize) {
    let d = &mut app.editor.source.doc;
    let text = d.buffer.text();
    let Some(c) = edit::ellipsis(&text, at) else { return };
    // Before a word, `\dots{}`.
    let c = edit::kept_apart(&text, std::slice::from_ref(&c)).0.into_iter().next().unwrap_or(c);
    let (start, end) = (d.buffer.byte_to_char(c.range.start), d.buffer.byte_to_char(c.range.end));
    let caret = start + c.text.chars().count();
    let before = d.selection.clone();
    d.buffer.seal();
    if d.buffer.edit(Transaction::replace(start, end, &c.text), &before, &Selection::single(caret, caret), EditKind::Other).is_ok() {
        d.selection = Selection::single(caret, caret);
        d.buffer.seal();
    }
}

/// Whether deleting the selection removes more than the source of what it shows: list items it
/// holds whole, with their `\item`s.
fn takes_structure(app: &App, f: &Frame<'_>) -> bool {
    selection(app, f).is_some_and(|(a, h)| a != h && f.doc.deleted(f.text, a, h) != f.doc.pieces(f.text, a, h))
}

/// Where text replacing the selection goes: where its first run starts, or the caret (in the
/// heading, after a heading's label: [`edit::typing_point`]).
fn insertion(app: &App, f: &Frame<'_>) -> usize {
    let at = caret_byte(app);
    match selection(app, f) {
        Some((a, h)) if a != h => edit::insertion(f.text, &f.doc, (a, h)).unwrap_or(at),
        _ => edit::typing_point(f.text, at),
    }
}

/// Replaces the selection (or inserts at the caret) with `source`. Deleting (`source` empty)
/// removes the list items the selection holds whole ([`Doc::deleted`]).
fn replace(app: &mut App, f: &Frame<'_>, source: &str, kind: EditKind) {
    replace_at(app, f, (source, source.len()), kind);
}

/// [`replace`], the caret then at byte `caret` of `source`.
fn replace_at(app: &mut App, f: &Frame<'_>, (source, caret): (&str, usize), kind: EditKind) {
    let Some((a, h)) = selection(app, f) else { return };
    if !source.is_empty() && takes_structure(app, f) {
        // The whole items go first, then the text goes where the caret is left: one change.
        if let Some(e) = edit::replace_structure(f.text, &f.doc, (a, h), (source, caret)) {
            commit(app, f, &e.changes, (e.caret, e.caret), kind);
        }
        return;
    }
    let pieces = match (a == h, source.is_empty()) {
        (true, _) => Vec::new(),
        (false, true) => f.doc.deleted(f.text, a, h),
        (false, false) => f.doc.pieces(f.text, a, h),
    };
    // A deletion of what is alone on its line takes the line's end (no blank line is left).
    let pieces: Vec<_> = if source.is_empty() { pieces.into_iter().map(|r| edit::alone_on_line(f.text, r)).collect() } else { pieces };
    let mut changes: Vec<Change> = pieces.iter().map(|r| Change { range: r.clone(), text: String::new() }).collect();
    // The text goes where the selection starts: in its first run, or before them (in an empty
    // heading the selection starts in).
    let at = insertion(app, f);
    match changes.first_mut() {
        Some(first) if first.range.start <= at => first.text = source.to_owned(),
        _ if source.is_empty() => {}
        _ => changes.insert(0, Change { range: at..at, text: source.to_owned() }),
    }
    // Nothing changes before it.
    let at = changes.first().map_or(at, |c| c.range.start.min(at));
    let caret = at + caret.min(source.len());
    // A deletion holding its first line whole leaves the caret where what is left of its last line
    // starts; one of whole items, in the item after them, not before its `\item`.
    let caret = if source.is_empty() && a != h {
        let caret = edit::deleted_caret(f.text, &f.doc, (a, h), &changes).unwrap_or(caret);
        let after = edit::apply(f.text, &changes);
        edit::in_float(&after, edit::into_item(&after, caret))
    } else {
        caret
    };
    commit(app, f, &changes, (caret, caret), kind);
}

fn enter(app: &mut App, f: &Frame<'_>, edit: Edit) {
    let Some((a, h)) = selection(app, f) else { return };
    if let Some(atom) = selected_atom(app, f, h.line) {
        // A formula or a command shown as source, selected: Enter edits it here.
        if let Some(path) = app.session.current().cloned() {
            crate::formatted::in_place::open(app, crate::formatted::in_place::InPlace::of_atom(&path, f.text, &atom));
        }
        return;
    }
    if a != h {
        // The selection goes first; the line break comes on the view without it.
        replace(app, f, "", EditKind::Other);
        app.editor.formatted.editing.pending.insert(0, edit);
        return;
    }
    if !editable(app) {
        return;
    }
    match edit::enter(f.text, caret_byte(app)) {
        Some(e) => {
            commit(app, f, &e.changes, (e.caret, e.caret), EditKind::Other);
        }
        None => say(app, tl!("A paragraph cannot be broken here: this text is part of a caption, a footnote or a formula.")),
    }
}

/// The run of line `line` that opens in the source editor, when the selection is exactly it.
fn selected_atom(app: &App, f: &Frame<'_>, line: usize) -> Option<crate::formatted::in_place::Atom> {
    let d = &app.editor.source.doc;
    let r = d.selection.primary();
    let (a, b) = (d.buffer.char_to_byte(r.start()), d.buffer.char_to_byte(r.end()));
    (a < b).then_some(())?;
    f.lines.get(line)?.atoms.iter().find(|atom| atom.range == (a..b)).cloned()
}

/// Backspace (`back`) or Delete.
fn delete(app: &mut App, f: &Frame<'_>, back: bool) {
    let Some((a, h)) = selection(app, f) else { return };
    if a != h {
        replace(app, f, "", EditKind::Other);
        return;
    }
    let Some(line) = f.doc.lines.get(h.line) else { return };
    let atom = if back { line.atom_before(h.at) } else { line.atom_after(h.at) };
    if let Some(r) = atom {
        // An atom is selected first, and deleted by the next press.
        set_bytes(app, (r.start, r.end), Some(h.line));
        return;
    }
    let ch = if back { line.char_before(h.at) } else { line.char_after(h.at) };
    if let Some(r) = ch {
        // An e-mail's last character takes its command: the view has no place for an empty one.
        if let Some((field, caret)) = edit::emptied_field(f.text, &r) {
            commit(app, f, &[Change { range: field, text: String::new() }], (caret, caret), EditKind::Other);
            return;
        }
        // Alone on its line, with that line's end: no blank line is left to end the paragraph.
        let r = edit::alone_on_line(f.text, r);
        // An empty style the caret stands in (Bold pressed, nothing typed) goes too.
        let mut changes = vec![Change { range: r.clone(), text: String::new() }];
        let mut caret = r.start;
        if let Some(empty) = edit::empty_styles_at(f.text, caret_byte(app)).filter(|e| e.end <= r.start || r.end <= e.start) {
            if !back {
                caret = empty.start;
            }
            changes.push(Change { range: empty, text: String::new() });
            changes.sort_by_key(|c| c.range.start);
        }
        let caret = edit::map_offset(&changes, caret);
        commit(app, f, &changes, (caret, caret), EditKind::Deleting { at: f.now });
        return;
    }
    let step = if back { line.prev_stop(h.at) } else { line.next_stop(h.at) };
    if let Some(v) = step {
        // Past what the view adds (a gap before a label).
        set_selection(app, f, Pos { line: h.line, at: v }, Pos { line: h.line, at: v });
        return;
    }
    // At a heading's or a list item's start: it becomes body text, or goes one level out.
    if back && let Some(e) = edit::backspace_at_start(f.text, caret_byte(app)) {
        commit(app, f, &e.changes, (e.caret, e.caret), EditKind::Other);
        return;
    }
    // At a line's edge: the paragraph break goes, joining two paragraphs.
    let gap = if back { h.line.checked_sub(1).and_then(|l| f.doc.gap_after(f.text, l)) } else { f.doc.gap_after(f.text, h.line) };
    if let Some(gap) = gap {
        commit(app, f, &[Change { range: gap.clone(), text: String::new() }], (gap.start, gap.start), EditKind::Other);
    }
}

fn key_press(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, key: egui::Key, m: egui::Modifiers, edit: Edit) {
    use egui::Key as K;
    let word = m.command || m.alt;
    match key {
        K::ArrowLeft | K::ArrowRight => horizontal(app, f, key == K::ArrowLeft, m.shift, word),
        K::ArrowUp | K::ArrowDown => vertical(app, f, key == K::ArrowUp, m.shift),
        K::Home | K::End => line_edge(app, f, key == K::Home, m.shift, m.command),
        K::Backspace => delete(app, f, true),
        K::Delete => delete(app, f, false),
        K::Enter => enter(app, f, edit),
        K::Tab if f.lines.get(selection(app, f).map_or(0, |s| s.1.line)).is_some_and(|d| d.cell.is_some()) => next_cell(app, f, m.shift),
        K::Tab => {
            let at = caret_byte(app);
            if m.shift {
                structure(app, f, edit::outdent(f.text, at), tl!("Put the caret in a list item to outdent it."));
            } else {
                structure(app, f, edit::indent(f.text, at), tl!("Put the caret in a list item after the first to indent it."));
            }
        }
        K::F10 if m.shift => app.editor.formatted.editing.menu_by_key = true,
        K::Z if m.command => undo(app, m.shift),
        K::Y if m.command => undo(app, true),
        K::A if m.command => select_all(app, f),
        _ => return,
    }
    ctx.request_repaint();
}

fn undo(app: &mut App, redo: bool) {
    let d = &mut app.editor.source.doc;
    let sel = if redo { d.buffer.redo() } else { d.buffer.undo() };
    if let Some(sel) = sel {
        d.selection = sel.clamp(d.buffer.len_chars());
    }
    app.editor.formatted.editing.caret_line = None;
}

fn select_all(app: &mut App, f: &Frame<'_>) {
    let last = f.doc.lines.len().saturating_sub(1);
    let first = Pos { line: 0, at: f.doc.lines.first().and_then(|l| l.first_stop()).unwrap_or(0) };
    let end = Pos { line: last, at: f.doc.lines.last().and_then(|l| l.last_stop()).unwrap_or(0) };
    set_selection(app, f, first, end);
}

fn horizontal(app: &mut App, f: &Frame<'_>, left: bool, shift: bool, word: bool) {
    let Some((a, h)) = selection(app, f) else { return };
    app.editor.formatted.editing.want_x = None;
    if !shift && a != h && !word {
        let to = if left { a.min(h) } else { a.max(h) };
        set_selection(app, f, to, to);
        return;
    }
    let Some(line) = f.doc.lines.get(h.line) else { return };
    let next = if word {
        word_stop(f, h, left)
    } else if left {
        line.prev_stop(h.at)
    } else {
        line.next_stop(h.at)
    };
    let to = match next {
        Some(at) => Pos { line: h.line, at },
        None if left && h.line > 0 => Pos { line: h.line - 1, at: f.doc.lines.get(h.line - 1).and_then(|l| l.last_stop()).unwrap_or(0) },
        None if !left && h.line + 1 < f.doc.lines.len() => {
            Pos { line: h.line + 1, at: f.doc.lines.get(h.line + 1).and_then(|l| l.first_stop()).unwrap_or(0) }
        }
        None => h,
    };
    set_selection(app, f, if shift { a } else { to }, to);
}

/// The stop at the start (left) or end of the word before or after `h`, in its line.
fn word_stop(f: &Frame<'_>, h: Pos, left: bool) -> Option<usize> {
    let line = f.doc.lines.get(h.line)?;
    let chars = f.chars(h.line);
    let is_word = |i: usize| chars.get(i).is_some_and(|c| c.is_alphanumeric());
    let mut p = h.at;
    if left {
        if p == 0 || line.prev_stop(p).is_none() {
            return None;
        }
        while p > 0 && !is_word(p - 1) {
            p -= 1;
        }
        while p > 0 && is_word(p - 1) {
            p -= 1;
        }
        if line.is_stop(p) { Some(p) } else { line.next_stop(p).filter(|s| *s < h.at).or_else(|| line.prev_stop(h.at)) }
    } else {
        line.next_stop(p)?;
        while p < chars.len() && !is_word(p) {
            p += 1;
        }
        while p < chars.len() && is_word(p) {
            p += 1;
        }
        if line.is_stop(p) { Some(p) } else { line.prev_stop(p).filter(|s| *s > h.at).or_else(|| line.next_stop(h.at)) }
    }
}

fn vertical(app: &mut App, f: &Frame<'_>, up: bool, shift: bool) {
    let Some((a, h)) = selection(app, f) else { return };
    let Some(d) = f.lines.get(h.line) else { return };
    let caret = d.galley.pos_from_cursor(CCursor::new(h.at)).translate(d.origin.to_vec2());
    let x = *app.editor.formatted.editing.want_x.get_or_insert(caret.center().x);
    let y = if up { caret.top() - 2.0 } else { caret.bottom() + 2.0 };
    let inside = |d: &DrawnLine| {
        let (Some(first), Some(last)) = (d.galley.rows.first(), d.galley.rows.last()) else { return false };
        d.origin.y + first.min_y() <= y && y <= d.origin.y + last.max_y()
    };
    let to = if inside(d) {
        Pos { line: h.line, at: stop_at(d, egui::pos2(x, y)) }
    } else {
        let other = match d.cell {
            Some(c) => cell_neighbour(f, c, up),
            None if up => h.line.checked_sub(1),
            None => Some(h.line + 1),
        };
        match other.and_then(|i| f.lines.get(i).map(|o| (i, o))) {
            Some((i, o)) => {
                let row = if up { o.galley.rows.last() } else { o.galley.rows.first() };
                let y = o.origin.y + row.map_or(0.0, |r| (r.min_y() + r.max_y()) / 2.0);
                Pos { line: i, at: stop_at(o, egui::pos2(x, y)) }
            }
            None => {
                let line = f.doc.lines.get(h.line);
                let at = if up { line.and_then(|l| l.first_stop()) } else { line.and_then(|l| l.last_stop()) };
                Pos { line: h.line, at: at.unwrap_or(h.at) }
            }
        }
    };
    let (Some(ab), Some(tb)) = (f.doc.source(if shift { a } else { to }), f.doc.source(to)) else { return };
    set_bytes(app, (ab, tb), Some(to.line));
}

/// The line Up or Down goes to from a table cell: the cell above or below (the one taking its
/// column), or the line before or after the table.
fn cell_neighbour(f: &Frame<'_>, c: crate::formatted::CellAt, up: bool) -> Option<usize> {
    let in_table = |d: &DrawnLine| d.cell.is_some_and(|o| o.table == c.table);
    let row = if up { c.row.checked_sub(1) } else { Some(c.row + 1).filter(|r| *r < c.rows) };
    match row {
        Some(row) => f
            .lines
            .iter()
            .enumerate()
            .filter(|(_, d)| d.cell.is_some_and(|o| o.table == c.table && o.row == row && o.column <= c.column))
            .map(|(i, _)| i)
            .next_back(),
        None if up => f.lines.iter().position(in_table).and_then(|i| i.checked_sub(1)),
        None => f.lines.iter().rposition(in_table).map(|i| i + 1),
    }
}

/// Tab and Shift+Tab in a table: the next or the previous cell, its text selected.
fn next_cell(app: &mut App, f: &Frame<'_>, back: bool) {
    let Some((_, h)) = selection(app, f) else { return };
    let Some(c) = f.lines.get(h.line).and_then(|d| d.cell) else { return };
    let next = if back { h.line.checked_sub(1) } else { Some(h.line + 1) };
    let Some((i, line)) = next.and_then(|i| Some((i, f.lines.get(i).filter(|d| d.cell.is_some_and(|o| o.table == c.table))?))) else {
        say(app, if back { tl!("This is the table's first cell.") } else { tl!("This is the table's last cell.") });
        return;
    };
    let (first, last) = (line.line.first_stop().unwrap_or(0), line.line.last_stop().unwrap_or(0));
    app.editor.formatted.editing.want_x = None;
    set_selection(app, f, Pos { line: i, at: first }, Pos { line: i, at: last });
}

/// Home and End: the start or end of the caret's row, or of the text with Ctrl.
fn line_edge(app: &mut App, f: &Frame<'_>, home: bool, shift: bool, whole: bool) {
    let Some((a, h)) = selection(app, f) else { return };
    app.editor.formatted.editing.want_x = None;
    let to = if whole {
        let line = if home { 0 } else { f.doc.lines.len().saturating_sub(1) };
        let l = f.doc.lines.get(line);
        Pos { line, at: (if home { l.and_then(|l| l.first_stop()) } else { l.and_then(|l| l.last_stop()) }).unwrap_or(0) }
    } else {
        let (Some(d), Some(line)) = (f.lines.get(h.line), f.doc.lines.get(h.line)) else { return };
        let (start, end) = row_bounds(&d.galley, h.at);
        let at = if home {
            Some(start).filter(|s| line.is_stop(*s)).or_else(|| line.next_stop(start))
        } else {
            Some(end).filter(|e| line.is_stop(*e)).or_else(|| line.prev_stop(end))
        };
        Pos { line: h.line, at: at.unwrap_or(h.at) }
    };
    set_selection(app, f, if shift { a } else { to }, to);
}

/// The first and last character positions of the galley row holding position `at`.
fn row_bounds(galley: &egui::Galley, at: usize) -> (usize, usize) {
    let mut start = 0;
    let rows = galley.rows.len();
    for (i, row) in galley.rows.iter().enumerate() {
        let with_break = row.char_count_including_newline().0;
        let without = row.char_count_excluding_newline().0;
        if at < start + with_break || i + 1 == rows {
            return (start, start + without);
        }
        start += with_break;
    }
    (0, 0)
}

/// Copies the selection as the text shown (Cut also deletes it).
/// The text shown from `lo` to `hi`, its lines as paragraphs.
fn shown_text(f: &Frame<'_>, lo: Pos, hi: Pos) -> String {
    let mut out = Vec::new();
    for i in lo.line..=hi.line {
        let chars = f.chars(i);
        let from = if i == lo.line { lo.at } else { 0 };
        let to = if i == hi.line { hi.at } else { chars.len() };
        let piece: String = chars.get(from..to.max(from)).unwrap_or_default().iter().collect();
        out.push(piece.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    out.join("\n\n")
}

/// Once the selection settles (no button held), the text it shows becomes the primary selection,
/// to paste with a middle click (X11, Wayland; elsewhere the window ignores it).
fn offer_primary(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, (a, h): (Pos, Pos)) {
    if ctx.input(|i| i.pointer.any_down()) {
        return;
    }
    let selected = (a != h).then(|| (a.min(h), a.max(h)));
    let e = &mut app.editor.formatted.editing;
    if selected == e.primary_sent {
        return;
    }
    e.primary_sent = selected;
    if let Some((lo, hi)) = selected {
        ctx.send_cmd(egui::OutputCommand::TextSelectionSettled(shown_text(f, lo, hi)));
    }
}

fn copy(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, cut: bool) {
    let Some((a, h)) = selection(app, f) else { return };
    let (lo, hi) = (a.min(h), a.max(h));
    if lo == hi {
        say(app, tl!("Select some text first."));
        return;
    }
    ctx.copy_text(shown_text(f, lo, hi));
    if cut {
        replace(app, f, "", EditKind::Other);
    }
}

/// Bold or italic on the selection (the word at the caret when nothing is selected): each run's
/// source wrapped in the command, or unwrapped when it is all in one already.
fn style(app: &mut App, f: &Frame<'_>, name: &str) {
    let Some((a, h)) = selection(app, f) else { return };
    // In an empty style just made (Italic pressed, nothing typed): pressed again, it goes; another
    // goes in it, for the text to come; else the word at the caret.
    let empty = edit::empty_style_at(f.text, caret_byte(app));
    if a == h
        && let Some(r) = empty.clone().filter(|r| f.text.get(r.clone()).is_some_and(|s| s.starts_with(&format!("\\{name}{{"))))
    {
        commit(app, f, &[Change { range: r.clone(), text: String::new() }], (r.start, r.start), EditKind::Other);
        return;
    }
    let empty = empty.is_some();
    let (a, h) = if a == h && !empty { word_at(f, h).unwrap_or((a, h)) } else { (a.min(h), a.max(h)) };
    if a == h {
        // Nothing to wrap: an empty command, and the caret in it, where typing goes (in a heading
        // or a caption, not after its label).
        let at = edit::typing_point(f.text, caret_byte(app));
        let inserted = format!("\\{name}{{}}");
        let caret = at + inserted.len() - 1;
        commit(app, f, &[Change { range: at..at, text: inserted }], (caret, caret), EditKind::Other);
        return;
    }
    // Each line's runs, without the breaks between paragraphs.
    let mut pieces = Vec::new();
    for i in a.line..=h.line {
        let Some(line) = f.doc.lines.get(i) else { continue };
        let from = if i == a.line { a.at } else { 0 };
        let to = if i == h.line { h.at } else { line.len() };
        pieces.extend(line.pieces(from, to));
    }
    let changes = edit::toggle_style(f.text, &pieces, name);
    if changes.is_empty() {
        // Part of a styled run: there is nothing to wrap, and its command is not the selection's.
        say(app, tl!("Select all of the styled text to take its style away."));
        return;
    }
    let (Some(start), Some(end)) = (pieces.first().map(|r| r.start), pieces.last().map(|r| r.end)) else { return };
    // The selection keeps the same text: inside the commands added, without those removed.
    let start = edit::map_offset(&changes, start);
    let end = map_before(&changes, end);
    commit(app, f, &changes, (start, end), EditKind::Other);
}

/// Like [`edit::map_offset`], but before what is inserted at `at`.
fn map_before(changes: &[Change], at: usize) -> usize {
    let mut delta: isize = 0;
    for c in changes {
        if c.range.start >= at && !(c.range.start == at && c.range.end > at && c.text.is_empty()) {
            break;
        }
        if at < c.range.end {
            return c.range.start.saturating_add_signed(delta);
        }
        delta += c.text.len() as isize - (c.range.end - c.range.start) as isize;
    }
    at.saturating_add_signed(delta)
}

/// The word around caret `h`, when it is inside one.
fn word_at(f: &Frame<'_>, h: Pos) -> Option<(Pos, Pos)> {
    let line = f.doc.lines.get(h.line)?;
    let chars = f.chars(h.line);
    let is_word = |i: usize| chars.get(i).is_some_and(|c| c.is_alphanumeric());
    // Inside a word, or at its start or end.
    if !(is_word(h.at) || (h.at > 0 && is_word(h.at - 1))) {
        return None;
    }
    let (mut s, mut e) = (h.at, h.at);
    while s > 0 && is_word(s - 1) {
        s -= 1;
    }
    while is_word(e) {
        e += 1;
    }
    let s = if line.is_stop(s) { s } else { line.next_stop(s)? };
    let e = if line.is_stop(e) { e } else { line.prev_stop(e)? };
    (s < e).then_some((Pos { line: h.line, at: s }, Pos { line: h.line, at: e }))
}

fn paint(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, painter: &egui::Painter, (a, h): (Pos, Pos), focused: bool) {
    let t = app.tokens;
    let (lo, hi) = (a.min(h), a.max(h));
    for (i, d) in f.lines.iter().enumerate() {
        if lo == hi || i < lo.line || i > hi.line {
            continue;
        }
        let from = if i == lo.line { lo.at } else { 0 };
        let to = if i == hi.line { hi.at } else { d.line.len() };
        let mut rects = span_rects(&d.galley, (from, to));
        // A line selected through its end shows it, even when empty.
        if i < hi.line {
            let end = d.galley.pos_from_cursor(CCursor::new(d.line.len()));
            rects.push(Rect::from_min_size(end.min, vec2(t.reading_size * 0.3, end.height())));
        }
        let shapes = rects.into_iter().map(|r| egui::Shape::rect_filled(r.translate(d.origin.to_vec2()), 0.0, t.selection)).collect();
        painter.set(d.slot, egui::Shape::Vec(shapes));
    }
    if !focused {
        return;
    }
    let Some(d) = f.lines.get(h.line) else { return };
    let c = d.galley.pos_from_cursor(CCursor::new(h.at)).translate(d.origin.to_vec2());
    let c = if c.height() < 1.0 { Rect::from_min_size(c.min - vec2(0.0, t.reading_size * 0.6), vec2(0.0, t.reading_size * 1.2)) } else { c };
    painter.rect_filled(Rect::from_min_size(c.min, vec2(2.0, c.height())), 0.0, t.text);
    // The text an input method is composing, underlined at the caret until it is committed.
    if let Some(pre) = app.editor.formatted.editing.composer.preedit() {
        let g = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(pre.text.to_owned(), t.reading(t.reading_size, false, false), t.text));
        let r = Rect::from_min_size(c.min, vec2(g.size().x, c.height().max(g.size().y)));
        painter.rect_filled(r, 0.0, t.surface);
        painter.galley(pos2(r.min.x, r.center().y - g.size().y / 2.0), g, t.text);
        painter.hline(r.x_range(), r.bottom(), egui::Stroke::new(1.0, t.text));
    }
    app.editor.formatted.editing.composer.set_caret(ctx, c);
}

/// The view as an editable multi-line text node: its value the lines' text, its caret and
/// selection in them, one text run per line.
fn accessibility(ui: &mut egui::Ui, response: &egui::Response, path: &ProjectPath, f: &Frame<'_>, (a, h): (Pos, Pos)) {
    let id = response.id;
    if ui.ctx().accesskit_node_builder(id, |_| ()).is_none() {
        return;
    }
    let name = tl!("{file} formatted").replace("{file}", path.file_name());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, &name));
    let run = |line: usize| id.with(("line", line));
    // Each character as read: a typeset formula, drawn over one space, reads as its LaTeX.
    let read: Vec<Vec<String>> = f.lines.iter().map(|d| read_chars(f.text, d)).collect();
    let texts: Vec<String> = read.iter().map(|chars| chars.concat()).collect();
    let position = |p: Pos| egui::accesskit::TextPosition { node: run(p.line).accesskit_id(), character_index: p.at };
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::MultilineTextInput);
        node.set_label(name.as_str());
        node.set_value(texts.join("\n"));
        node.set_text_selection(egui::accesskit::TextSelection { anchor: position(a), focus: position(h) });
    });
    let child = ui.new_child(egui::UiBuilder::new().accessibility_parent(id));
    for (i, ((d, value), chars)) in f.lines.iter().zip(texts).zip(read).enumerate() {
        let rect = d.galley.rect.translate(d.origin.to_vec2());
        let _ = child.interact(rect, run(i), Sense::hover());
        child.ctx().accesskit_node_builder(run(i), |node| {
            node.set_role(egui::accesskit::Role::TextRun);
            node.set_bounds(egui::accesskit::Rect {
                x0: rect.left().into(),
                y0: rect.top().into(),
                x1: rect.right().into(),
                y1: rect.bottom().into(),
            });
            let lengths: Vec<u8> = chars.iter().map(|c| c.len().min(255) as u8).collect();
            node.set_value(value);
            node.set_character_lengths(lengths);
        });
    }
}

/// The characters of line `d` as a screen reader reads them: those drawn, but a typeset formula's
/// one (a space the formula is drawn over) as the formula's LaTeX (at most 255 bytes, a character
/// length's limit).
fn read_chars(text: &str, d: &DrawnLine) -> Vec<String> {
    let mut chars: Vec<String> = d.galley.text().chars().map(String::from).collect();
    let mut at = 0;
    for part in d.line.parts() {
        if let edit::Part::Atom { chars: 1, range } = part
            && chars.get(at).is_some_and(|c| c == "\u{a0}")
            && let Some((math, _)) = d.atoms.iter().find(|a| &a.range == range).and_then(|a| a.math.as_ref())
        {
            let src = text.get(math.clone()).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
            let mut end = src.len().min(255);
            while !src.is_char_boundary(end) {
                end -= 1;
            }
            if let (Some(c), false) = (chars.get_mut(at), src.is_empty()) {
                *c = src.get(..end).unwrap_or_default().to_owned();
            }
        }
        at += part.chars();
    }
    chars
}

/// The misspelled word at the caret, its bytes and text.
fn misspelled_at_caret(app: &App) -> Option<(std::ops::Range<usize>, String)> {
    let r = app.editor.formatted.misspelling_at(caret_byte(app))?;
    let word = app.editor.source.doc.buffer.text().get(r.clone())?.to_owned();
    Some((r, word))
}

/// The spelling commands (`edit.spelling.replace {text?}`, `.add`, `.ignore`) on the formatted
/// view's caret; false when the view is not shown.
pub(crate) fn spelling(app: &mut App, id: &str, params: &serde_json::Value) -> bool {
    let Some(path) = app.session.current().cloned() else { return false };
    if !app.editor.formatted.shows(&path) {
        return false;
    }
    let Some((range, word)) = misspelled_at_caret(app) else {
        app.view.status = Some((tl!("The caret is not on a misspelled word.").to_owned(), true));
        return true;
    };
    if id != "edit.spelling.replace" {
        crate::spell::keep(app, id, &word, params);
        return true;
    }
    let chosen = params
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| crate::spell::suggestions(app, &word).into_iter().next());
    let Some(text) = chosen else {
        app.view.status = Some((tl!("The dictionary has no spelling to offer for “{word}”.").replace("{word}", &word), true));
        return true;
    };
    if !editable(app) {
        return true;
    }
    // The word's source replaced, its spelling escaped as typing is.
    let source = edit::escape(&text);
    let d = &mut app.editor.source.doc;
    let (a, b) = (d.buffer.byte_to_char(range.start), d.buffer.byte_to_char(range.end));
    let caret = a + source.chars().count();
    let before = d.selection.clone();
    d.buffer.seal();
    if d.buffer.edit(Transaction::replace(a, b, &source), &before, &Selection::single(caret, caret), EditKind::Other).is_ok() {
        d.selection = Selection::single(caret, caret);
        d.buffer.seal();
    }
    app.editor.formatted.editing.focus = true;
    true
}

/// The text's context menu, opened by a right-click or from the keyboard (Shift+F10, the Menu
/// key, which give its first entry the keyboard): on a misspelled word its spellings, Add to
/// Dictionary and Ignore first; then the clipboard and Select All. Each entry runs its command;
/// closed from the keyboard without a choice, it gives the keyboard back to the text.
fn context_menu(app: &mut App, ui: &mut egui::Ui, f: &Frame<'_>, response: &egui::Response, focused: bool) {
    let t = app.tokens;
    let ctx = ui.ctx().clone();
    let by_key = response.id.with("menu-by-key");
    let mut menu = egui::Popup::context_menu(response);
    // A right-click on a line: the caret there first, unless it is in the selection.
    let clicked = f.lines.iter().enumerate().find_map(|(i, d)| d.response.secondary_clicked().then_some(i).zip(d.response.interact_pointer_pos()));
    if let Some((i, p)) = clicked
        && let Some(d) = f.lines.get(i)
    {
        let at = Pos { line: i, at: stop_at(d, p) };
        let inside = selection(app, f).is_some_and(|(a, h)| a.min(h) <= at && at <= a.max(h) && a != h);
        if !inside {
            set_selection(app, f, at, at);
        }
        menu = menu.open_memory(egui::SetOpenCommand::Bool(true)).at_position(p);
        app.editor.formatted.editing.focus = true;
    }
    let asked = std::mem::take(&mut app.editor.formatted.editing.menu_by_key);
    if focused && asked {
        ctx.data_mut(|d| d.insert_temp(by_key, true));
        let at = selection(app, f).and_then(|(_, h)| {
            let d = f.lines.get(h.line)?;
            Some(d.galley.pos_from_cursor(CCursor::new(h.at)).translate(d.origin.to_vec2()).left_bottom())
        });
        ctx.data_mut(|d| d.insert_temp(by_key.with("at"), at.unwrap_or(response.rect.left_top())));
        menu = menu.open_memory(egui::SetOpenCommand::Bool(true));
    }
    let keyed = ctx.data(|d| d.get_temp::<bool>(by_key));
    // Opened from the keyboard, it stays at the caret (there is no pointer to follow).
    if keyed.is_some()
        && let Some(at) = ctx.data(|d| d.get_temp::<egui::Pos2>(by_key.with("at")))
    {
        menu = menu.at_position(at);
    }
    let word = misspelled_at_caret(app).map(|(_, w)| w);
    let offered = word.as_ref().map(|w| crate::spell::suggestions(app, w));
    let selected = selection(app, f).is_some_and(|(a, h)| a != h);
    let mut chosen: Option<(&'static str, serde_json::Value)> = None;
    let shown = menu.show(|ui| {
        ui.set_min_width(220.0);
        let mut focus = keyed == Some(true);
        if let Some(offered) = &offered {
            if offered.is_empty() {
                ui.label(egui::RichText::new(tl!("No spellings to offer")).font(t.small()).color(t.text_muted));
            }
            let changes =
                offered.iter().map(|s| ("edit.spelling.replace", tl!("Change to “{text}”").replace("{text}", s), serde_json::json!({"text": s})));
            let others = [
                ("edit.spelling.add", tl!("Add to Dictionary").to_owned(), serde_json::json!({})),
                ("edit.spelling.ignore", tl!("Ignore").to_owned(), serde_json::json!({})),
            ];
            for (id, label, params) in changes.chain(others) {
                let icon = if id == "edit.spelling.replace" { Some("pencil") } else { crate::top_bar::icon_for(id) };
                let item = crate::top_bar::item_response(ui, &t, icon, &label, None, Ok(()));
                if std::mem::take(&mut focus) {
                    item.request_focus();
                }
                if item.clicked() {
                    chosen = Some((id, params));
                }
            }
            ui.separator();
        }
        for (id, label) in [
            ("edit.cut", tl!("Cut")),
            ("edit.copy", tl!("Copy")),
            ("edit.paste", tl!("Paste")),
            ("edit.select_all", tl!("Select All")),
            ("view.show_in_source", tl!("Show in Source")),
        ] {
            let enabled = match id {
                "edit.cut" | "edit.copy" if !selected => Err(tl!("Select some text first.").to_owned()),
                _ => crate::ui_cmd::enabled(app, id),
            };
            let can = enabled.is_ok();
            let item = crate::top_bar::item_response(ui, &t, crate::top_bar::icon_for(id), label, app.keys.shortcut_text(id), enabled);
            if focus && can {
                item.request_focus();
                focus = false;
            }
            if item.clicked() {
                chosen = Some((id, serde_json::json!({})));
            }
        }
        if chosen.is_some() {
            ui.close();
        }
    });
    if keyed == Some(true) {
        ctx.data_mut(|d| d.insert_temp(by_key, false));
    }
    if shown.is_none() || chosen.is_some() {
        ctx.data_mut(|d| d.remove::<bool>(by_key));
        if keyed.is_some() || chosen.is_some() {
            app.editor.formatted.editing.focus = true;
        }
    }
    if let Some((id, params)) = chosen {
        crate::ui_cmd::run_with(app, id, &params);
    }
}
