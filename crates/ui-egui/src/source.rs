//! TeXCraft's source editor widget (docs/plan.md, phase 2, item 3): a [`Buffer`] laid out one
//! galley per line, with the gutter, the caret line's band, selection, search matches and the
//! caret painted around it.
//!
//! - **Input** comes in the order the platform sent it through craft-ui's IME [`Composer`]: typed
//!   and committed text, keys, line breaks, clipboard events and the IME's corrections. The marked
//!   text of a composition is painted at the caret.
//! - **Key bindings** are the standard ones, or Vim's ([`Vim`]) when the setting is on. In Vim
//!   mode, Vim's Ctrl keys are taken from the input before the window's shortcuts see them
//!   ([`SourceEditor::claim_keys`]); the others (Ctrl+S) still run the window's commands.
//! - **Accessibility:** the editor is an editable multi-line text node with its value, caret and
//!   selection, one text run per line. Escape leaves the editor in the standard bindings, and F6
//!   moves between regions in both, so the keyboard is never trapped.
//!
//! The widget knows nothing of projects or sessions: the editor pane hands it a document and reads
//! back whether the text changed and what Vim asked the window to do.

use std::sync::Arc;

use craft_ui::ime::{Composer, Edit};
use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{Align2, Galley, Pos2, Rect, Sense, Vec2, pos2, vec2};
use texcraft_buffer::{Buffer, EditKind, Selection, Transaction};
use texcraft_engine::Severity;
use texcraft_vim::{Ctx, Effect, Key, Mode, Vim};

use crate::icons;
use crate::theme::Tokens;

/// One file's text and where its caret is.
#[derive(Debug, Clone, Default)]
pub(crate) struct Doc {
    pub buffer: Buffer,
    pub selection: Selection,
}

impl Doc {
    pub fn new(text: &str) -> Self {
        Self { buffer: Buffer::new(text), selection: Selection::point(0) }
    }
}

/// Diagnostics by line (from 0): the most severe one's severity and message.
pub(crate) type Markers = std::collections::HashMap<u32, (Severity, String)>;

#[derive(Default)]
struct Layout {
    version: u64,
    width: f32,
    /// The theme's text colour and the source font, so a theme or size change rebuilds the jobs.
    theme: (egui::Color32, egui::FontId),
    /// One per line, rebuilt when the text changes.
    jobs: Vec<LayoutJob>,
    /// Laid out every frame: egui keeps the galleys of unchanged jobs, and they follow its font
    /// atlas, which a kept galley would not.
    galleys: Vec<Arc<Galley>>,
    /// Each line's top, from the text's top; one more entry for the end.
    tops: Vec<f32>,
}

/// Where a frame paints: the editor's rectangle, the text's top left, the gutter's width, the
/// lines shown, and whether the editor has the keyboard.
struct Area {
    rect: Rect,
    origin: Pos2,
    gutter: f32,
    lines: (usize, usize),
    focused: bool,
}

/// What a frame of the editor did.
#[derive(Default)]
pub(crate) struct Output {
    /// The text changed.
    pub changed: bool,
    /// The caret's line and column, from 0.
    pub caret: (usize, usize),
    /// What Vim asked the window to do.
    pub effects: Vec<Effect>,
    /// Ask the language server for completions at this line and column (characters, from 0).
    pub complete_at: Option<(usize, usize)>,
    /// Go to the definition of what is at this line and column (Ctrl+click, F12).
    pub definition_at: Option<(usize, usize)>,
    /// Ask what the thing the pointer rests on is (its line and column).
    pub hover_at: Option<(usize, usize)>,
    /// Ask the language server for the fixes of the problems on this line (from 0).
    pub fixes_for: Option<usize>,
    /// Apply this fix.
    pub apply_fix: Option<texcraft_engine::Fix>,
}

/// A problem in the text: a build's message or the language server's hint, on chars `start..end`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Problem {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub severity: Severity,
    pub message: String,
    pub help: Vec<String>,
    /// The build's own fixes of it (a hint's come from the language server when asked).
    pub fixes: Vec<texcraft_engine::Fix>,
}

/// The popup of a problem: its message, help and fixes.
pub(crate) struct ProblemPopup {
    pub problem: Problem,
    /// Where it opens, on screen.
    at: Pos2,
    /// The fixes, once the editor pane asked the language server (with Fix All last).
    pub fixes: Option<Vec<texcraft_engine::Fix>>,
    /// Opened from the keyboard (Quick Fix): it stays until Escape or a fix, and Up/Down choose.
    pinned: bool,
    selected: usize,
}

/// Completions offered at the caret.
pub(crate) struct CompletionList {
    pub items: Vec<texcraft_engine::Completion>,
    pub selected: usize,
}

/// How long a line gone to stays highlighted, in seconds.
const FLASH_SECONDS: f64 = 1.5;

/// Rows of the completion list shown at once.
const COMPLETION_ROWS: usize = 8;

/// The editor's state across frames (one per editor pane).
pub(crate) struct SourceEditor {
    pub doc: Doc,
    pub vim: Option<Vim>,
    /// Typing an opening bracket adds its closing one, and `\begin{…}` its `\end{…}`.
    pub auto_close: bool,
    composer: Composer,
    layout: Layout,
    /// Where a mouse drag started.
    drag: Option<usize>,
    /// The last click's time and position, and how many clicks it made (word, line).
    clicks: (f64, usize, u8),
    /// The horizontal position kept by Up and Down.
    want_x: Option<f32>,
    /// Bring the caret into view next frame.
    pub reveal: bool,
    /// A scroll asked for by Vim, applied next frame.
    scroll: Option<texcraft_vim::Scroll>,
    /// Keys taken from the input for Vim before the window's shortcuts ran.
    claimed: Vec<egui::Event>,
    /// The first line shown and how many fit.
    view: (usize, usize),
    /// The find bar's matches (chars, in order) and its current one, painted in the text.
    pub matches: Vec<(usize, usize)>,
    pub current_match: Option<(usize, usize)>,
    /// The problems in the text, underlined by severity.
    pub problems: Vec<Problem>,
    pub popup: Option<ProblemPopup>,
    /// Where the popup was last drawn, so the pointer can move into it.
    popup_rect: Option<Rect>,
    apply_fix: Option<texcraft_engine::Fix>,
    /// A line just gone to (from the PDF, the log, the outline) and when, flashed for a moment.
    flash: Option<(usize, f64)>,
    /// The completion list, while it is open.
    pub completion: Option<CompletionList>,
    /// Ask for completions at the end of this frame.
    wants_completion: bool,
    /// The caret's rectangle on screen, for the completion list.
    caret_screen: Option<Rect>,
    /// Where the pointer rests and since when; then what the language server said about it.
    resting: Option<(usize, f64)>,
    pub hover: Option<(usize, String)>,
    /// Go to the definition at this position at the end of the frame.
    wants_definition: Option<usize>,
}

impl Default for SourceEditor {
    fn default() -> Self {
        Self {
            doc: Doc::default(),
            vim: None,
            composer: Composer::new(),
            layout: Layout::default(),
            drag: None,
            clicks: (0.0, 0, 0),
            want_x: None,
            reveal: false,
            scroll: None,
            claimed: Vec::new(),
            view: (0, 40),
            matches: Vec::new(),
            current_match: None,
            problems: Vec::new(),
            popup: None,
            popup_rect: None,
            apply_fix: None,
            flash: None,
            completion: None,
            wants_completion: false,
            caret_screen: None,
            resting: None,
            hover: None,
            wants_definition: None,
            auto_close: true,
        }
    }
}

/// Vim's Ctrl keys, by mode: taken before the window's shortcuts.
fn vim_claims(mode: Mode, key: egui::Key) -> bool {
    use egui::Key as K;
    match mode {
        Mode::Insert | Mode::Replace => matches!(key, K::W | K::U | K::O | K::R | K::H | K::OpenBracket),
        _ => matches!(
            key,
            K::R | K::D | K::U | K::F | K::B | K::O | K::I | K::V | K::A | K::X | K::E | K::Y | K::N | K::P | K::H | K::J | K::OpenBracket
        ),
    }
}

/// An egui key as Vim's, when Vim has a use for it.
fn vim_key(key: egui::Key, modifiers: egui::Modifiers) -> Option<Key> {
    use egui::Key as K;
    if modifiers.command || modifiers.ctrl {
        let name = key.name();
        let c = match key {
            K::OpenBracket => '[',
            _ if name.len() == 1 => name.chars().next()?.to_ascii_lowercase(),
            _ => return None,
        };
        return Some(Key::Ctrl(c));
    }
    Some(match key {
        K::Escape => Key::Esc,
        K::Enter => Key::Enter,
        K::Backspace => Key::Backspace,
        K::Delete => Key::Delete,
        K::Tab => Key::Tab,
        K::ArrowLeft => Key::Left,
        K::ArrowRight => Key::Right,
        K::ArrowUp => Key::Up,
        K::ArrowDown => Key::Down,
        K::Home => Key::Home,
        K::End => Key::End,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
        // Printable keys arrive as text.
        _ => return None,
    })
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl SourceEditor {
    /// Before the window's shortcuts run: in Vim mode, takes Vim's own Ctrl keys from the input
    /// when the editor has the keyboard.
    pub fn claim_keys(&mut self, ctx: &egui::Context, id: egui::Id) {
        let Some(vim) = &self.vim else { return };
        if !ctx.memory(|m| m.has_focus(id)) {
            return;
        }
        let mode = vim.mode();
        let mut claimed = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. }
                    if modifiers.command && !modifiers.shift && !modifiers.alt && vim_claims(mode, *key) =>
                {
                    claimed.push(e.clone());
                    false
                }
                _ => true,
            });
        });
        self.claimed.extend(claimed);
    }

    /// A new document: Vim's marks and commands in progress belong to the old one.
    pub fn set_doc(&mut self, doc: Doc) {
        self.doc = doc;
        self.layout = Layout::default();
        self.drag = None;
        self.want_x = None;
        if let Some(vim) = &mut self.vim {
            vim.switch_buffer();
        }
        self.reveal = true;
    }

    pub fn set_vim(&mut self, on: bool) {
        match (on, self.vim.is_some()) {
            (true, false) => self.vim = Some(Vim::new()),
            (false, true) => self.vim = None,
            _ => {}
        }
    }

    pub fn caret(&self) -> usize {
        match &self.vim {
            Some(v) => v.cursor(),
            None => self.doc.selection.primary().head,
        }
    }

    /// Highlights line `line` (from 0) for a moment, to show where a jump landed.
    pub fn flash_line(&mut self, line: usize, now: f64) {
        self.flash = Some((line, now));
    }

    /// Selects `start..end` and brings it into view.
    pub fn select(&mut self, start: usize, end: usize) {
        let len = self.doc.buffer.len_chars();
        self.doc.selection = Selection::single(start.min(len), end.min(len));
        self.doc.buffer.seal();
        self.reveal = true;
        self.vim_sync();
    }

    fn lay_out(&mut self, ui: &egui::Ui, t: &Tokens, width: f32) {
        let version = self.doc.buffer.version();
        let theme = (t.text, t.code_font());
        let l = &self.layout;
        if l.version != version || (l.width - width).abs() >= 0.5 || l.theme != theme || l.jobs.is_empty() {
            self.layout.jobs = self.jobs(t, width);
            self.layout.version = version;
            self.layout.width = width;
            self.layout.theme = theme;
        }
        let mut galleys = Vec::with_capacity(self.layout.jobs.len());
        let mut tops = Vec::with_capacity(self.layout.jobs.len() + 1);
        let mut y = 0.0;
        ui.fonts_mut(|f| {
            for job in &self.layout.jobs {
                let galley = f.layout_job(job.clone());
                tops.push(y);
                y += galley.size().y.max(t.code_line_height);
                galleys.push(galley);
            }
        });
        tops.push(y);
        self.layout.galleys = galleys;
        self.layout.tops = tops;
    }

    /// One layout job per line, coloured by the LaTeX lexer.
    fn jobs(&self, t: &Tokens, width: f32) -> Vec<LayoutJob> {
        let text = self.doc.buffer.text();
        let font = t.code_font();
        let format = |kind| TextFormat { font_id: font.clone(), color: t.syntax(kind), line_height: Some(t.code_line_height), ..Default::default() };
        let mut jobs = vec![LayoutJob::default()];
        for token in texcraft_latex::lex(&text) {
            let Some(run) = text.get(token.range.clone()) else { continue };
            for (i, piece) in run.split('\n').enumerate() {
                if i > 0 {
                    jobs.push(LayoutJob::default());
                }
                if !piece.is_empty()
                    && let Some(job) = jobs.last_mut()
                {
                    job.append(piece, 0.0, format(token.kind));
                }
            }
        }
        let plain = TextFormat { font_id: font.clone(), color: t.text, line_height: Some(t.code_line_height), ..Default::default() };
        for job in &mut jobs {
            if job.sections.is_empty() {
                job.append("", 0.0, plain.clone());
            }
            job.wrap.max_width = width;
        }
        jobs
    }

    fn line_at_y(&self, y: f32) -> usize {
        let tops = &self.layout.tops;
        tops.partition_point(|top| *top <= y).saturating_sub(1).min(self.layout.galleys.len().saturating_sub(1))
    }

    /// The position under `p` (relative to the text's top left).
    fn pos_at(&self, p: Vec2) -> usize {
        let line = self.line_at_y(p.y);
        let Some(galley) = self.layout.galleys.get(line) else { return 0 };
        let top = self.layout.tops.get(line).copied().unwrap_or(0.0);
        let col: usize = galley.cursor_from_pos(vec2(p.x, p.y - top)).index.into();
        self.doc.buffer.line_to_char(line) + col.min(self.doc.buffer.line(line).chars().count())
    }

    /// The caret's rectangle at `pos` (relative to the text's top left).
    fn rect_at(&self, pos: usize) -> Rect {
        let b = &self.doc.buffer;
        let line = b.char_to_line(pos);
        let col = pos - b.line_to_char(line);
        let top = self.layout.tops.get(line).copied().unwrap_or(0.0);
        match self.layout.galleys.get(line) {
            Some(g) => g.pos_from_cursor(CCursor::new(col)).translate(vec2(0.0, top)),
            None => Rect::from_min_size(pos2(0.0, top), vec2(0.0, 16.0)),
        }
    }

    /// Shows the editor; `id` is its focus and accessibility id.
    pub fn show(&mut self, ui: &mut egui::Ui, t: &Tokens, id: egui::Id, markers: &Markers, name: &str, indent: &str) -> Output {
        let mut out = Output::default();
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(id));
        let vim_mode = self.vim.as_ref().map(Vim::mode);
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: vim_mode.is_some() || self.completion.is_some(),
                },
            );
        });
        let version = self.doc.buffer.version();
        // Input first, so this frame shows its result.
        let edits = self.composer.update(&ctx, focused);
        let claimed = std::mem::take(&mut self.claimed);
        if focused {
            let now = ctx.input(|i| i.time);
            for event in claimed {
                if let egui::Event::Key { key, modifiers, .. } = event {
                    self.key(key, modifiers, now, indent, &ctx, &mut out);
                }
            }
            for edit in edits {
                self.apply(edit, now, indent, &ctx, &mut out);
            }
        }
        out.changed = self.doc.buffer.version() != version;

        let lines = self.doc.buffer.len_lines();
        let digits = lines.to_string().len().max(2) as f32;
        let char_w = ui.fonts_mut(|f| f.glyph_width(&t.code_font(), '0'));
        let gutter = digits * char_w + 9.0 * t.unit;
        let margin = vec2(4.0, 8.0);
        let mut scroll = egui::ScrollArea::vertical().id_salt(id.with("scroll")).auto_shrink([false, false]);
        if let Some(request) = self.scroll.take() {
            let caret_top = self.rect_at(self.caret()).top();
            let (view_h, offset) = ui.ctx().data(|d| d.get_temp::<(f32, f32)>(id.with("view"))).unwrap_or((400.0, 0.0));
            let target = match request {
                texcraft_vim::Scroll::Center => caret_top - view_h / 2.0,
                texcraft_vim::Scroll::Top => caret_top,
                texcraft_vim::Scroll::Bottom => caret_top - view_h + t.code_line_height * 2.0,
                texcraft_vim::Scroll::Lines(n) => offset + n as f32 * t.code_line_height,
            };
            scroll = scroll.vertical_scroll_offset(target.max(0.0));
        }
        scroll.show_viewport(ui, |ui, viewport| {
            // The visible width, less the gutter, the margins and the scroll bar.
            let width = (viewport.width() - gutter - margin.x - 24.0).max(40.0);
            self.lay_out(ui, t, width);
            let height = self.layout.tops.last().copied().unwrap_or(0.0) + margin.y + 40.0;
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height.max(viewport.height())), Sense::hover());
            ui.ctx().data_mut(|d| d.insert_temp(id.with("view"), (viewport.height(), viewport.min.y)));
            let origin = rect.min + vec2(gutter + margin.x, margin.y);
            let response = ui.interact(rect, id, Sense::click_and_drag());
            if response.clicked() || response.drag_started() {
                response.request_focus();
            }
            self.mouse(&response, origin, &ctx);
            let first = self.line_at_y(viewport.min.y - margin.y);
            let last = self.line_at_y(viewport.max.y - margin.y);
            self.view = (first, last.saturating_sub(first).max(1));
            let area = Area { rect, origin, gutter, lines: (first, last), focused };
            self.paint(ui, t, &area, markers);
            let caret = self.rect_at(self.caret()).translate(origin.to_vec2());
            if std::mem::take(&mut self.reveal) {
                ui.scroll_to_rect(caret.expand2(vec2(0.0, t.code_line_height)), None);
            }
            if focused {
                self.composer.set_caret(&ctx, caret);
            }
            self.caret_screen = (focused && ui.clip_rect().intersects(caret)).then_some(caret);
            self.accessibility(ui, &response, id, name, origin);
        });
        if self.caret_screen.is_none() {
            self.completion = None;
        }
        self.paint_completion(&ctx, t, id);
        self.paint_popup(&ctx, t, id);
        if let Some((_, text)) = self.hover.as_ref().filter(|(_, text)| !text.is_empty() && self.popup.is_none())
            && let Some(p) = ctx.pointer_hover_pos()
        {
            let text = text.replace('`', "");
            egui::Area::new(id.with("hover")).order(egui::Order::Tooltip).fixed_pos(p + vec2(12.0, 18.0)).interactable(false).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).fill(t.raised).stroke(t.hairline()).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(egui::RichText::new(text).font(t.small()).color(t.text));
                });
            });
        }
        let caret = self.caret();
        let b = &self.doc.buffer;
        let line = b.char_to_line(caret);
        out.caret = (line, caret - b.line_to_char(line));
        if std::mem::take(&mut self.wants_completion) {
            out.complete_at = Some(out.caret);
        }
        let line_col = |b: &Buffer, p: usize| {
            let l = b.char_to_line(p);
            (l, p - b.line_to_char(l))
        };
        if let Some(p) = self.wants_definition.take() {
            out.definition_at = Some(line_col(&self.doc.buffer, p));
        }
        let now = ctx.input(|i| i.time);
        // Resting on a problem's text opens its popup.
        if let Some((p, since)) = self.resting
            && now - since >= 0.4
            && self.completion.is_none()
            && self.popup.as_ref().is_none_or(|pp| !(pp.pinned || (pp.problem.start <= p && p <= pp.problem.end)))
            && let Some(problem) = self.problems.iter().find(|pr| pr.start <= p && p <= pr.end).cloned()
            && let Some(pointer) = ctx.pointer_hover_pos()
        {
            let fixes = (!problem.fixes.is_empty()).then(|| problem.fixes.clone());
            self.popup = Some(ProblemPopup { problem, at: pointer + vec2(0.0, 14.0), fixes, pinned: false, selected: 0 });
        }
        if let Some(popup) = &self.popup
            && popup.fixes.is_none()
        {
            out.fixes_for = Some(popup.problem.line);
        }
        out.apply_fix = self.apply_fix.take();
        if let Some((p, since)) = self.resting
            && now - since >= 0.6
            && self.hover.is_none()
            && self.completion.is_none()
            && self.popup.is_none()
        {
            out.hover_at = Some(line_col(&self.doc.buffer, p));
            // Asked once per resting place: an empty answer is kept.
            self.hover = Some((p, String::new()));
        }
        out
    }

    // ----- Input -----

    fn apply(&mut self, edit: Edit, now: f64, indent: &str, ctx: &egui::Context, out: &mut Output) {
        match edit {
            Edit::Preedit { .. } => {}
            Edit::Insert(text) => {
                let inserting = self.vim.as_ref().is_none_or(|v| v.mode().is_inserting());
                if self.vim.is_some() {
                    for c in text.chars() {
                        self.vim_key(Key::Char(c), now, indent, ctx, out);
                    }
                } else if self.auto_close && text.chars().count() == 1 {
                    let c = text.chars().next().unwrap_or(' ');
                    self.type_char(c, now, indent);
                } else {
                    self.replace_selection(&text, EditKind::Typing { at: now });
                }
                // A command, a reference, a citation, an environment being typed: ask what fits.
                let trigger = text.chars().last().is_some_and(|c| c.is_alphanumeric() || matches!(c, '\\' | '{' | ',' | ':' | '-' | '_'));
                if inserting && (trigger || self.completion.is_some()) {
                    self.wants_completion = true;
                } else {
                    self.completion = None;
                }
            }
            Edit::LineBreak { modifiers } => self.key(egui::Key::Enter, modifiers, now, indent, ctx, out),
            Edit::Key { key, modifiers, .. } => self.key(key, modifiers, now, indent, ctx, out),
            Edit::Paste(text) => match &self.vim {
                Some(v) if v.mode().is_inserting() => {
                    for c in text.chars() {
                        self.vim_key(Key::Char(c), now, indent, ctx, out);
                    }
                }
                Some(_) => self.vim_key(Key::Ctrl('v'), now, indent, ctx, out),
                None => self.replace_selection(&text, EditKind::Other),
            },
            Edit::Copy | Edit::Cut => {
                if self.vim.is_some() {
                    let key = if edit == Edit::Copy { 'c' } else { 'x' };
                    self.vim_key(Key::Ctrl(key), now, indent, ctx, out);
                    return;
                }
                let r = self.doc.selection.primary();
                if r.is_empty() {
                    return;
                }
                ctx.copy_text(self.doc.buffer.slice(r.start(), r.end()));
                if edit == Edit::Cut {
                    self.replace_selection("", EditKind::Other);
                }
            }
            Edit::DeleteSurrounding { before_chars, after_chars } => {
                let caret = self.caret();
                let len = self.doc.buffer.len_chars();
                self.replace_range(caret.saturating_sub(before_chars), (caret + after_chars).min(len), "", EditKind::Other);
            }
            Edit::DeleteSurroundingBytes { before_bytes, after_bytes } => {
                let caret = self.caret();
                let b = &self.doc.buffer;
                let line = b.char_to_line(caret);
                let (before, after) = (b.slice(b.line_to_char(line), caret), b.slice(caret, b.line_end(line)));
                let (bc, ac) = craft_ui::ime::surrounding_chars(&before, &after, before_bytes, after_bytes);
                self.replace_range(caret - bc, caret + ac, "", EditKind::Other);
            }
        }
    }

    fn key(&mut self, key: egui::Key, modifiers: egui::Modifiers, now: f64, indent: &str, ctx: &egui::Context, out: &mut Output) {
        if self.completion_key(key, modifiers) || self.popup_key(key) {
            return;
        }
        let inserting = self.vim.as_ref().is_none_or(|v| v.mode().is_inserting());
        if key == egui::Key::F12 {
            self.wants_definition = Some(self.caret());
            return;
        }
        if key == egui::Key::Space && modifiers.command && inserting {
            self.wants_completion = true;
            return;
        }
        if key == egui::Key::Backspace && self.completion.is_some() {
            self.wants_completion = true;
        } else {
            self.completion = None;
        }
        if self.vim.is_some() {
            if let Some(k) = vim_key(key, modifiers) {
                self.vim_key(k, now, indent, ctx, out);
            }
            return;
        }
        self.standard_key(key, modifiers, now, indent, ctx);
    }

    fn vim_key(&mut self, key: Key, now: f64, indent: &str, ctx: &egui::Context, out: &mut Output) {
        let Some(vim) = &mut self.vim else { return };
        let (rows, top) = (self.view.1, self.view.0);
        let mut vctx =
            Ctx { buffer: &mut self.doc.buffer, selection: &mut self.doc.selection, now, view_top: top, view_rows: rows, clipboard: None, indent };
        let result = vim.key(key, &mut vctx);
        for effect in result.effects {
            match effect {
                Effect::Copy(text) => ctx.copy_text(text),
                Effect::Scroll(s) => self.scroll = Some(s),
                other => out.effects.push(other),
            }
        }
        self.reveal = true;
    }

    fn replace_range(&mut self, start: usize, end: usize, text: &str, kind: EditKind) {
        let before = self.doc.selection.clone();
        let after = Selection::point(start + text.chars().count());
        if self.doc.buffer.edit(Transaction::replace(start, end, text), &before, &after, kind).is_ok() {
            self.doc.selection = after;
        }
        self.want_x = None;
        self.reveal = true;
    }

    fn replace_selection(&mut self, text: &str, kind: EditKind) {
        let r = self.doc.selection.primary();
        // Typing over a selection is its own undo step.
        if !r.is_empty() {
            self.doc.buffer.seal();
        }
        self.replace_range(r.start(), r.end(), text, if r.is_empty() { kind } else { EditKind::Other });
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        let anchor = if extend { self.doc.selection.primary().anchor } else { pos };
        self.doc.selection = Selection::single(anchor, pos.min(self.doc.buffer.len_chars()));
        self.doc.buffer.seal();
        self.reveal = true;
    }

    fn word_left(&self, pos: usize) -> usize {
        let b = &self.doc.buffer;
        let mut p = pos;
        while p > 0 && b.char_at(p - 1).is_some_and(|c| c.is_whitespace()) {
            p -= 1;
        }
        let word = b.char_at(p.saturating_sub(1)).is_some_and(is_word);
        while p > 0 && b.char_at(p - 1).is_some_and(|c| !c.is_whitespace() && is_word(c) == word) {
            p -= 1;
        }
        p
    }

    fn word_right(&self, pos: usize) -> usize {
        let b = &self.doc.buffer;
        let len = b.len_chars();
        let mut p = pos;
        let word = b.char_at(p).is_some_and(is_word);
        while p < len && b.char_at(p).is_some_and(|c| !c.is_whitespace() && is_word(c) == word) {
            p += 1;
        }
        while p < len && b.char_at(p).is_some_and(|c| c.is_whitespace() && c != '\n') {
            p += 1;
        }
        if p == pos && p < len {
            p += 1;
        }
        p
    }

    /// The position one visual row above or below `pos`, keeping `want_x`.
    fn row_step(&mut self, pos: usize, rows: f32) -> usize {
        let caret = self.rect_at(pos);
        let x = *self.want_x.get_or_insert(caret.left());
        let y = caret.center().y + rows * caret.height().max(1.0);
        let total = self.layout.tops.last().copied().unwrap_or(0.0);
        if y < 0.0 {
            return 0;
        }
        if y > total {
            return self.doc.buffer.len_chars();
        }
        self.pos_at(vec2(x, y))
    }

    fn standard_key(&mut self, key: egui::Key, m: egui::Modifiers, now: f64, indent: &str, ctx: &egui::Context) {
        use egui::Key as K;
        let b = &self.doc.buffer;
        let r = self.doc.selection.primary();
        let pos = r.head;
        let line = b.char_to_line(pos);
        let word = m.command || m.alt;
        let keep_x = matches!(key, K::ArrowUp | K::ArrowDown | K::PageUp | K::PageDown);
        match key {
            K::ArrowLeft | K::ArrowRight => {
                let left = key == K::ArrowLeft;
                let to = if !r.is_empty() && !m.shift {
                    if left { r.start() } else { r.end() }
                } else if word {
                    if left { self.word_left(pos) } else { self.word_right(pos) }
                } else if left {
                    pos.saturating_sub(1)
                } else {
                    (pos + 1).min(b.len_chars())
                };
                self.move_to(to, m.shift);
            }
            K::ArrowUp | K::ArrowDown => {
                let to = self.row_step(pos, if key == K::ArrowUp { -1.0 } else { 1.0 });
                self.move_to(to, m.shift);
            }
            K::PageUp | K::PageDown => {
                let rows = self.view.1.max(2) as f32 - 1.0;
                let to = self.row_step(pos, if key == K::PageUp { -rows } else { rows });
                self.move_to(to, m.shift);
            }
            K::Home => {
                let to = if m.command {
                    0
                } else {
                    // The first non-blank, then the line's start.
                    let start = b.line_to_char(line);
                    let first = start + b.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').count();
                    if pos == first { start } else { first }
                };
                self.move_to(to, m.shift);
            }
            K::End => {
                let to = if m.command { b.len_chars() } else { b.line_end(line) };
                self.move_to(to, m.shift);
            }
            K::Backspace | K::Delete => {
                if !r.is_empty() {
                    self.replace_selection("", EditKind::Other);
                } else if key == K::Backspace && pos > 0 && !word && self.auto_close && self.in_empty_pair(pos) {
                    // Inside a pair the editor added: both go.
                    self.replace_range(pos - 1, pos + 1, "", EditKind::Deleting { at: now });
                } else if key == K::Backspace && pos > 0 {
                    let from = if word { self.word_left(pos) } else { pos - 1 };
                    self.replace_range(from, pos, "", EditKind::Deleting { at: now });
                } else if key == K::Delete && pos < b.len_chars() {
                    let to = if word { self.word_right(pos) } else { pos + 1 };
                    self.replace_range(pos, to, "", EditKind::Deleting { at: now });
                }
            }
            K::Enter => {
                let lead: String = b.line(line).chars().take(pos - b.line_to_char(line)).take_while(|c| *c == ' ' || *c == '\t').collect();
                self.replace_selection(&format!("\n{lead}"), EditKind::Typing { at: now });
            }
            K::Tab => {
                let (first, last) = (b.char_to_line(r.start()), b.char_to_line(r.end().saturating_sub(1).max(r.start())));
                if m.shift || first != last {
                    self.shift_lines(first, last, !m.shift, indent);
                } else {
                    self.replace_selection(indent, EditKind::Typing { at: now });
                }
            }
            K::A if m.command => {
                self.doc.selection = Selection::single(0, b.len_chars());
            }
            K::Z if m.command && !m.shift => self.undo(false),
            K::Z if m.command && m.shift => self.undo(true),
            K::Y if m.command => self.undo(true),
            K::Slash if m.command => self.toggle_comment(),
            _ => {}
        }
        if !keep_x {
            self.want_x = None;
        }
        let _ = ctx;
    }

    fn in_empty_pair(&self, pos: usize) -> bool {
        let b = &self.doc.buffer;
        let (Some(before), Some(after)) = (b.char_at(pos - 1), b.char_at(pos)) else { return false };
        closing(before) == Some(after) && !(pos >= 2 && b.char_at(pos - 2) == Some('\\'))
    }

    /// Types `c` with brackets closed: an opening bracket adds its closing one (or wraps the
    /// selection), a closing bracket steps over the same one after the caret, and finishing
    /// `\begin{name}` adds `\end{name}`.
    fn type_char(&mut self, c: char, now: f64, indent: &str) {
        let r = self.doc.selection.primary();
        let b = &self.doc.buffer;
        let pos = r.head;
        let escaped = pos > 0 && b.char_at(pos - 1) == Some('\\');
        if let Some(close) = closing(c).filter(|_| !escaped) {
            if !r.is_empty() {
                let inner = b.slice(r.start(), r.end());
                let (start, n) = (r.start(), inner.chars().count());
                self.replace_range(start, r.end(), &format!("{c}{inner}{close}"), EditKind::Other);
                self.doc.selection = Selection::single(start + 1, start + 1 + n);
                return;
            }
            let next = b.char_at(pos);
            if next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | '.')) {
                self.replace_range(pos, pos, &format!("{c}{close}"), EditKind::Typing { at: now });
                self.doc.selection = Selection::point(pos + 1);
                return;
            }
        }
        if matches!(c, ')' | ']' | '}') && r.is_empty() && b.char_at(pos) == Some(c) {
            self.doc.selection = Selection::point(pos + 1);
        } else {
            self.replace_selection(&c.to_string(), EditKind::Typing { at: now });
        }
        if c == '}' {
            self.close_environment(indent);
        }
    }

    /// After `\begin{name}` was typed at the caret: adds `\end{name}` on a line of its own, with
    /// an indented line between for the caret, unless the environment is closed further on.
    fn close_environment(&mut self, indent: &str) {
        let b = &self.doc.buffer;
        let pos = self.doc.selection.primary().head;
        let line = b.char_to_line(pos);
        let before = b.slice(b.line_to_char(line), pos);
        let Some(at) = before.rfind("\\begin{") else { return };
        let Some(name) = before[at + 7..].strip_suffix('}') else { return };
        if name.is_empty() || name.contains(['{', '}', '\\']) {
            return;
        }
        // Text right after the caret (an argument, a comment) is left alone.
        if b.slice(pos, b.line_end(line)).trim().is_empty() {
            let rest = b.slice(pos, b.len_chars());
            let (begins, ends) = (rest.matches(&format!("\\begin{{{name}}}")).count(), rest.matches(&format!("\\end{{{name}}}")).count());
            if ends > begins {
                return;
            }
            let lead: String = before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let inner = format!("\n{lead}{indent}");
            let text = format!("{inner}\n{lead}\\end{{{name}}}");
            let caret = pos + inner.chars().count();
            self.replace_range(pos, pos, &text, EditKind::Other);
            self.doc.selection = Selection::point(caret);
        }
    }

    /// The bracket matching the one at `pos`, skipping escaped ones; none past a long way.
    fn matching_bracket(&self, pos: usize) -> Option<usize> {
        const LIMIT: usize = 20_000;
        let b = &self.doc.buffer;
        let c = b.char_at(pos)?;
        let escaped = |p: usize| p > 0 && b.char_at(p - 1) == Some('\\');
        if escaped(pos) {
            return None;
        }
        let (open, close, forward) = if let Some(cl) = closing(c) { (c, cl, true) } else { (opening(c)?, c, false) };
        let mut depth = 0usize;
        let mut p = pos;
        for _ in 0..LIMIT {
            let ch = b.char_at(p)?;
            if !escaped(p) {
                if ch == open {
                    if forward { depth += 1 } else { depth = depth.checked_sub(1)? }
                } else if ch == close {
                    if forward { depth = depth.checked_sub(1)? } else { depth += 1 }
                }
                if depth == 0 {
                    return Some(p);
                }
            }
            if forward {
                p += 1;
            } else {
                p = p.checked_sub(1)?;
            }
        }
        None
    }

    /// The bracket at the caret (or, outside Vim, just before it) and its match.
    fn bracket_pair(&self) -> Option<(usize, usize)> {
        let caret = self.caret();
        let b = &self.doc.buffer;
        let is_bracket = |p: usize| b.char_at(p).is_some_and(|c| closing(c).is_some() || opening(c).is_some());
        let at = if is_bracket(caret) {
            caret
        } else if self.vim.is_none() && caret > 0 && is_bracket(caret - 1) {
            caret - 1
        } else {
            return None;
        };
        Some((at, self.matching_bracket(at)?))
    }

    fn undo(&mut self, redo: bool) {
        let sel = if redo { self.doc.buffer.redo() } else { self.doc.buffer.undo() };
        if let Some(sel) = sel {
            self.doc.selection = sel.clamp(self.doc.buffer.len_chars());
            self.reveal = true;
        }
    }

    fn shift_lines(&mut self, first: usize, last: usize, right: bool, indent: &str) {
        let b = &self.doc.buffer;
        let width = indent.chars().count().max(1);
        let mut changes = Vec::new();
        for l in first..=last {
            let start = b.line_to_char(l);
            let text = b.line(l);
            if right {
                if !text.is_empty() {
                    changes.push(texcraft_buffer::Change { start, end: start, text: indent.to_owned() });
                }
            } else {
                let n = text.chars().take(width).take_while(|c| *c == ' ').count().max(usize::from(text.starts_with('\t')));
                if n > 0 {
                    changes.push(texcraft_buffer::Change { start, end: start + n, text: String::new() });
                }
            }
        }
        let Ok(tx) = Transaction::new(changes) else { return };
        let before = self.doc.selection.clone();
        let after = before.map(&tx);
        if self.doc.buffer.edit(tx, &before, &after, EditKind::Other).is_ok() {
            // Keep whole lines selected.
            let b = &self.doc.buffer;
            self.doc.selection =
                if first == last && before.primary().is_empty() { after } else { Selection::single(b.line_to_char(first), b.line_end(last)) };
        }
    }

    /// Ctrl+/ comments the selected lines out with `%`, or back in when all are comments.
    fn toggle_comment(&mut self) {
        let b = &self.doc.buffer;
        let r = self.doc.selection.primary();
        let (first, last) = (b.char_to_line(r.start()), b.char_to_line(r.end().saturating_sub(1).max(r.start())));
        let lines: Vec<(usize, String)> = (first..=last).map(|l| (l, b.line(l))).collect();
        let all = lines.iter().filter(|(_, t)| !t.trim().is_empty()).all(|(_, t)| t.trim_start().starts_with('%'));
        let mut changes = Vec::new();
        for (l, text) in &lines {
            if text.trim().is_empty() {
                continue;
            }
            let lead = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            let at = b.line_to_char(*l) + lead;
            if all {
                let n = if text.chars().nth(lead + 1) == Some(' ') { 2 } else { 1 };
                changes.push(texcraft_buffer::Change { start: at, end: at + n, text: String::new() });
            } else {
                changes.push(texcraft_buffer::Change { start: at, end: at, text: "% ".to_owned() });
            }
        }
        let Ok(tx) = Transaction::new(changes) else { return };
        let before = self.doc.selection.clone();
        let after = before.map(&tx);
        if self.doc.buffer.edit(tx, &before, &after, EditKind::Other).is_ok() {
            self.doc.selection = after;
        }
    }

    /// Keys for the completion list while it is open; true when the list used the key.
    fn completion_key(&mut self, key: egui::Key, m: egui::Modifiers) -> bool {
        use egui::Key as K;
        let Some(list) = &mut self.completion else { return false };
        if m.command || m.alt {
            return false;
        }
        let n = list.items.len().max(1);
        match key {
            K::ArrowDown => list.selected = (list.selected + 1) % n,
            K::ArrowUp => list.selected = (list.selected + n - 1) % n,
            K::PageDown => list.selected = (list.selected + COMPLETION_ROWS).min(n - 1),
            K::PageUp => list.selected = list.selected.saturating_sub(COMPLETION_ROWS),
            K::Enter | K::Tab => self.accept_completion(),
            K::Escape => self.completion = None,
            _ => return false,
        }
        true
    }

    /// Puts the chosen completion in place of what it completes.
    fn accept_completion(&mut self) {
        let Some(list) = self.completion.take() else { return };
        let Some(item) = list.items.get(list.selected) else { return };
        let caret = self.caret();
        let b = &self.doc.buffer;
        let start = b.line_to_char(b.char_to_line(caret)) + item.from;
        if start > caret {
            return;
        }
        let text = item.text.clone();
        self.replace_range(start, caret, &text, EditKind::Other);
        self.vim_sync();
    }

    /// The editor pane's answer to `Output::complete_at`.
    pub fn offer(&mut self, items: Vec<texcraft_engine::Completion>) {
        let keep = self.completion.as_ref().and_then(|l| l.items.get(l.selected)).map(|i| i.label.clone());
        self.completion = if items.is_empty() {
            None
        } else {
            let selected = keep.and_then(|k| items.iter().position(|i| i.label == k)).unwrap_or(0);
            Some(CompletionList { items, selected })
        };
    }

    /// The completion list under the caret, when it is open.
    fn paint_completion(&mut self, ctx: &egui::Context, t: &Tokens, id: egui::Id) {
        let (Some(list), Some(caret)) = (&self.completion, self.caret_screen) else { return };
        let selected = list.selected;
        let first = selected.saturating_sub(COMPLETION_ROWS - 1);
        let mut chosen = None;
        egui::Area::new(id.with("completion")).order(egui::Order::Foreground).fixed_pos(caret.left_bottom() + vec2(-4.0, 2.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.raised).stroke(t.hairline()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_width(380.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, item) in list.items.iter().enumerate().skip(first).take(COMPLETION_ROWS) {
                    let (rect, r) = ui.allocate_exact_size(vec2(380.0, t.code_line_height + 4.0), Sense::click());
                    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, true, i == selected, &item.label));
                    if i == selected {
                        ui.painter().rect_filled(rect, t.corner(), t.accent_soft);
                    } else if r.hovered() {
                        ui.painter().rect_filled(rect, t.corner(), t.hover);
                    }
                    // One line each: the label, then the detail in what is left, cut with "…".
                    let one_line = |text: &str, font: egui::FontId, color: egui::Color32, width: f32| {
                        let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
                        job.wrap =
                            egui::text::TextWrapping {
                                max_width: width.max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…')
                            };
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let inner = rect.shrink2(vec2(8.0, 0.0));
                    let label = one_line(&item.label, t.code_font(), t.text, inner.width() * 0.6);
                    let gap = 12.0;
                    let detail = one_line(&item.detail, t.small(), t.text_muted, inner.width() - label.size().x - gap);
                    let painter = ui.painter_at(rect);
                    painter.galley(egui::pos2(inner.left(), rect.center().y - label.size().y / 2.0), label, t.text);
                    painter.galley(egui::pos2(inner.right() - detail.size().x, rect.center().y - detail.size().y / 2.0), detail, t.text_muted);
                    let r = if item.detail.is_empty() { r } else { r.on_hover_text(&item.detail) };
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
                if list.items.len() > COMPLETION_ROWS {
                    ui.label(
                        egui::RichText::new(crate::i18n::trn(list.items.len(), "{n} completion", "{n} completions"))
                            .font(t.small())
                            .color(t.text_muted),
                    );
                }
                // The highlighted entry, announced as it changes.
                if let Some(item) = list.items.get(selected) {
                    let text = format!("{} {}", item.label, item.detail);
                    let (rect, _) = ui.allocate_exact_size(vec2(0.0, 0.0), Sense::hover());
                    let r = ui.interact(rect, id.with("completion-live"), Sense::hover());
                    crate::status::announce(ui, &r, text.trim());
                }
            });
        });
        if let Some(i) = chosen {
            if let Some(list) = &mut self.completion {
                list.selected = i;
            }
            self.accept_completion();
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }

    /// Opens the popup of the problem at the caret, or the first one on its line (Quick Fix,
    /// Cmd+.); false when there is none.
    pub fn quick_fix(&mut self) -> bool {
        let caret = self.caret();
        let line = self.doc.buffer.char_to_line(caret);
        let found =
            self.problems.iter().find(|p| p.start <= caret && caret <= p.end).or_else(|| self.problems.iter().find(|p| p.line == line)).cloned();
        let Some(problem) = found else { return false };
        let at = self.caret_screen.map_or(Pos2::ZERO, |c| c.left_bottom() + vec2(0.0, 4.0));
        let fixes = (!problem.fixes.is_empty()).then(|| problem.fixes.clone());
        self.popup = Some(ProblemPopup { problem, at, fixes, pinned: true, selected: 0 });
        true
    }

    /// Keys for a popup opened from the keyboard; true when it used the key.
    fn popup_key(&mut self, key: egui::Key) -> bool {
        use egui::Key as K;
        let Some(popup) = self.popup.as_mut().filter(|p| p.pinned) else { return false };
        let n = popup.fixes.as_ref().map_or(0, Vec::len);
        match key {
            K::ArrowDown if n > 0 => popup.selected = (popup.selected + 1) % n,
            K::ArrowUp if n > 0 => popup.selected = (popup.selected + n - 1) % n,
            K::Enter | K::Tab if n > 0 => {
                self.apply_fix = popup.fixes.as_ref().and_then(|f| f.get(popup.selected)).cloned();
                self.popup = None;
            }
            K::Escape => self.popup = None,
            _ => {
                self.popup = None;
                return false;
            }
        }
        true
    }

    /// The problem popup: the message, its help, and a button per fix.
    fn paint_popup(&mut self, ctx: &egui::Context, t: &Tokens, id: egui::Id) {
        let Some(popup) = &self.popup else {
            self.popup_rect = None;
            return;
        };
        // A popup the pointer opened closes when the pointer leaves both the text and the popup.
        if !popup.pinned
            && let Some(p) = ctx.pointer_hover_pos()
        {
            let in_popup = self.popup_rect.is_some_and(|r| r.expand(8.0).contains(p));
            let on_text = self.resting.is_some_and(|(at, _)| popup.problem.start <= at && at <= popup.problem.end);
            if !in_popup && !on_text {
                self.popup = None;
                self.popup_rect = None;
                return;
            }
        }
        let mut chosen = None;
        let mut close = false;
        let (icon, color) = match popup.problem.severity {
            Severity::Error => ("circle-exclamation-point", t.error),
            Severity::Warning => ("triangle-exclamation-point", t.warning),
            _ => ("info", t.note),
        };
        let area = egui::Area::new(id.with("problem")).order(egui::Order::Foreground).fixed_pos(popup.at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.raised).stroke(t.hairline()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_max_width(440.0);
                ui.horizontal(|ui| {
                    ui.add(crate::icons::image(icon, 16.0, color));
                    let label = ui.add(egui::Label::new(egui::RichText::new(&popup.problem.message).font(t.strong()).color(t.text)).wrap());
                    crate::status::announce(ui, &label, &popup.problem.message);
                });
                for help in &popup.problem.help {
                    ui.label(egui::RichText::new(help).font(t.small()).color(t.text_muted));
                }
                match &popup.fixes {
                    Some(fixes) if !fixes.is_empty() => {
                        ui.add_space(t.unit * 2.0);
                        for (i, fix) in fixes.iter().enumerate() {
                            let all = fix.problem.is_none();
                            let text = if all { tl!("Fix all hints in this file") } else { fix.title.as_str() };
                            let selected = popup.pinned && i == popup.selected;
                            let button =
                                egui::Button::image_and_text(crate::icons::image(if all { "list" } else { "pencil" }, 14.0, t.text_muted), text)
                                    .fill(if selected { t.accent_soft } else { t.surface })
                                    .stroke(if selected { egui::Stroke::new(1.0, t.accent) } else { t.hairline() });
                            if ui.add(button).clicked() {
                                chosen = Some(fix.clone());
                            }
                        }
                    }
                    Some(_) => {}
                    None => {
                        ui.label(egui::RichText::new(tl!("Looking for fixes…")).font(t.small()).color(t.text_muted));
                    }
                }
                if popup.pinned {
                    ui.add_space(t.unit);
                    let hint = tl!("Up and Down choose a fix, Enter applies it, Escape closes.");
                    ui.label(egui::RichText::new(hint).font(t.small()).color(t.text_muted));
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                }
            });
        });
        self.popup_rect = Some(area.response.rect);
        if let Some(fix) = chosen {
            self.apply_fix = Some(fix);
            self.popup = None;
            ctx.memory_mut(|m| m.request_focus(id));
        } else if close {
            self.popup = None;
        }
    }

    /// Makes a fix's edits on the buffer, as one undoable edit.
    pub fn apply_fix(&mut self, fix: &texcraft_engine::Fix) {
        let b = &self.doc.buffer;
        let at = |line: usize, col: usize| {
            let line = line.min(b.len_lines().saturating_sub(1));
            (b.line_to_char(line) + col).min(b.line_end(line))
        };
        let changes = fix
            .edits
            .iter()
            .map(|e| texcraft_buffer::Change { start: at(e.line, e.column), end: at(e.end_line, e.end_column), text: e.text.clone() })
            .collect();
        let Ok(tx) = Transaction::new(changes) else { return };
        let before = self.doc.selection.clone();
        let after = before.map(&tx);
        if self.doc.buffer.edit(tx, &before, &after, EditKind::Other).is_ok() {
            self.doc.buffer.seal();
            self.doc.selection = after;
            self.vim_sync();
        }
    }

    /// Notes where the pointer rests on the text, for the language server's description.
    fn rest(&mut self, response: &egui::Response, origin: Pos2, ctx: &egui::Context) {
        let Some(p) = response.hover_pos().filter(|_| !response.dragged()) else {
            self.resting = None;
            self.hover = None;
            return;
        };
        let pos = self.pos_at(p - origin);
        let now = ctx.input(|i| i.time);
        match self.resting {
            Some((at, _)) if at == pos => {}
            _ => {
                self.resting = Some((pos, now));
                if self.hover.as_ref().is_some_and(|(at, _)| *at != pos) {
                    self.hover = None;
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(400));
            }
        }
    }

    /// Vim takes a selection the mouse made at once, so its caret shows where the click was.
    fn vim_sync(&mut self) {
        let Some(vim) = &mut self.vim else { return };
        let (top, rows) = self.view;
        let mut vctx = Ctx {
            buffer: &mut self.doc.buffer,
            selection: &mut self.doc.selection,
            now: 0.0,
            view_top: top,
            view_rows: rows,
            clipboard: None,
            indent: "  ",
        };
        vim.sync(&mut vctx);
    }

    fn mouse(&mut self, response: &egui::Response, origin: Pos2, ctx: &egui::Context) {
        // Ctrl+click (Cmd+click) goes to the definition of what is clicked.
        if response.clicked()
            && ctx.input(|i| i.modifiers.command)
            && let Some(p) = response.interact_pointer_pos()
        {
            self.wants_definition = Some(self.pos_at(p - origin));
            return;
        }
        self.rest(response, origin, ctx);
        let before = self.doc.selection.clone();
        self.mouse_select(response, origin, ctx);
        if self.doc.selection != before {
            self.completion = None;
            self.vim_sync();
        }
    }

    fn mouse_select(&mut self, response: &egui::Response, origin: Pos2, ctx: &egui::Context) {
        let Some(p) = response.interact_pointer_pos() else {
            self.drag = None;
            return;
        };
        let pos = self.pos_at(p - origin);
        let shift = ctx.input(|i| i.modifiers.shift);
        if response.drag_started() || response.clicked() {
            let now = ctx.input(|i| i.time);
            let (last_time, last_pos, count) = self.clicks;
            let count = if now - last_time < 0.4 && last_pos == pos { count % 3 + 1 } else { 1 };
            self.clicks = (now, pos, count);
            let b = &self.doc.buffer;
            let (start, end) = match count {
                2 => {
                    let mut s = pos;
                    while s > 0 && b.char_at(s - 1).is_some_and(is_word) {
                        s -= 1;
                    }
                    let mut e = pos;
                    while b.char_at(e).is_some_and(is_word) {
                        e += 1;
                    }
                    (s, e)
                }
                3 => {
                    let line = b.char_to_line(pos);
                    (b.line_to_char(line), (b.line_end(line) + 1).min(b.len_chars()))
                }
                _ => (pos, pos),
            };
            if shift && count == 1 {
                self.move_to(pos, true);
            } else {
                self.doc.selection = Selection::single(start, end);
                self.doc.buffer.seal();
            }
            self.drag = Some(self.doc.selection.primary().anchor);
            self.want_x = None;
        } else if response.dragged()
            && let Some(anchor) = self.drag
        {
            self.doc.selection = Selection::single(anchor, pos);
        }
    }

    // ----- Painting -----

    fn paint(&self, ui: &egui::Ui, t: &Tokens, area: &Area, markers: &Markers) {
        let Area { rect, origin, gutter, lines: (first, last), focused } = *area;
        let painter = ui.painter();
        let b = &self.doc.buffer;
        let caret = self.caret();
        let caret_line = b.char_to_line(caret);
        let font = egui::FontId::new(t.code_size - 1.5, egui::FontFamily::Monospace);
        let number_right = rect.left() + gutter - 3.0 * t.unit;
        let sel = self.doc.selection.primary();
        let highlight = self.vim.as_ref().and_then(Vim::highlighted);
        for line in first..=last.min(self.layout.galleys.len().saturating_sub(1)) {
            let (Some(galley), Some(top)) = (self.layout.galleys.get(line), self.layout.tops.get(line)) else { continue };
            let bottom = self.layout.tops.get(line + 1).copied().unwrap_or(*top);
            let y0 = origin.y + top;
            let band = Rect::from_min_max(pos2(rect.left(), y0), pos2(rect.right(), origin.y + bottom));
            if matches!(markers.get(&(line as u32)), Some((Severity::Error, _))) {
                painter.rect_filled(band, 0.0, t.error_line);
            } else if focused && line == caret_line && sel.is_empty() {
                painter.rect_filled(band, 0.0, t.current_line);
            }
            let line_start = b.line_to_char(line);
            let line_len = galley.rows.iter().map(|r| r.char_count_excluding_newline().0).sum::<usize>();
            // A line just gone to, fading.
            if let Some((flashed, since)) = self.flash
                && flashed == line
            {
                let now = ui.input(|i| i.time);
                let fade = 1.0 - ((now - since) / FLASH_SECONDS) as f32;
                if fade > 0.0 {
                    painter.rect_filled(band, 0.0, t.search_match.gamma_multiply(fade.clamp(0.0, 1.0)));
                    ui.ctx().request_repaint();
                }
            }
            // The find bar's matches.
            let line_end = line_start + line_len;
            let from = self.matches.partition_point(|(_, e)| *e <= line_start);
            for (ms, me) in self.matches[from..].iter().take_while(|(s, _)| *s <= line_end) {
                let s = ms.saturating_sub(line_start).min(line_len);
                let e = me.saturating_sub(line_start).min(line_len);
                let color = if self.current_match == Some((*ms, *me)) { t.selection } else { t.search_match };
                paint_span(painter, galley, origin + vec2(0.0, *top), (s, e.max(s)), color, *me > line_end);
            }
            // Vim's search matches.
            if let Some(re) = highlight.filter(|_| self.matches.is_empty()) {
                let text = b.line(line);
                for m in re.find_iter(&text) {
                    let (s, e) = (text[..m.start()].chars().count(), text[..m.end()].chars().count());
                    paint_span(painter, galley, origin + vec2(0.0, *top), (s, e.max(s + 1).min(line_len.max(s + 1))), t.search_match, false);
                }
            }
            // The selection.
            if !sel.is_empty() && sel.start() <= line_start + line_len && sel.end() > line_start {
                let s = sel.start().saturating_sub(line_start);
                let e = (sel.end() - line_start).min(line_len);
                let through_break = sel.end() > line_start + line_len;
                paint_span(painter, galley, origin + vec2(0.0, *top), (s, e), t.selection, through_break);
            }
            painter.galley(origin + vec2(0.0, *top), galley.clone(), t.text);
            // Problems, underlined: hints with dots, warnings and errors with waves.
            for p in self.problems.iter().filter(|p| p.end > line_start && p.start <= line_start + line_len) {
                let (s, e) = (p.start.saturating_sub(line_start).min(line_len), (p.end - line_start).min(line_len));
                let span = (s, e.max(s + 1).min(line_len.max(s + 1)));
                let at = origin + vec2(0.0, *top);
                match p.severity {
                    Severity::Error => underline(painter, galley, at, span, t.error, true),
                    Severity::Warning => underline(painter, galley, at, span, t.warning, true),
                    _ => underline(painter, galley, at, span, t.note, false),
                }
            }
            // The gutter.
            let first_row = galley.rows.first().map_or(t.code_line_height, |r| r.max_y());
            let mid = y0 + first_row / 2.0;
            let color = if line == caret_line { t.text } else { t.text_muted };
            painter.text(pos2(number_right, mid), Align2::RIGHT_CENTER, (line + 1).to_string(), font.clone(), color);
            if let Some((severity, message)) = markers.get(&(line as u32)) {
                let (icon, color) = match severity {
                    Severity::Error => ("circle-exclamation-point", t.error),
                    Severity::Warning => ("triangle-exclamation-point", t.warning),
                    _ => ("info", t.note),
                };
                let r = Rect::from_center_size(pos2(rect.left() + 2.5 * t.unit, mid), vec2(14.0, 14.0));
                icons::paint(ui, r, icon, 13.0, color);
                let hover = ui.interact(r, ui.scope_id().with(("marker", line)), Sense::hover());
                let _ = hover.on_hover_text(message.as_str());
            }
        }
        // The bracket at the caret and its match, outlined.
        if focused && let Some((a, z)) = self.bracket_pair() {
            for p in [a, z] {
                let r = self.rect_at(p).translate(origin.to_vec2());
                let w = ui.fonts_mut(|f| f.glyph_width(&t.code_font(), b.char_at(p).unwrap_or(' ')));
                painter.rect_stroke(
                    Rect::from_min_size(r.min, vec2(w, r.height())),
                    1.0,
                    egui::Stroke::new(1.0, t.text_muted),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // Without the keyboard, Vim's caret stays visible as an outline.
        if !focused && self.vim.as_ref().is_some_and(|v| !v.mode().is_inserting()) {
            let c = self.rect_at(caret).translate(origin.to_vec2());
            let w = ui.fonts_mut(|f| f.glyph_width(&t.code_font(), b.char_at(caret).filter(|c| *c != '\n').unwrap_or(' ')));
            painter.rect_stroke(
                Rect::from_min_size(c.min, vec2(w.max(4.0), c.height())),
                1.0,
                egui::Stroke::new(1.0, t.text_muted),
                egui::StrokeKind::Inside,
            );
        }
        // The caret, and the marked text of a composition.
        if focused {
            let c = self.rect_at(caret).translate(origin.to_vec2());
            let mode = self.vim.as_ref().map(Vim::mode);
            match mode {
                Some(m) if !m.is_inserting() => {
                    // A solid block, its character drawn over it in reverse.
                    let under = b.char_at(caret).filter(|c| *c != '\n');
                    let w = ui.fonts_mut(|f| f.glyph_width(&t.code_font(), under.unwrap_or(' ')));
                    let block = Rect::from_min_size(c.min, vec2(w.max(4.0), c.height()));
                    let _ = painter.rect_filled(block, 1.0, t.caret_block);
                    if let Some(ch) = under {
                        painter.text(pos2(block.left(), block.center().y), Align2::LEFT_CENTER, ch.to_string(), t.code_font(), t.caret_text);
                    }
                }
                Some(Mode::Replace) => {
                    let _ = painter.rect_filled(Rect::from_min_max(pos2(c.left(), c.bottom() - 2.0), pos2(c.left() + 8.0, c.bottom())), 0.0, t.text);
                }
                _ => {
                    let _ = painter.rect_filled(Rect::from_min_size(c.min, vec2(2.0, c.height())), 0.0, t.text);
                }
            }
            if let Some(pre) = self.composer.preedit() {
                let g = ui.fonts_mut(|f| f.layout_no_wrap(pre.text.to_owned(), t.code_font(), t.text));
                let r = Rect::from_min_size(c.min, g.size());
                painter.rect_filled(r, 0.0, t.surface);
                painter.galley(c.min, g, t.text);
                painter.hline(r.x_range(), r.bottom(), egui::Stroke::new(1.0, t.text));
            }
        }
    }

    // ----- Accessibility -----

    fn accessibility(&self, ui: &mut egui::Ui, response: &egui::Response, id: egui::Id, name: &str, origin: Pos2) {
        let enabled = ui.ctx().accesskit_node_builder(id, |_| ()).is_some();
        if !enabled {
            return;
        }
        let b = &self.doc.buffer;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, name));
        let run = |line: usize| id.with(("line", line));
        let position = |pos: usize| {
            let line = b.char_to_line(pos);
            egui::accesskit::TextPosition { node: run(line).accesskit_id(), character_index: pos - b.line_to_char(line) }
        };
        let sel = self.doc.selection.primary();
        let mode = self.vim.as_ref().map(Vim::mode);
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::MultilineTextInput);
            node.set_label(name);
            node.set_value(b.text());
            let (anchor, focus) = match mode {
                Some(m) if !m.is_inserting() && !m.is_visual() => (self.caret(), self.caret()),
                _ => (sel.anchor, sel.head),
            };
            node.set_text_selection(egui::accesskit::TextSelection { anchor: position(anchor), focus: position(focus) });
        });
        let child = ui.new_child(egui::UiBuilder::new().accessibility_parent(id));
        for line in 0..b.len_lines() {
            let mut value = b.line(line);
            if line + 1 < b.len_lines() {
                value.push('\n');
            }
            let top = self.layout.tops.get(line).copied().unwrap_or(0.0);
            let bottom = self.layout.tops.get(line + 1).copied().unwrap_or(top);
            let width = self.layout.galleys.get(line).map_or(0.0, |g| g.size().x);
            // A widget of a child of the editor's node, so the run is the editor's child.
            let line_rect = Rect::from_min_max(pos2(origin.x, origin.y + top), pos2(origin.x + width.max(1.0), origin.y + bottom.max(top + 1.0)));
            let _ = child.interact(line_rect, run(line), Sense::hover());
            child.ctx().accesskit_node_builder(run(line), |node| {
                node.set_role(egui::accesskit::Role::TextRun);
                node.set_bounds(egui::accesskit::Rect {
                    x0: origin.x.into(),
                    y0: (origin.y + top).into(),
                    x1: (origin.x + width).into(),
                    y1: (origin.y + bottom).into(),
                });
                let lengths: Vec<u8> = value.chars().map(|c| c.len_utf8() as u8).collect();
                node.set_value(value);
                node.set_character_lengths(lengths);
            });
        }
    }
}

/// Paints chars `s..e` of one line's galley (`through_break`: and its line break).
fn paint_span(painter: &egui::Painter, galley: &Galley, at: Pos2, (s, e): (usize, usize), color: egui::Color32, through_break: bool) {
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (a, z) = (s.max(row_start), e.min(row_start + n));
        let last_row = row_start + n >= galley.rows.iter().map(|r| r.char_count_excluding_newline().0).sum::<usize>();
        if a < z || (through_break && last_row && e >= row_start + n) {
            let x0 = row.pos.x + row.x_offset(egui::text::CharIndex(a - row_start));
            let mut x1 = row.pos.x + row.x_offset(egui::text::CharIndex(z.max(a) - row_start));
            if through_break && last_row {
                x1 += 6.0;
            }
            let r = Rect::from_min_max(pos2(x0, row.min_y()), pos2(x1, row.max_y())).translate(at.to_vec2());
            painter.rect_filled(r, 0.0, color);
        }
        row_start += n;
    }
}

/// The closing bracket of an opening one.
fn closing(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

/// The opening bracket of a closing one.
fn opening(c: char) -> Option<char> {
    match c {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// A dotted (or, `wavy`, a wavy) line under chars `s..e` of one line's galley.
fn underline(painter: &egui::Painter, galley: &Galley, at: Pos2, (s, e): (usize, usize), color: egui::Color32, wavy: bool) {
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (a, z) = (s.max(row_start), e.min(row_start + n));
        if a < z {
            let x0 = at.x + row.pos.x + row.x_offset(egui::text::CharIndex(a - row_start));
            let x1 = at.x + row.pos.x + row.x_offset(egui::text::CharIndex(z - row_start));
            let y = at.y + row.max_y() - 1.5;
            if wavy {
                let mut points = Vec::new();
                let mut x = x0;
                let mut up = true;
                while x <= x1 {
                    points.push(pos2(x, if up { y - 1.2 } else { y + 1.2 }));
                    x += 2.5;
                    up = !up;
                }
                painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, color)));
            } else {
                let mut x = x0;
                while x < x1 {
                    painter.circle_filled(pos2(x + 1.0, y), 1.0, color);
                    x += 3.5;
                }
            }
        }
        row_start += n;
    }
}
