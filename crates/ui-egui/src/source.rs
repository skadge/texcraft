//! TeXCraft's source editor widget (docs/ROADMAP.md): a [`Buffer`] laid out one
//! galley per line, with the gutter, the caret line's band, selection, search matches and the
//! caret painted around it.
//!
//! - **Input** comes in the order the platform sent it through craft-ui's IME [`Composer`]: typed
//!   and committed text, keys, line breaks, clipboard events and the IME's corrections. The marked
//!   text of a composition is painted at the caret.
//! - **Key bindings** are the standard ones, or Vim's ([`Vim`]) when the setting is on. In Vim
//!   mode, Vim's Ctrl keys are taken from the input before the window's shortcuts see them
//!   ([`SourceEditor::claim_keys`]); the others (Ctrl+S) still run the window's commands.
//! - **Folding:** environments, sections and the preamble fold ([`texcraft_latex::fold`]) from
//!   their gutter marker, the Fold commands or Vim's `z` keys. A folded region keeps its first line,
//!   with a "…" chip, and its other lines take no room; folds follow edits through buffer marks,
//!   and open when the caret lands inside or the region's structure breaks.
//! - **Several carets** (outside Vim mode): Alt+click adds or removes a caret, Cmd+D selects the
//!   word at the caret and then each next occurrence of it. Typing, Backspace, Delete, Enter, Tab
//!   and the arrows, Home and End act at every caret, an edit at all of them undone as one; Escape
//!   returns to one caret. Other commands act on the primary caret.
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
use texcraft_buffer::{Bias, Buffer, Change, EditKind, MarkId, Range, Selection, Transaction};
use texcraft_engine::Severity;
use texcraft_latex::fold::{FoldKind, FoldRegion};
use texcraft_vim::{Ctx, Effect, Fold, Key, Mode, Vim};

use crate::icons;
use crate::theme::Tokens;

/// One file's text, where its caret is, and what of it is folded.
#[derive(Debug, Clone, Default)]
pub(crate) struct Doc {
    pub buffer: Buffer,
    pub selection: Selection,
    /// The folded regions: a mark at the start of each one's first line, which follows edits, and
    /// what the region is. A fold whose region is no longer there (its structure broke) unfolds.
    folds: Vec<(MarkId, FoldKind)>,
}

impl Doc {
    pub fn new(text: &str) -> Self {
        Self { buffer: Buffer::new(text), selection: Selection::point(0), folds: Vec::new() }
    }
}

/// Whether a fold of `kind` still stands for `region`: the same environment, or a heading of the
/// same level (its title may change), or the preamble.
fn same_kind(fold: &FoldKind, region: &FoldKind) -> bool {
    match (fold, region) {
        (FoldKind::Section { level: a, .. }, FoldKind::Section { level: b, .. }) => a == b,
        _ => fold == region,
    }
}

/// What a region is, for its fold marker's name and the announcements.
fn describe(kind: &FoldKind) -> String {
    match kind {
        FoldKind::Section { title, .. } => tl!("section {title}").replace("{title}", title),
        FoldKind::Environment(name) => tl!("environment {name}").replace("{name}", name),
        FoldKind::Preamble => tl!("the preamble").to_owned(),
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
    /// Run this command of a spelling or grammar popup (Add to Dictionary, Ignore, Turn Off This
    /// Rule) on its problem.
    pub popup_action: Option<(&'static str, Problem)>,
    /// Explain the build's message on this line (from 0) with AI.
    pub explain: Option<usize>,
    /// Cancel the AI request in flight.
    pub cancel_ai: bool,
    /// The next words shown after the caret were taken with Tab (true) or dismissed with Escape.
    pub ghost: Option<bool>,
    /// What was being dragged was let go over the text, at this position.
    pub dropped_at: Option<usize>,
    /// Something to tell the user (and announce): a fold or an unfold.
    pub announce: Option<String>,
    /// The text's response, for its context menu.
    pub response: Option<egui::Response>,
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
    /// A build's message (AI can explain it), not the language server's hint.
    pub build: bool,
    /// A misspelled word: its fixes are the dictionary's spellings.
    pub spelling: bool,
    /// A grammar problem: its fixes are its replacements.
    pub grammar: Option<GrammarMark>,
}

/// What a grammar problem is, for Ignore and Turn Off This Rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrammarMark {
    /// As `grammar.ignore` takes it.
    pub id: String,
    /// Harper's rule; `None` for one the model found.
    pub rule: Option<String>,
}

/// How a problem is underlined: the shape tells kinds apart as well as the colour.
#[derive(Clone, Copy)]
enum Line {
    Wavy,
    Dotted,
    Double,
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
    /// For a build message, when a model is connected: its explanation, if asked for, and whether
    /// a request is in flight. The editor pane sets it every frame.
    pub ai: Option<(Option<texcraft_engine::ai::Explanation>, Option<String>)>,
}

/// Replacements a spelling or grammar popup offers, at most.
pub(crate) const POPUP_FIXES: usize = 5;

/// The fixes a popup lists: a spelling or grammar problem's first [`POPUP_FIXES`], the others'
/// all.
fn shown_fixes(popup: &ProblemPopup) -> Vec<texcraft_engine::Fix> {
    let fixes = popup.fixes.clone().unwrap_or_default();
    if popup.problem.spelling || popup.problem.grammar.is_some() { fixes.into_iter().take(POPUP_FIXES).collect() } else { fixes }
}

/// The commands a spelling or grammar popup offers after its replacements, with their labels.
fn popup_actions(problem: &Problem) -> Vec<(&'static str, &'static str)> {
    if problem.spelling {
        return vec![("edit.spelling.add", tl!("Add to Dictionary")), ("edit.spelling.ignore", tl!("Ignore"))];
    }
    match &problem.grammar {
        Some(mark) => {
            let mut v = vec![("edit.grammar.ignore", tl!("Ignore"))];
            if mark.rule.is_some() {
                v.push(("edit.grammar.rule_off", tl!("Turn Off This Rule")));
            }
            v
        }
        None => Vec::new(),
    }
}

/// How a passage of the review is drawn. Struck and underlined text tell a suggestion apart
/// without its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mark {
    /// A commented passage: tinted.
    #[default]
    Comment,
    /// Text a suggestion deletes or replaces: struck through.
    Deleted,
    /// Text a suggestion puts in, shown though not in the file: underlined.
    Inserted,
}

/// A passage of the review: tinted under the text in its author's colour, more strongly when its
/// thread is selected, and marked in the gutter; a suggestion's text is also struck through or
/// underlined ([`Mark`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Passage {
    pub start: usize,
    pub end: usize,
    pub color: egui::Color32,
    pub selected: bool,
    pub mark: Mark,
}

/// Completions offered at the caret.
pub(crate) struct CompletionList {
    pub items: Vec<texcraft_engine::Completion>,
    pub selected: usize,
}

/// How long a line gone to stays highlighted, in seconds.
const FLASH_SECONDS: f64 = 1.5;

/// How long the window may take to answer Edit › Paste with the clipboard's text, in seconds.
const PASTE_SECONDS: f64 = 2.0;

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
    popup_action: Option<(&'static str, Problem)>,
    /// What the popup's AI part asked for: explain (the message's line) or cancel.
    ai_act: Option<Option<usize>>,
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
    /// The commented passages, in order.
    pub passages: Vec<Passage>,
    /// Where the text was drawn last: its top left, and the part of the editor shown, on screen.
    shown: Option<(Pos2, Rect)>,
    /// Something the editor takes is being dragged: show where it would go, and say where it was
    /// let go ([`Output::dropped_at`]).
    pub accept_drop: bool,
    /// When the window was asked for the clipboard's text (Edit › Paste): the paste that comes
    /// back inserts it, even in Vim's Normal mode, where Ctrl+V's paste is Vim's own key.
    paste_asked: Option<f64>,
    /// The text's foldable regions and the text version they are of.
    regions: (Option<u64>, Vec<FoldRegion>),
    /// The folded regions this frame, as lines (first, last), by their first line, with each
    /// one's mark.
    folded: Vec<(usize, usize, MarkId)>,
    /// Per line: hidden in a fold.
    hidden: Vec<bool>,
    /// A fold or unfold to announce, for [`Output::announce`].
    announce: Option<String>,
    /// The next words a model suggests (AI3), drawn in grey after the caret at this position:
    /// Tab takes them, Escape dismisses them. The editor pane sets them every frame.
    pub ghost: Option<(usize, String)>,
    /// The selection last put in the primary selection (X11, Wayland), so it goes once it settles.
    primary_sent: Option<(usize, usize)>,
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
            popup_action: None,
            ai_act: None,
            flash: None,
            completion: None,
            wants_completion: false,
            caret_screen: None,
            resting: None,
            hover: None,
            wants_definition: None,
            auto_close: true,
            passages: Vec::new(),
            shown: None,
            accept_drop: false,
            paste_asked: None,
            regions: (None, Vec::new()),
            folded: Vec::new(),
            hidden: Vec::new(),
            announce: None,
            ghost: None,
            primary_sent: None,
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
    match (key, modifiers.command || modifiers.ctrl, modifiers.shift) {
        (K::ArrowLeft, true, _) => return Some(Key::CtrlLeft),
        (K::ArrowRight, true, _) => return Some(Key::CtrlRight),
        (K::ArrowLeft, false, true) => return Some(Key::ShiftLeft),
        (K::ArrowRight, false, true) => return Some(Key::ShiftRight),
        _ => {}
    }
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
        self.regions = (None, Vec::new());
        self.resolve_folds();
        self.drag = None;
        self.want_x = None;
        if let Some(vim) = &mut self.vim {
            vim.switch_buffer();
        }
        self.reveal = true;
    }

    pub fn set_vim(&mut self, on: bool) {
        match (on, self.vim.is_some()) {
            (true, false) => {
                // Vim mode has one caret.
                let r = self.doc.selection.primary();
                self.doc.selection = Selection::single(r.anchor, r.head);
                self.vim = Some(Vim::new());
            }
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

    /// The selected text, if any.
    pub fn selected(&self) -> Option<String> {
        let r = self.doc.selection.primary();
        (!r.is_empty()).then(|| self.doc.buffer.slice(r.start(), r.end()))
    }

    /// The lines shown, first to last (from 0, end excluded), as last drawn.
    pub fn visible_lines(&self) -> std::ops::Range<usize> {
        self.view.0..self.view.0 + self.view.1 + 1
    }

    /// The misspelled word at the caret, if it is on one.
    pub fn misspelling_at_caret(&self) -> Option<&Problem> {
        let caret = self.caret();
        self.problems.iter().find(|p| p.spelling && p.start <= caret && caret <= p.end)
    }

    /// The grammar problem at the caret, if it is on one.
    pub fn grammar_at_caret(&self) -> Option<&Problem> {
        let caret = self.caret();
        self.problems.iter().find(|p| p.grammar.is_some() && p.start <= caret && caret <= p.end)
    }

    /// Whether typing inserts text: always, or in Vim's Insert mode.
    pub fn inserting(&self) -> bool {
        self.vim.as_ref().is_none_or(|v| v.mode().is_inserting())
    }

    /// Whether the one caret is at the end of its line's text (only spaces after it).
    pub fn at_line_end(&self) -> bool {
        let b = &self.doc.buffer;
        let caret = self.caret();
        !self.multi() && b.slice(caret, b.line_end(b.char_to_line(caret))).trim().is_empty()
    }

    /// Whether the caret is on a problem's text (Quick Fix opens its popup).
    pub fn on_problem(&self) -> bool {
        let caret = self.caret();
        self.problems.iter().any(|p| p.start <= caret && caret <= p.end)
    }

    /// A command outside Vim acted on the selection: Vim leaves Visual mode, as after an operator.
    pub fn end_visual(&mut self) {
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
        vim.end_visual(&mut vctx);
    }

    /// Puts the selection on the clipboard (Edit › Copy), and removes it from the text when
    /// `cut`; false when nothing is selected. In Vim mode it acts on Visual mode's selection and
    /// leaves Visual mode.
    pub fn copy(&mut self, ctx: &egui::Context, cut: bool) -> bool {
        let Some(text) = self.selected() else { return false };
        ctx.copy_text(text);
        if cut {
            self.replace_selection("", EditKind::Other);
            self.doc.buffer.seal();
            self.vim_sync();
        } else {
            self.end_visual();
        }
        true
    }

    /// The window was asked for the clipboard's text at `now`: the paste it sends next goes into
    /// the text at the selection ([`Edit::Paste`]).
    pub fn ask_paste(&mut self, now: f64) {
        self.paste_asked = Some(now);
    }

    // ----- Folding -----

    /// The text's foldable regions, made again when the text changed.
    pub fn regions(&mut self) -> &[FoldRegion] {
        let version = self.doc.buffer.version();
        if self.regions.0 != Some(version) {
            self.regions = (Some(version), texcraft_latex::fold::regions(&self.doc.buffer.text()));
        }
        &self.regions.1
    }

    /// Finds each fold's region again after edits; a fold whose region is gone unfolds. Then marks
    /// the lines the folds hide.
    fn resolve_folds(&mut self) {
        self.regions();
        let (regions, buffer) = (&self.regions.1, &mut self.doc.buffer);
        let mut folded = Vec::new();
        let mut gone = Vec::new();
        self.doc.folds.retain(|(mark, kind)| {
            let line = buffer.mark(*mark).map(|p| buffer.char_to_line(p));
            let region = line.and_then(|l| regions.iter().find(|r| r.start == l && same_kind(kind, &r.kind)));
            match region {
                Some(r) if !folded.iter().any(|(s, _, _)| *s == r.start) => {
                    folded.push((r.start, r.end, *mark));
                    true
                }
                _ => {
                    gone.push(*mark);
                    false
                }
            }
        });
        for mark in gone {
            buffer.remove_mark(mark);
        }
        folded.sort_unstable_by_key(|(s, _, _)| *s);
        let mut hidden = vec![false; buffer.len_lines()];
        for (s, e, _) in &folded {
            for h in hidden.iter_mut().take(e + 1).skip(s + 1) {
                *h = true;
            }
        }
        self.folded = folded;
        self.hidden = hidden;
    }

    fn is_hidden(&self, line: usize) -> bool {
        self.hidden.get(line).copied().unwrap_or(false)
    }

    /// The outermost fold that hides `line`.
    fn fold_hiding(&self, line: usize) -> Option<(usize, usize, MarkId)> {
        self.folded.iter().find(|(s, e, _)| *s < line && line <= *e).copied()
    }

    /// Whether the region that starts on `line` is folded.
    pub fn is_folded(&self, line: usize) -> bool {
        self.folded.iter().any(|(s, _, _)| *s == line)
    }

    fn unfold_mark(&mut self, mark: MarkId) {
        self.doc.folds.retain(|(m, _)| *m != mark);
        self.doc.buffer.remove_mark(mark);
    }

    /// Unfolds what hides the caret: a jump, a search or a motion landed inside a fold.
    fn reveal_caret(&mut self) {
        let line = self.doc.buffer.char_to_line(self.caret());
        if !self.is_hidden(line) {
            return;
        }
        while let Some((_, _, mark)) = self.fold_hiding(line) {
            self.unfold_mark(mark);
            self.resolve_folds();
        }
        self.reveal = true;
    }

    /// After folding: a caret inside a fold goes to its first line.
    fn caret_out_of_folds(&mut self) {
        let b = &self.doc.buffer;
        let caret = self.caret();
        let line = b.char_to_line(caret);
        let Some((start, _, _)) = self.fold_hiding(line) else { return };
        let col = caret - b.line_to_char(line);
        let to = b.line_to_char(start) + col.min(b.line(start).chars().count());
        self.doc.selection = Selection::point(to);
        self.doc.buffer.seal();
        self.vim_sync();
        self.reveal = true;
    }

    /// Folds the region that starts on `line`.
    fn fold_line(&mut self, line: usize) -> Option<String> {
        if self.is_folded(line) {
            return None;
        }
        let region = self.regions().iter().find(|r| r.start == line)?.clone();
        let at = self.doc.buffer.line_to_char(line);
        let mark = self.doc.buffer.add_mark(at, Bias::Right);
        self.doc.folds.push((mark, region.kind.clone()));
        self.resolve_folds();
        self.caret_out_of_folds();
        Some(fold_message(true, &region))
    }

    /// Unfolds the region that starts on `line`.
    fn unfold_line(&mut self, line: usize) -> Option<String> {
        let (s, e, mark) = self.folded.iter().find(|(s, _, _)| *s == line).copied()?;
        let region = self.regions().iter().find(|r| r.start == s && r.end == e).cloned();
        self.unfold_mark(mark);
        self.resolve_folds();
        Some(region.map_or_else(|| tl!("Unfolded").to_owned(), |r| fold_message(false, &r)))
    }

    /// Folds or unfolds the region that starts on `line` (its gutter marker, its "…" chip).
    pub fn toggle_fold(&mut self, line: usize) -> Option<String> {
        if self.is_folded(line) { self.unfold_line(line) } else { self.fold_line(line) }
    }

    /// Fold, Unfold, Fold All and Unfold All (and Vim's `zc`, `zo`, `za`, `zM`, `zR`): what to
    /// announce, or why nothing changed.
    pub fn fold(&mut self, action: Fold) -> String {
        self.resolve_folds();
        let line = self.doc.buffer.char_to_line(self.caret());
        // The innermost region around the caret that is not folded yet.
        let innermost_open = |me: &mut Self| {
            let folded: Vec<usize> = me.folded.iter().map(|(s, _, _)| *s).collect();
            me.regions().iter().filter(|r| r.start <= line && line <= r.end && !folded.contains(&r.start)).max_by_key(|r| r.start).map(|r| r.start)
        };
        let said = match action {
            Fold::Close => innermost_open(self).and_then(|l| self.fold_line(l)),
            Fold::Open => self.unfold_line(line),
            Fold::Toggle if self.is_folded(line) => self.unfold_line(line),
            Fold::Toggle => innermost_open(self).and_then(|l| self.fold_line(l)),
            Fold::CloseAll => {
                let starts: Vec<usize> = self.regions().iter().map(|r| r.start).collect();
                let mut n = 0;
                for start in starts {
                    if !self.is_folded(start) {
                        let at = self.doc.buffer.line_to_char(start);
                        let kind = self.regions.1.iter().find(|r| r.start == start).map(|r| r.kind.clone());
                        if let Some(kind) = kind {
                            let mark = self.doc.buffer.add_mark(at, Bias::Right);
                            self.doc.folds.push((mark, kind));
                            n += 1;
                        }
                    }
                }
                self.resolve_folds();
                self.caret_out_of_folds();
                (n > 0).then(|| crate::i18n::trn(n, "Folded {n} region", "Folded {n} regions"))
            }
            Fold::OpenAll => {
                let n = self.doc.folds.len();
                for (mark, _) in std::mem::take(&mut self.doc.folds) {
                    self.doc.buffer.remove_mark(mark);
                }
                self.resolve_folds();
                (n > 0).then(|| crate::i18n::trn(n, "Unfolded {n} region", "Unfolded {n} regions"))
            }
        };
        said.unwrap_or_else(|| match action {
            Fold::Close | Fold::Toggle => tl!("There is nothing to fold at the caret.").to_owned(),
            Fold::Open => tl!("There is no fold at the caret.").to_owned(),
            Fold::CloseAll => tl!("There is nothing to fold in this file.").to_owned(),
            Fold::OpenAll => tl!("Nothing in this file is folded.").to_owned(),
        })
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
            for (line, job) in self.layout.jobs.iter().enumerate() {
                let galley = f.layout_job(job.clone());
                tops.push(y);
                // A line hidden in a fold takes no room.
                if !self.hidden.get(line).copied().unwrap_or(false) {
                    y += galley.size().y.max(t.code_line_height);
                }
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
        // A hidden line has no height: the last line starting above `y` is a shown one, except
        // past the end of a fold that ends the text, where its first line is.
        let line = tops.partition_point(|top| *top <= y).saturating_sub(1).min(self.layout.galleys.len().saturating_sub(1));
        self.fold_hiding(line).map_or(line, |(start, _, _)| start)
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

    /// The on-screen rectangle of the caret at `pos`, as drawn this frame (after [`Self::show`]).
    pub fn screen_rect(&self, pos: usize) -> Option<Rect> {
        let (origin, _) = self.shown?;
        Some(self.rect_at(pos).translate(origin.to_vec2()))
    }

    /// The part of the editor shown, on screen, and where its text starts (after [`Self::show`]).
    pub fn shown(&self) -> Option<(Pos2, Rect)> {
        self.shown
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
                    // Escape returns several carets to one before it leaves the editor.
                    escape: vim_mode.is_some() || self.completion.is_some() || self.multi() || self.ghost.is_some(),
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
        self.middle_click_paste(&ctx, id);
        out.changed = self.doc.buffer.version() != version;
        // Folds follow the edits; a jump or a motion into one unfolds it.
        self.resolve_folds();
        self.reveal_caret();

        let lines = self.doc.buffer.len_lines();
        let digits = lines.to_string().len().max(2) as f32;
        let char_w = ui.fonts_mut(|f| f.glyph_width(&t.code_font(), '0'));
        // The numbers, then the fold markers.
        let gutter = digits * char_w + 13.0 * t.unit;
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
            // A finger's drag scrolls; its tap or long press takes the keyboard.
            let finger = crate::touch::scroll(ui, &response);
            if response.clicked() || (!finger && (response.drag_started() || response.secondary_clicked())) {
                response.request_focus();
            }
            self.mouse(&response, origin, &ctx, finger);
            let first = self.line_at_y(viewport.min.y - margin.y);
            let last = self.line_at_y(viewport.max.y - margin.y);
            self.view = (first, last.saturating_sub(first).max(1));
            let area = Area { rect, origin, gutter, lines: (first, last), focused };
            self.shown = Some((origin, ui.clip_rect()));
            self.paint(ui, t, &area, markers);
            self.fold_controls(ui, t, &area, id);
            if self.accept_drop
                && response.contains_pointer()
                && let Some(p) = ctx.pointer_interact_pos()
            {
                let at = self.pos_at(p - origin);
                let line = self.rect_at(at).translate(origin.to_vec2());
                ui.painter().rect_filled(Rect::from_min_size(line.min, vec2(2.0, line.height())), 0.0, t.accent);
                if ctx.input(|i| i.pointer.any_released()) {
                    out.dropped_at = Some(at);
                }
            }
            let caret = self.rect_at(self.caret()).translate(origin.to_vec2());
            if std::mem::take(&mut self.reveal) {
                ui.scroll_to_rect(caret.expand2(vec2(0.0, t.code_line_height)), None);
            }
            if focused {
                self.composer.set_caret(&ctx, caret);
            }
            self.caret_screen = (focused && ui.clip_rect().intersects(caret)).then_some(caret);
            self.accessibility(ui, &response, id, name, origin);
            out.response = Some(response);
        });
        if self.caret_screen.is_none() {
            self.completion = None;
        }
        if focused {
            self.offer_primary(&ctx);
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
            self.popup = Some(ProblemPopup { problem, at: pointer + vec2(0.0, 14.0), fixes, pinned: false, selected: 0, ai: None });
        }
        if let Some(popup) = &self.popup
            && popup.fixes.is_none()
        {
            out.fixes_for = Some(popup.problem.line);
        }
        out.apply_fix = self.apply_fix.take();
        out.popup_action = self.popup_action.take();
        match self.ai_act.take() {
            Some(Some(line)) => out.explain = Some(line),
            Some(None) => out.cancel_ai = true,
            None => {}
        }
        out.announce = self.announce.take();
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

    /// A middle click on the text pastes the primary selection there (X11, Wayland: the window
    /// sends it with the click's place). Before this frame's text is drawn, so the place is read
    /// on last frame's, which the pointer saw; a click on a window over the text is not ours.
    fn middle_click_paste(&mut self, ctx: &egui::Context, id: egui::Id) {
        let Some((origin, clip)) = self.shown else { return };
        let Some(response) = ctx.read_response(id) else { return };
        let pasted = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::MiddleClickPaste { pos, text } => Some((*pos, text.clone())),
                _ => None,
            })
        });
        let Some((p, text)) = pasted else { return };
        if text.is_empty() || !response.rect.contains(p) || !clip.contains(p) || ctx.layer_id_at(p) != Some(response.layer_id) {
            return;
        }
        let at = self.pos_at(p - origin);
        // Its own undo step, as a paste is.
        self.doc.buffer.seal();
        self.replace_range(at, at, &text, EditKind::Other);
        self.doc.buffer.seal();
        self.completion = None;
        self.vim_sync();
        ctx.memory_mut(|m| m.request_focus(id));
    }

    /// Once the selection settles (no button held), its text becomes the primary selection, to
    /// paste with a middle click here or in another program (X11, Wayland; elsewhere the window
    /// ignores it).
    fn offer_primary(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let r = self.doc.selection.primary();
        let selected = (!r.is_empty()).then(|| (r.start(), r.end()));
        if selected == self.primary_sent {
            return;
        }
        self.primary_sent = selected;
        if let Some((start, end)) = selected {
            ctx.send_cmd(egui::OutputCommand::TextSelectionSettled(self.doc.buffer.slice(start, end)));
        }
    }

    fn apply(&mut self, edit: Edit, now: f64, indent: &str, ctx: &egui::Context, out: &mut Output) {
        match edit {
            Edit::Preedit { .. } => {}
            Edit::Insert(text) => {
                let inserting = self.vim.as_ref().is_none_or(|v| v.mode().is_inserting());
                if self.vim.is_some() {
                    for c in text.chars() {
                        self.vim_key(Key::Char(c), now, indent, ctx, out);
                    }
                } else if self.auto_close && text.chars().count() == 1 && !self.multi() {
                    let c = text.chars().next().unwrap_or(' ');
                    self.type_char(c, now, indent);
                } else {
                    self.replace_selection(&text, EditKind::Typing { at: now });
                }
                // A command, a reference, a citation, an environment being typed: ask what fits.
                let trigger = text.chars().last().is_some_and(|c| c.is_alphanumeric() || matches!(c, '\\' | '{' | ',' | ':' | '-' | '_'));
                if inserting && !self.multi() && (trigger || self.completion.is_some()) {
                    self.wants_completion = true;
                } else {
                    self.completion = None;
                }
            }
            Edit::LineBreak { modifiers } => self.key(egui::Key::Enter, modifiers, now, indent, ctx, out),
            Edit::Key { key, modifiers, .. } => self.key(key, modifiers, now, indent, ctx, out),
            // The paste asked for by Edit › Paste: in Vim mode it replaces Visual mode's selection
            // or goes in before the caret, as `"+P` would.
            Edit::Paste(text) if self.paste_asked.take().is_some_and(|at| now - at < PASTE_SECONDS) && self.vim.is_some() => {
                self.replace_selection(&text, EditKind::Other);
                self.doc.buffer.seal();
                self.vim_sync();
            }
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
                let ranges = self.doc.selection.ranges();
                if ranges.iter().all(Range::is_empty) {
                    return;
                }
                // With several selections, their texts one per line.
                let texts: Vec<String> = ranges.iter().filter(|r| !r.is_empty()).map(|r| self.doc.buffer.slice(r.start(), r.end())).collect();
                ctx.copy_text(texts.join("\n"));
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
        // The next words shown after the caret: Tab takes them, Escape dismisses them (in Vim,
        // Escape also leaves Insert mode).
        if let Some((at, text)) = self.ghost.take() {
            if key == egui::Key::Tab && modifiers.is_none() && inserting && at == self.caret() {
                self.replace_selection(&text, EditKind::Other);
                self.doc.buffer.seal();
                self.vim_sync();
                out.ghost = Some(true);
                return;
            }
            if key == egui::Key::Escape {
                out.ghost = Some(false);
                if self.vim.is_none() {
                    return;
                }
            }
        }
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
        let from = self.doc.buffer.char_to_line(vim.cursor());
        let mut vctx =
            Ctx { buffer: &mut self.doc.buffer, selection: &mut self.doc.selection, now, view_top: top, view_rows: rows, clipboard: None, indent };
        let result = vim.key(key, &mut vctx);
        // `j` and `k` step over a fold, as Vim's do, rather than open it.
        if matches!(key, Key::Char('j' | 'k') | Key::Down | Key::Up) && vim.mode() == Mode::Normal {
            self.step_over_fold(from);
        }
        for effect in result.effects {
            match effect {
                Effect::Copy(text) => ctx.copy_text(text),
                Effect::Scroll(s) => self.scroll = Some(s),
                Effect::Fold(f) => {
                    let said = self.fold(f);
                    self.announce = Some(said);
                }
                other => out.effects.push(other),
            }
        }
        self.reveal = true;
    }

    // ----- Several carets -----

    /// More than one caret or selection.
    fn multi(&self) -> bool {
        self.doc.selection.ranges().len() > 1
    }

    fn announce_carets(&mut self) {
        let n = self.doc.selection.ranges().len();
        self.announce = Some(if n > 1 { crate::i18n::trn(n, "{n} caret", "{n} carets") } else { tl!("One caret").to_owned() });
    }

    /// One edit at every caret or selection, undone as one: `f` gives what each range's change
    /// replaces and with what (none: that range changes nothing). Each range becomes a caret after
    /// its change.
    fn edit_each(&mut self, kind: EditKind, f: impl Fn(&Buffer, Range) -> Option<(usize, usize, String)>) {
        let before = self.doc.selection.clone();
        let b = &self.doc.buffer;
        let mut changes: Vec<Change> =
            before.ranges().iter().filter_map(|r| f(b, *r)).map(|(start, end, text)| Change { start, end, text }).collect();
        changes.sort_by_key(|c| (c.start, c.end));
        // Two carets whose changes overlap (Backspace at both ends of a character) make one.
        let mut merged: Vec<Change> = Vec::with_capacity(changes.len());
        for c in changes {
            match merged.last_mut() {
                Some(last) if c.start < last.end => last.end = last.end.max(c.end),
                _ => merged.push(c),
            }
        }
        let Ok(tx) = Transaction::new(merged) else { return };
        let ranges = before.ranges().iter().map(|r| Range::point(tx.map(r.end(), Bias::Right))).collect();
        let after = Selection::with_primary(ranges, before.primary_index()).merged();
        if self.doc.buffer.edit(tx, &before, &after, kind).is_ok() {
            self.doc.selection = after;
        }
        self.want_x = None;
        self.reveal = true;
    }

    /// Moves every range with `f`.
    fn move_each(&mut self, f: impl Fn(&mut Self, Range) -> Range) {
        let before = self.doc.selection.clone();
        let ranges = before.ranges().iter().map(|r| f(self, *r)).collect();
        self.doc.selection = Selection::with_primary(ranges, before.primary_index()).merged();
        self.doc.buffer.seal();
        self.reveal = true;
    }

    /// The keys that act on every caret; false for the others, which act on the primary one.
    fn multi_key(&mut self, key: egui::Key, m: egui::Modifiers, now: f64, indent: &str) -> bool {
        use egui::Key as K;
        let word = m.command || m.alt;
        let to = |r: Range, pos: usize| if m.shift { Range::new(r.anchor, pos) } else { Range::point(pos) };
        match key {
            K::Escape => {
                let r = self.doc.selection.primary();
                self.doc.selection = Selection::single(r.anchor, r.head);
                self.announce_carets();
            }
            K::ArrowLeft | K::ArrowRight => {
                let left = key == K::ArrowLeft;
                self.move_each(|me, r| {
                    if !r.is_empty() && !m.shift {
                        return Range::point(if left { r.start() } else { r.end() });
                    }
                    let pos = match (left, word) {
                        (true, true) => me.word_left(r.head),
                        (false, true) => me.word_right(r.head),
                        (true, false) => r.head.saturating_sub(1),
                        (false, false) => (r.head + 1).min(me.doc.buffer.len_chars()),
                    };
                    to(r, pos)
                });
            }
            K::ArrowUp | K::ArrowDown => {
                let rows = if key == K::ArrowUp { -1.0 } else { 1.0 };
                self.move_each(|me, r| {
                    me.want_x = None;
                    let pos = me.row_step(r.head, rows);
                    to(r, pos)
                });
                self.want_x = None;
            }
            K::Home | K::End => {
                self.move_each(|me, r| {
                    let b = &me.doc.buffer;
                    let line = b.char_to_line(r.head);
                    let pos = if key == K::End {
                        b.line_end(line)
                    } else {
                        let start = b.line_to_char(line);
                        let first = start + b.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').count();
                        if r.head == first { start } else { first }
                    };
                    to(r, pos)
                });
            }
            K::Backspace | K::Delete => {
                let back = key == K::Backspace;
                let len = self.doc.buffer.len_chars();
                let (left, right): (Vec<usize>, Vec<usize>) = if word {
                    self.doc.selection.ranges().iter().map(|r| (self.word_left(r.head), self.word_right(r.head))).unzip()
                } else {
                    self.doc.selection.ranges().iter().map(|r| (r.head.saturating_sub(1), (r.head + 1).min(len))).unzip()
                };
                let ranges = self.doc.selection.ranges().to_vec();
                self.edit_each(EditKind::Deleting { at: now }, |_, r| {
                    let i = ranges.iter().position(|x| *x == r)?;
                    match (r.is_empty(), back) {
                        (false, _) => Some((r.start(), r.end(), String::new())),
                        (true, true) => Some((left[i], r.head, String::new())).filter(|(s, e, _)| s < e),
                        (true, false) => Some((r.head, right[i], String::new())).filter(|(s, e, _)| s < e),
                    }
                });
            }
            K::Enter => self.edit_each(EditKind::Typing { at: now }, |b, r| {
                let line = b.char_to_line(r.start());
                let lead: String = b.line(line).chars().take(r.start() - b.line_to_char(line)).take_while(|c| *c == ' ' || *c == '\t').collect();
                Some((r.start(), r.end(), format!("\n{lead}")))
            }),
            K::Tab if !m.shift => {
                let indent = indent.to_owned();
                self.edit_each(EditKind::Typing { at: now }, |_, r| Some((r.start(), r.end(), indent.clone())));
            }
            _ => return false,
        }
        true
    }

    /// Cmd+D: selects the word at the caret, or adds the next occurrence of the selected text as
    /// one more selection (after the last, from the start again at the end).
    fn add_next_occurrence(&mut self) {
        let sel = self.doc.selection.clone();
        let p = sel.primary();
        let b = &self.doc.buffer;
        if p.is_empty() {
            let (mut s, mut e) = (p.head, p.head);
            while s > 0 && b.char_at(s - 1).is_some_and(is_word) {
                s -= 1;
            }
            while b.char_at(e).is_some_and(is_word) {
                e += 1;
            }
            if s < e {
                let mut ranges = sel.ranges().to_vec();
                if let Some(r) = ranges.get_mut(sel.primary_index()) {
                    *r = Range::new(s, e);
                }
                self.doc.selection = Selection::with_primary(ranges, sel.primary_index()).merged();
                self.doc.buffer.seal();
            }
            return;
        }
        let needle = b.slice(p.start(), p.end());
        let text = b.text();
        let from = b.char_to_byte(sel.ranges().iter().map(Range::end).max().unwrap_or(p.end()));
        let taken = |s: usize, e: usize| sel.ranges().iter().any(|r| s < r.end() && r.start() < e);
        let after = text.get(from..).unwrap_or("").match_indices(needle.as_str()).map(|(i, _)| from + i);
        let before = text.match_indices(needle.as_str()).map(|(i, _)| i).take_while(|i| *i < from);
        let n = needle.chars().count();
        let found = after.chain(before).map(|i| b.byte_to_char(i)).find(|s| !taken(*s, s + n));
        match found {
            Some(s) => {
                let mut ranges = sel.ranges().to_vec();
                ranges.push(Range::new(s, s + n));
                self.doc.selection = Selection::many(ranges).merged();
                self.doc.buffer.seal();
                self.reveal = true;
                self.announce_carets();
            }
            None => self.announce = Some(tl!("There is no other occurrence.").to_owned()),
        }
    }

    /// After a line motion from line `from` that landed inside a fold: the caret goes on to the
    /// line after the fold going down, or to its first line going up.
    fn step_over_fold(&mut self, from: usize) {
        if self.regions.0 != Some(self.doc.buffer.version()) {
            return;
        }
        let b = &self.doc.buffer;
        let caret = self.caret();
        let line = b.char_to_line(caret);
        let Some((start, end, _)) = self.fold_hiding(line) else { return };
        let target = if line > from && end + 1 < b.len_lines() { end + 1 } else { start };
        let col = caret - b.line_to_char(line);
        let to = b.line_to_char(target) + col.min(b.line(target).chars().count().saturating_sub(1));
        self.doc.selection = Selection::point(to);
        self.vim_sync();
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
        if self.multi() {
            let text = text.to_owned();
            self.edit_each(kind, |_, r| Some((r.start(), r.end(), text.clone())));
            return;
        }
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
        if key == K::D && m.command {
            self.add_next_occurrence();
            return;
        }
        if self.multi() && self.multi_key(key, m, now, indent) {
            return;
        }
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
                // Over a fold rather than into it.
                let to = match self.fold_hiding(b.char_to_line(to)) {
                    Some((start, _, _)) if left => b.line_end(start),
                    Some((_, end, _)) if end + 1 < b.len_lines() => b.line_to_char(end + 1),
                    Some((start, _, _)) => b.line_end(start),
                    None => to,
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
        self.popup = Some(ProblemPopup { problem, at, fixes, pinned: true, selected: 0, ai: None });
        true
    }

    /// Keys for a popup opened from the keyboard; true when it used the key.
    fn popup_key(&mut self, key: egui::Key) -> bool {
        use egui::Key as K;
        let Some(popup) = self.popup.as_mut().filter(|p| p.pinned) else { return false };
        let fixes = shown_fixes(popup);
        let actions = popup_actions(&popup.problem);
        let n = fixes.len() + actions.len();
        match key {
            K::ArrowDown if n > 0 => popup.selected = (popup.selected + 1) % n,
            K::ArrowUp if n > 0 => popup.selected = (popup.selected + n - 1) % n,
            K::Enter | K::Tab if n > 0 => {
                match fixes.get(popup.selected) {
                    Some(fix) => self.apply_fix = Some(fix.clone()),
                    None => {
                        let action = actions.get(popup.selected - fixes.len()).map(|(id, _)| *id);
                        self.popup_action = action.map(|id| (id, popup.problem.clone()));
                    }
                }
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
        if popup.problem.spelling || popup.problem.grammar.is_some() {
            return self.paint_compact_popup(ctx, t, id);
        }
        let mut chosen = None;
        let mut close = false;
        let mut ai_act = None;
        let (icon, color) = match popup.problem.severity {
            _ if popup.problem.spelling => ("type", t.spelling),
            _ if popup.problem.grammar.is_some() => ("message-square-text", t.grammar),
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
                            let all = fix.everywhere > 0;
                            let text = if all {
                                crate::i18n::trn(
                                    fix.everywhere,
                                    "Apply this fix everywhere in this file ({n} place)",
                                    "Apply this fix everywhere in this file ({n} places)",
                                )
                            } else {
                                fix.title.clone()
                            };
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
                if let Some((explanation, blocked)) = &popup.ai {
                    ui.add_space(t.unit);
                    match crate::ai::explanation(ui, t, explanation.as_ref(), blocked.as_deref()) {
                        crate::ai::Act::Explain => ai_act = Some(Some(popup.problem.line)),
                        crate::ai::Act::Cancel => ai_act = Some(None),
                        crate::ai::Act::None => {}
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
        if ai_act.is_some() {
            self.ai_act = ai_act;
        }
        if let Some(fix) = chosen {
            self.apply_fix = Some(fix);
            self.popup = None;
            ctx.memory_mut(|m| m.request_focus(id));
        } else if close {
            self.popup = None;
        }
    }

    /// A misspelled word's or a grammar problem's popup, small so it stays out of the way: the
    /// grammar problem's message in a line of small text (a misspelling needs none: the
    /// underline says it), up to [`POPUP_FIXES`] replacements, then Add to Dictionary and Ignore
    /// (Ignore and Turn Off This Rule). The message is announced all the same.
    fn paint_compact_popup(&mut self, ctx: &egui::Context, t: &Tokens, id: egui::Id) {
        let Some(popup) = &self.popup else { return };
        let fixes = shown_fixes(popup);
        let actions = popup_actions(&popup.problem);
        let mut chosen = None;
        let mut action = None;
        let mut close = false;
        let row = |ui: &mut egui::Ui, text: &str, selected: bool| {
            let button = egui::Button::selectable(selected, egui::RichText::new(text).color(t.text)).min_size(vec2(ui.available_width(), 24.0));
            ui.add(button)
        };
        let area = egui::Area::new(id.with("problem")).order(egui::Order::Foreground).fixed_pos(popup.at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.raised).stroke(t.hairline()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_width(140.0);
                ui.set_max_width(280.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                // Said, not shown, for a misspelling; shown small for grammar, whose rule needs it.
                let (spot, _) = ui.allocate_exact_size(vec2(0.0, 0.0), Sense::hover());
                let said = ui.interact(spot, id.with("problem-said"), Sense::hover());
                crate::status::announce(ui, &said, &popup.problem.message);
                if popup.problem.grammar.is_some() {
                    let message = egui::RichText::new(&popup.problem.message).font(t.small()).color(t.text_muted);
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| ui.add(egui::Label::new(message).wrap()));
                }
                match &popup.fixes {
                    None => {
                        ui.label(egui::RichText::new(tl!("Looking for fixes…")).font(t.small()).color(t.text_muted));
                    }
                    Some(_) if fixes.is_empty() && popup.problem.spelling => {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(6, 4))
                            .show(ui, |ui| ui.label(egui::RichText::new(tl!("No spellings to offer")).font(t.small()).color(t.text_muted)));
                    }
                    Some(_) => {}
                }
                for (i, fix) in fixes.iter().enumerate() {
                    let text = match fix.edits.first().map(|e| e.text.as_str()) {
                        Some(word) if !word.is_empty() => word,
                        _ => fix.title.as_str(),
                    };
                    if row(ui, text, popup.pinned && i == popup.selected).clicked() {
                        chosen = Some(fix.clone());
                    }
                }
                if !fixes.is_empty() {
                    ui.add_space(t.unit);
                    ui.separator();
                    ui.add_space(t.unit);
                }
                for (i, (command, label)) in actions.iter().enumerate() {
                    if row(ui, label, popup.pinned && fixes.len() + i == popup.selected).clicked() {
                        action = Some(*command);
                    }
                }
                if popup.pinned && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        self.popup_rect = Some(area.response.rect);
        if let Some(fix) = chosen {
            self.apply_fix = Some(fix);
            self.popup = None;
            ctx.memory_mut(|m| m.request_focus(id));
        } else if let Some(command) = action {
            self.popup_action = self.popup.take().map(|p| (command, p.problem));
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

    fn mouse(&mut self, response: &egui::Response, origin: Pos2, ctx: &egui::Context, finger: bool) {
        // Ctrl+click (Cmd+click) goes to the definition of what is clicked.
        if response.clicked()
            && ctx.input(|i| i.modifiers.command)
            && let Some(p) = response.interact_pointer_pos()
        {
            self.wants_definition = Some(self.pos_at(p - origin));
            return;
        }
        // Alt+click adds a caret, or takes one away (outside Vim mode).
        if self.vim.is_none()
            && response.clicked()
            && ctx.input(|i| i.modifiers.alt)
            && let Some(p) = response.interact_pointer_pos()
        {
            let pos = self.pos_at(p - origin);
            let mut ranges = self.doc.selection.ranges().to_vec();
            match ranges.iter().position(|r| r.is_empty() && r.head == pos) {
                Some(i) if ranges.len() > 1 => {
                    ranges.remove(i);
                }
                Some(_) => {}
                None => ranges.push(Range::point(pos)),
            }
            self.doc.selection = Selection::many(ranges).merged();
            self.doc.buffer.seal();
            self.completion = None;
            self.announce_carets();
            return;
        }
        self.rest(response, origin, ctx);
        let before = self.doc.selection.clone();
        // A right-click outside the selection places the caret there, for the context menu.
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let pos = self.pos_at(p - origin);
            let r = self.doc.selection.primary();
            if r.is_empty() || pos < r.start() || pos > r.end() {
                self.doc.selection = Selection::point(pos);
                self.doc.buffer.seal();
            }
        }
        if finger {
            // A tap places the caret; the drag scrolled.
            if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                self.doc.selection = Selection::point(self.pos_at(p - origin));
                self.doc.buffer.seal();
                self.want_x = None;
            }
        } else if response.long_touched() || crate::touch::selecting(ctx) {
            self.touch_select(response, origin, ctx);
        } else {
            self.mouse_select(response, origin, ctx);
        }
        if self.doc.selection != before {
            self.completion = None;
            self.vim_sync();
        }
    }

    /// A finger's long press selects the word there; moving the finger, still down, extends the
    /// selection from it.
    fn touch_select(&mut self, response: &egui::Response, origin: Pos2, ctx: &egui::Context) {
        let Some(p) = ctx.input(|i| i.pointer.latest_pos()) else { return };
        let pos = self.pos_at(p - origin);
        if response.long_touched() {
            let (start, end) = self.word_at(pos);
            self.doc.selection = Selection::single(start, end);
            self.doc.buffer.seal();
            self.drag = Some(start);
            self.want_x = None;
        } else if let Some(anchor) = self.drag {
            self.doc.selection = Selection::single(anchor, pos);
        }
    }

    /// The word around `pos`: its start and end.
    fn word_at(&self, pos: usize) -> (usize, usize) {
        let b = &self.doc.buffer;
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

    fn mouse_select(&mut self, response: &egui::Response, origin: Pos2, ctx: &egui::Context) {
        let Some(p) = response.interact_pointer_pos() else {
            self.drag = None;
            return;
        };
        let pos = self.pos_at(p - origin);
        let shift = ctx.input(|i| i.modifiers.shift);
        if response.drag_started_by(egui::PointerButton::Primary) || response.clicked() {
            let now = ctx.input(|i| i.time);
            let (last_time, last_pos, count) = self.clicks;
            let count = if now - last_time < 0.4 && last_pos == pos { count % 3 + 1 } else { 1 };
            self.clicks = (now, pos, count);
            let b = &self.doc.buffer;
            let (start, end) = match count {
                2 => self.word_at(pos),
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
        } else if response.dragged_by(egui::PointerButton::Primary)
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
        let number_right = rect.left() + gutter - 7.0 * t.unit;
        let sel = self.doc.selection.primary();
        let highlight = self.vim.as_ref().and_then(Vim::highlighted);
        for line in first..=last.min(self.layout.galleys.len().saturating_sub(1)) {
            if self.is_hidden(line) {
                continue;
            }
            let (Some(galley), Some(top)) = (self.layout.galleys.get(line), self.layout.tops.get(line)) else { continue };
            let bottom = self.layout.tops.get(line + 1).copied().unwrap_or(*top);
            let y0 = origin.y + top;
            let band = Rect::from_min_max(pos2(rect.left(), y0), pos2(rect.right(), origin.y + bottom));
            if matches!(markers.get(&(line as u32)), Some((Severity::Error, _))) {
                painter.rect_filled(band, 0.0, t.error_line);
            } else if focused && line == caret_line && sel.is_empty() && !self.multi() {
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
            // Commented passages, under everything else; a removed one is a thin mark where it was.
            let line_end = line_start + line_len;
            for p in self.passages.iter().filter(|p| p.end >= line_start && p.start <= line_end && (p.end > line_start || p.start == p.end)) {
                let (s, e) = (p.start.saturating_sub(line_start).min(line_len), (p.end.saturating_sub(line_start)).min(line_len));
                paint_passage(painter, galley, origin + vec2(0.0, *top), p, (s, e), p.end > line_end);
                let bar =
                    Rect::from_min_max(pos2(rect.left() + gutter - 1.5 * t.unit, y0), pos2(rect.left() + gutter - 0.75 * t.unit, origin.y + bottom));
                painter.rect_filled(bar, 1.0, if p.selected { p.color } else { p.color.gamma_multiply(0.7) });
            }
            // The find bar's matches.
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
            // The selection, or the selections.
            for r in self.doc.selection.ranges().iter().filter(|r| !r.is_empty() && r.start() <= line_start + line_len && r.end() > line_start) {
                let s = r.start().saturating_sub(line_start);
                let e = (r.end() - line_start).min(line_len);
                let through_break = r.end() > line_start + line_len;
                paint_span(painter, galley, origin + vec2(0.0, *top), (s, e), t.selection, through_break);
            }
            painter.galley(origin + vec2(0.0, *top), galley.clone(), t.text);
            self.paint_problems(painter, t, galley, origin + vec2(0.0, *top), (line_start, line_len));
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
            for p in [a, z].into_iter().filter(|p| !self.is_hidden(b.char_to_line(*p))) {
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
            // The other carets.
            let ranges = self.doc.selection.ranges();
            if ranges.len() > 1 {
                for r in ranges.iter().filter(|r| r.head != caret && !self.is_hidden(b.char_to_line(r.head))) {
                    let c = self.rect_at(r.head).translate(origin.to_vec2());
                    let _ = painter.rect_filled(Rect::from_min_size(c.min, vec2(2.0, c.height())), 0.0, t.text);
                }
            }
            self.paint_ghost(ui, t, c, caret);
            if let Some(pre) = self.composer.preedit() {
                let g = ui.fonts_mut(|f| f.layout_no_wrap(pre.text.to_owned(), t.code_font(), t.text));
                let r = Rect::from_min_size(c.min, g.size());
                painter.rect_filled(r, 0.0, t.surface);
                painter.galley(c.min, g, t.text);
                painter.hline(r.x_range(), r.bottom(), egui::Stroke::new(1.0, t.text));
            }
        }
    }

    /// The problems of one line, underlined: hints with dots, warnings, errors and misspelled words
    /// with waves, grammar problems with a double line.
    fn paint_problems(&self, painter: &egui::Painter, t: &Tokens, galley: &Galley, at: Pos2, (line_start, line_len): (usize, usize)) {
        for p in self.problems.iter().filter(|p| p.end > line_start && p.start <= line_start + line_len) {
            let (s, e) = (p.start.saturating_sub(line_start).min(line_len), (p.end - line_start).min(line_len));
            let span = (s, e.max(s + 1).min(line_len.max(s + 1)));
            match p.severity {
                _ if p.spelling => underline(painter, galley, at, span, t.spelling, Line::Wavy),
                _ if p.grammar.is_some() => underline(painter, galley, at, span, t.grammar, Line::Double),
                Severity::Error => underline(painter, galley, at, span, t.error, Line::Wavy),
                Severity::Warning => underline(painter, galley, at, span, t.warning, Line::Wavy),
                _ => underline(painter, galley, at, span, t.note, Line::Dotted),
            }
        }
    }

    /// The next words a model suggests, in grey after the caret (`c`, on screen, at `caret`).
    fn paint_ghost(&self, ui: &egui::Ui, t: &Tokens, c: Rect, caret: usize) {
        let Some((at, text)) = &self.ghost else { return };
        if *at != caret || self.composer.preedit().is_some() {
            return;
        }
        let g = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), t.code_font(), t.text_muted));
        ui.painter().galley(c.min + vec2(2.0, 0.0), g, t.text_muted);
    }

    /// The fold markers in the gutter, on the shown lines that start a region, and the "…" chip
    /// after a folded region's first line. Both are buttons: a click folds or unfolds.
    fn fold_controls(&mut self, ui: &egui::Ui, t: &Tokens, area: &Area, id: egui::Id) {
        let Area { rect, origin, gutter, lines: (first, last), focused } = *area;
        let caret_line = self.doc.buffer.char_to_line(self.caret());
        let in_gutter = ui.ctx().pointer_hover_pos().is_some_and(|p| p.x < rect.left() + gutter && rect.contains(p));
        let regions: Vec<FoldRegion> = self.regions().iter().filter(|r| r.start >= first && r.start <= last).cloned().collect();
        let mut toggled = None;
        for region in regions {
            let line = region.start;
            if self.is_hidden(line) {
                continue;
            }
            let (Some(galley), Some(top)) = (self.layout.galleys.get(line), self.layout.tops.get(line)) else { continue };
            let folded = self.is_folded(line);
            let first_row = galley.rows.first().map_or(t.code_line_height, |r| r.max_y());
            let mid = origin.y + top + first_row / 2.0;
            let marker = Rect::from_center_size(pos2(rect.left() + gutter - 4.5 * t.unit, mid), vec2(3.5 * t.unit, first_row.max(3.5 * t.unit)));
            let what = describe(&region.kind);
            let (from, to) = ((region.start + 1).to_string(), (region.end + 1).to_string());
            let name = if folded { tl!("Unfold lines {from}–{to}, {what}") } else { tl!("Fold lines {from}–{to}, {what}") }
                .replace("{from}", &from)
                .replace("{to}", &to)
                .replace("{what}", &what);
            let r = ui.interact(marker, id.with(("fold", line)), Sense::click());
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            ui.ctx().accesskit_node_builder(r.id, |node| node.set_expanded(!folded));
            let shown = folded || in_gutter || r.hovered() || r.has_focus() || (focused && line == caret_line);
            if shown {
                if r.hovered() || r.has_focus() {
                    ui.painter().rect_filled(marker, t.corner(), t.hover);
                }
                let color = if folded || r.hovered() { t.text } else { t.text_muted };
                icons::paint(ui, marker, if folded { "chevron-right" } else { "chevron-down" }, 3.0 * t.unit, color);
            }
            if r.has_focus() {
                ui.painter().rect_stroke(marker, t.corner(), egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            let r = r.on_hover_text(&name);
            if r.clicked() {
                toggled = Some(line);
            }
            // The chip that stands for the hidden lines.
            if folded && let Some(row) = galley.rows.last() {
                let label = ui.fonts_mut(|f| f.layout_no_wrap("…".to_owned(), t.code_font(), t.text_muted));
                let at = origin + vec2(row.rect().right() + 2.0 * t.unit, top + row.min_y());
                let chip = Rect::from_min_size(at, vec2(label.size().x + 3.0 * t.unit, row.height())).shrink2(vec2(0.0, 1.0));
                let c = ui.interact(chip, id.with(("fold-chip", line)), Sense::click());
                let unfold = tl!("Unfold lines {from}–{to}, {what}").replace("{from}", &from).replace("{to}", &to).replace("{what}", &what);
                c.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &unfold));
                let fill = if c.hovered() || c.has_focus() { t.hover } else { t.raised };
                ui.painter().rect(chip, t.corner(), fill, t.hairline(), egui::StrokeKind::Inside);
                if c.has_focus() {
                    ui.painter().rect_stroke(chip, t.corner(), egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                }
                ui.painter().galley(chip.center() - label.size() / 2.0, label, t.text_muted);
                if c.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&unfold).clicked() {
                    toggled = Some(line);
                }
            }
        }
        if let Some(line) = toggled {
            self.announce = self.toggle_fold(line);
            ui.ctx().memory_mut(|m| m.request_focus(id));
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

/// Passage `p` on chars `s..e` of one line's galley (`through_break`: and its line break).
fn paint_passage(painter: &egui::Painter, galley: &Galley, at: Pos2, p: &Passage, (s, e): (usize, usize), through_break: bool) {
    if p.start == p.end {
        let c = galley.pos_from_cursor(CCursor::new(s)).translate(at.to_vec2());
        painter.rect_filled(Rect::from_min_size(c.min - vec2(1.0, 0.0), vec2(2.0, c.height())), 1.0, p.color);
        return;
    }
    let strength = match (p.mark, p.selected) {
        (Mark::Comment, false) => 0.2,
        (_, true) => 0.42,
        (_, false) => 0.14,
    };
    paint_span(painter, galley, at, (s, e), p.color.gamma_multiply(strength), through_break);
    match p.mark {
        Mark::Comment if p.selected => underline(painter, galley, at, (s, e), p.color, Line::Dotted),
        Mark::Comment => {}
        Mark::Deleted => rule(painter, galley, at, (s, e), p.color, 0.5),
        Mark::Inserted => rule(painter, galley, at, (s, e), p.color, 1.0),
    }
}

/// A solid line across chars `s..e` of one line's galley, at `height` of each row from its top
/// (0.5 strikes the text through, 1.0 underlines it).
fn rule(painter: &egui::Painter, galley: &Galley, at: Pos2, (s, e): (usize, usize), color: egui::Color32, height: f32) {
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (a, z) = (s.max(row_start), e.min(row_start + n));
        if a < z {
            let x0 = at.x + row.pos.x + row.x_offset(egui::text::CharIndex(a - row_start));
            let x1 = at.x + row.pos.x + row.x_offset(egui::text::CharIndex(z - row_start));
            let y = at.y + row.min_y() + (row.max_y() - row.min_y()) * height - if height >= 1.0 { 1.5 } else { 0.0 };
            painter.line_segment([pos2(x0, y), pos2(x1, y)], egui::Stroke::new(1.5, color));
        }
        row_start += n;
    }
}

/// A dotted (or, `wavy`, a wavy) line under chars `s..e` of one line's galley.
fn underline(painter: &egui::Painter, galley: &Galley, at: Pos2, (s, e): (usize, usize), color: egui::Color32, line: Line) {
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (a, z) = (s.max(row_start), e.min(row_start + n));
        if a < z {
            let x0 = at.x + row.pos.x + row.x_offset(egui::text::CharIndex(a - row_start));
            let x1 = at.x + row.pos.x + row.x_offset(egui::text::CharIndex(z - row_start));
            let y = at.y + row.max_y() - 1.5;
            if let Line::Double = line {
                for dy in [-1.5, 0.5] {
                    painter.hline(x0..=x1, y + dy, egui::Stroke::new(1.0, color));
                }
            } else if let Line::Wavy = line {
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

/// "Folded lines 12–40, section Introduction", or Unfolded.
fn fold_message(folded: bool, r: &FoldRegion) -> String {
    let text = if folded { tl!("Folded lines {from}–{to}, {what}") } else { tl!("Unfolded lines {from}–{to}, {what}") };
    text.replace("{from}", &(r.start + 1).to_string()).replace("{to}", &(r.end + 1).to_string()).replace("{what}", &describe(&r.kind))
}
