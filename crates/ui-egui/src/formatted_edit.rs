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
use egui::{Rect, Sense, vec2};
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
    /// A toolbar or menu command for the next frame's view.
    commands: Vec<String>,
    /// The caret's line, to tell lines apart that meet at one source offset.
    caret_line: Option<usize>,
    /// The horizontal position kept by Up and Down, on screen.
    want_x: Option<f32>,
    dragging: bool,
    /// Bring the caret into view next frame.
    pub reveal: bool,
    /// Take the keyboard next frame.
    focus: bool,
    /// The view asked for the keyboard itself (its caret is where it wants it).
    ours: bool,
    was_focused: bool,
}

/// The caret's source byte.
pub(crate) fn caret_byte(app: &App) -> usize {
    let d = &app.editor.source.doc;
    d.buffer.char_to_byte(d.selection.primary().head)
}

/// Before the view is drawn: the keyboard's edits of this frame (and those left from the last).
pub(crate) fn begin(app: &mut App, ctx: &egui::Context, id: egui::Id, focused: bool) -> Vec<Edit> {
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(id, egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false });
    });
    let e = &mut app.editor.formatted.editing;
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
/// command acts on it; `None` otherwise.
pub(crate) fn command(app: &mut App, id: &str) -> Option<bool> {
    let path = app.session.current()?.clone();
    if !app.editor.formatted.shows(&path) {
        return None;
    }
    match id {
        "edit.bold" | "edit.italic" | "edit.section" | "edit.subsection" | "edit.subsubsection" | "edit.paragraph" | "edit.itemize"
        | "edit.enumerate" | "edit.undo" | "edit.redo" | "edit.select_all" | "edit.cut" | "edit.copy" => {
            app.editor.formatted.editing.commands.push(id.to_owned());
        }
        "edit.paste" => {
            if let Err(reason) = crate::ui_cmd::enabled(app, id) {
                app.view.status = Some((reason, false));
                return Some(true);
            }
            // The clipboard's text comes back as a paste, which the view takes with the keyboard.
            app.ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        _ => return None,
    }
    app.editor.formatted.editing.focus = true;
    app.ctx.request_repaint();
    Some(true)
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
    let commands = std::mem::take(&mut app.editor.formatted.editing.commands);
    for c in commands {
        if app.editor.source.doc.buffer.version() != version {
            app.editor.formatted.editing.commands.push(c);
            continue;
        }
        run_command(app, &ctx, &f, &c);
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
    let Some((a, h)) = selection(app, &f) else { return };
    paint(app, &ctx, &f, painter, (a, h), focused);
    accessibility(ui, response, path, &f, (a, h));
}

/// The selection's anchor and head in the view.
fn selection(app: &App, f: &Frame<'_>) -> Option<(Pos, Pos)> {
    let d = &app.editor.source.doc;
    let r = d.selection.primary();
    let hint = app.editor.formatted.editing.caret_line;
    let a = f.doc.locate(d.buffer.char_to_byte(r.anchor), hint)?;
    let h = f.doc.locate(d.buffer.char_to_byte(r.head), hint)?;
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
    for (i, d) in f.lines.iter().enumerate() {
        if !(pressed && d.response.is_pointer_button_down_on()) {
            continue;
        }
        let Some(p) = d.response.interact_pointer_pos() else { continue };
        let head = Pos { line: i, at: stop_at(d, p) };
        let anchor = if shift { selection(app, f).map_or(head, |s| s.0) } else { head };
        set_selection(app, f, anchor, head);
        let e = &mut app.editor.formatted.editing;
        e.dragging = true;
        e.want_x = None;
        e.focus = true;
        ctx.request_repaint();
    }
    if !app.editor.formatted.editing.dragging {
        return;
    }
    if !down {
        app.editor.formatted.editing.dragging = false;
        return;
    }
    let Some(p) = ctx.pointer_interact_pos() else { return };
    // The line under the pointer, or the nearest one above or below it.
    let distance = |d: &DrawnLine| {
        let r = d.response.rect;
        if p.y < r.top() { r.top() - p.y } else { (p.y - r.bottom()).max(0.0) }
    };
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
        Edit::Paste(s) => {
            let at = caret_byte(app);
            let source = edit::paste(f.text, at, &s);
            replace(app, f, &source, EditKind::Other);
        }
        Edit::Copy => copy(app, ctx, f, false),
        Edit::Cut => copy(app, ctx, f, true),
        Edit::DeleteSurrounding { before_chars, .. } | Edit::DeleteSurroundingBytes { before_bytes: before_chars, .. } => {
            delete(app, f, before_chars > 0);
        }
        Edit::Preedit { .. } => {}
    }
}

fn run_command(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, id: &str) {
    match id {
        "edit.bold" => style(app, f, "textbf"),
        "edit.italic" => style(app, f, "textit"),
        "edit.section" | "edit.subsection" | "edit.subsubsection" | "edit.paragraph" => {
            let at = caret_byte(app);
            let name = id.trim_start_matches("edit.");
            match edit::set_heading(f.text, at, name) {
                Some(changes) => {
                    let at = map(&changes, at);
                    commit(app, f, &changes, (at, at), EditKind::Other);
                }
                None => say(app, tl!("Put the caret in a paragraph or a heading to make it a heading.")),
            }
        }
        "edit.itemize" | "edit.enumerate" => {
            let at = caret_byte(app);
            match edit::set_list(f.text, at, id.trim_start_matches("edit.")) {
                Some(changes) => {
                    let at = map(&changes, at);
                    commit(app, f, &changes, (at, at), EditKind::Other);
                }
                None => say(app, tl!("Put the caret in a paragraph, or in a list of the other kind.")),
            }
        }
        "edit.undo" | "edit.redo" => undo(app, id == "edit.redo"),
        "edit.select_all" => select_all(app, f),
        "edit.copy" | "edit.cut" => copy(app, ctx, f, id == "edit.cut"),
        _ => {}
    }
}

fn say(app: &mut App, message: &str) {
    app.view.status = Some((message.to_owned(), false));
}

/// Where byte `at` of the text goes once `changes` are made (after what they insert there).
fn map(changes: &[Change], at: usize) -> usize {
    let mut delta: isize = 0;
    for c in changes {
        if c.range.start > at || (c.range.start == at && c.range.end > at) {
            break;
        }
        if at < c.range.end {
            return c.range.start.saturating_add_signed(delta);
        }
        delta += c.text.len() as isize - (c.range.end - c.range.start) as isize;
    }
    at.saturating_add_signed(delta)
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
    let kind = if a == h { EditKind::Typing { at: f.now } } else { EditKind::Other };
    replace(app, f, &edit::escape(s), kind);
}

/// Replaces the selection (or inserts at the caret) with `source`.
fn replace(app: &mut App, f: &Frame<'_>, source: &str, kind: EditKind) {
    let Some((a, h)) = selection(app, f) else { return };
    let pieces = if a == h { Vec::new() } else { f.doc.pieces(f.text, a, h) };
    let mut changes: Vec<Change> = pieces.iter().map(|r| Change { range: r.clone(), text: String::new() }).collect();
    match changes.first_mut() {
        Some(first) => first.text = source.to_owned(),
        None => {
            let at = caret_byte(app);
            changes.push(Change { range: at..at, text: source.to_owned() });
        }
    }
    let caret = changes.first().map_or(0, |c| c.range.start + c.text.len());
    commit(app, f, &changes, (caret, caret), kind);
}

fn enter(app: &mut App, f: &Frame<'_>, edit: Edit) {
    let Some((a, h)) = selection(app, f) else { return };
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
        commit(app, f, &[Change { range: r.clone(), text: String::new() }], (r.start, r.start), EditKind::Deleting { at: f.now });
        return;
    }
    let step = if back { line.prev_stop(h.at) } else { line.next_stop(h.at) };
    if let Some(v) = step {
        // Past what the view adds (a gap before a label).
        set_selection(app, f, Pos { line: h.line, at: v }, Pos { line: h.line, at: v });
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
        let other = if up { h.line.checked_sub(1) } else { Some(h.line + 1) };
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
fn copy(app: &mut App, ctx: &egui::Context, f: &Frame<'_>, cut: bool) {
    let Some((a, h)) = selection(app, f) else { return };
    let (lo, hi) = (a.min(h), a.max(h));
    if lo == hi {
        say(app, tl!("Select some text first."));
        return;
    }
    let mut out = Vec::new();
    for i in lo.line..=hi.line {
        let chars = f.chars(i);
        let from = if i == lo.line { lo.at } else { 0 };
        let to = if i == hi.line { hi.at } else { chars.len() };
        let piece: String = chars.get(from..to.max(from)).unwrap_or_default().iter().collect();
        out.push(piece.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    ctx.copy_text(out.join("\n\n"));
    if cut {
        replace(app, f, "", EditKind::Other);
    }
}

/// Bold or italic on the selection (the word at the caret when nothing is selected): each run's
/// source wrapped in the command, or unwrapped when it is all in one already.
fn style(app: &mut App, f: &Frame<'_>, name: &str) {
    let Some((a, h)) = selection(app, f) else { return };
    let (a, h) = if a == h { word_at(f, h).unwrap_or((a, h)) } else { (a.min(h), a.max(h)) };
    if a == h {
        // Nothing to wrap: an empty command, and the caret in it.
        let at = caret_byte(app);
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
    let (Some(start), Some(end)) = (pieces.first().map(|r| r.start), pieces.last().map(|r| r.end)) else { return };
    // The selection keeps the same text: inside the commands added, without those removed.
    let start = map(&changes, start);
    let end = map_before(&changes, end);
    commit(app, f, &changes, (start, end), EditKind::Other);
}

/// Like [`map`], but before what is inserted at `at`.
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
    if !(h.at > 0 && is_word(h.at - 1) && is_word(h.at)) {
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
    let texts: Vec<String> = f.lines.iter().map(|d| d.galley.text().to_owned()).collect();
    let position = |p: Pos| egui::accesskit::TextPosition { node: run(p.line).accesskit_id(), character_index: p.at };
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::MultilineTextInput);
        node.set_label(name.as_str());
        node.set_value(texts.join("\n"));
        node.set_text_selection(egui::accesskit::TextSelection { anchor: position(a), focus: position(h) });
    });
    let child = ui.new_child(egui::UiBuilder::new().accessibility_parent(id));
    for (i, (d, value)) in f.lines.iter().zip(texts).enumerate() {
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
            let lengths: Vec<u8> = value.chars().map(|c| c.len_utf8() as u8).collect();
            node.set_value(value);
            node.set_character_lengths(lengths);
        });
    }
}
