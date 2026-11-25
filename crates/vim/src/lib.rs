//! Vim key bindings (docs/ROADMAP.md), as a state machine over a
//! [`texcraft_buffer::Buffer`]: the editor widget turns key presses into [`Key`]s and hands them
//! to [`Vim::key`] with the buffer and selection ([`Ctx`]); Vim edits the buffer, sets the
//! selection, and returns what the window must do ([`Effect`]: save, close, open, copy, scroll,
//! show a message).
//!
//! Behaviour follows Vim's documentation (`:help`); Vim's code is not read (clean room). What is
//! not supported yet says so through [`Effect::Error`] instead of doing something else: Visual
//! block mode, `=`, back-references in search patterns, case changes in replacements, and `:s`
//! with confirmation.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod ex;
mod motion;
mod object;
mod parse;

use std::collections::HashMap;

use regex::Regex;
use texcraft_buffer::{Bias, Buffer, Change, EditKind, MarkId, Selection, Transaction};

use crate::motion::{at_column, column, first_non_blank, line_last};
use crate::parse::{Cmd, Motion, Op, Parse, Parsed, Simple, Target};

/// A key, as the widget reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A typed character, with Shift applied.
    Char(char),
    /// Ctrl with a letter or `[`, lower case.
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
    Delete,
    Tab,
    Left,
    Right,
    /// Ctrl+Left and Ctrl+Right: WORDS back and forward (`B`, `W`); words in Insert mode.
    CtrlLeft,
    CtrlRight,
    /// Shift+Left and Shift+Right: words back and forward (`b`, `w`).
    ShiftLeft,
    ShiftRight,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
    Visual,
    VisualLine,
}

impl Mode {
    /// The mode's name, as the status bar shows it.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Replace => "REPLACE",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine)
    }

    /// The caret is a bar between characters (it is a block on one otherwise).
    pub fn is_inserting(self) -> bool {
        matches!(self, Mode::Insert | Mode::Replace)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// The caret's line to the middle, top or bottom of the view.
    Center,
    Top,
    Bottom,
    /// The view by this many lines (down when positive).
    Lines(isize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Save,
    SaveAll,
    /// Close the file (`force`: without saving changes).
    Close {
        force: bool,
    },
    Open(String),
    /// Put text on the system clipboard.
    Copy(String),
    Message(String),
    Error(String),
    Scroll(Scroll),
    /// Comment on `start..end` (`:comment [text]`): the lines of the range, or the caret's line.
    Comment {
        start: usize,
        end: usize,
        text: String,
    },
    /// Turn the editor's Suggest mode on or off (`:set suggest`, `:set nosuggest`).
    Suggest(bool),
}

/// What a key works on.
pub struct Ctx<'a> {
    pub buffer: &'a mut Buffer,
    pub selection: &'a mut Selection,
    /// The time, in seconds, for undo grouping.
    pub now: f64,
    /// The first line shown and how many fit, for `H`, `M`, `L` and the scrolling keys.
    pub view_top: usize,
    pub view_rows: usize,
    /// The system clipboard's text, when the window can read it (`"+p`).
    pub clipboard: Option<String>,
    /// One level of indentation, for `>`, `<`, Tab.
    pub indent: &'a str,
}

/// The result of a key.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Vim used the key; when false, the window's own shortcuts may.
    pub handled: bool,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Reg {
    text: String,
    linewise: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Command,
    Search { forward: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Here,
    Lines,
}

#[derive(Debug, Clone)]
struct InsertSession {
    count: usize,
    repeat: Repeat,
    typed: String,
    /// In Replace mode, the characters typed over (none where text was added).
    replaced: Vec<Option<char>>,
}

#[derive(Debug, Clone)]
struct Search {
    source: String,
    forward: bool,
    re: Regex,
}

/// A region an operator works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    /// `start..end`, chars.
    Chars(usize, usize),
    /// Lines `first..=last`.
    Lines(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

#[derive(Debug, Clone, Copy)]
struct Moved {
    to: usize,
    kind: Kind,
    jump: bool,
}

const MAX_REPLAY_DEPTH: usize = 50;

/// Vim's state for one editor.
#[derive(Debug, Clone)]
pub struct Vim {
    mode: Mode,
    cursor: usize,
    anchor: usize,
    pending: Vec<Key>,
    regs: HashMap<char, Reg>,
    marks: HashMap<char, MarkId>,
    want: Option<usize>,
    last_find: Option<(char, bool, bool)>,
    search: Option<Search>,
    highlight: bool,
    line: Option<(LineKind, String)>,
    insert: Option<InsertSession>,
    insert_register: bool,
    oneshot: bool,
    cmd_keys: Vec<Key>,
    cmd_version: u64,
    dot: Vec<Key>,
    dot_rec: Option<Vec<Key>>,
    replaying: usize,
    recording: Option<(char, Vec<Key>)>,
    macros: HashMap<char, Vec<Key>>,
    last_macro: Option<char>,
    last_ex: Option<String>,
    last_visual: Option<Mode>,
    jumps: Vec<MarkId>,
    jump_at: usize,
    ignorecase: bool,
    smartcase: bool,
    written: Option<Selection>,
}

impl Default for Vim {
    fn default() -> Self {
        Self::new()
    }
}

impl Vim {
    pub fn new() -> Self {
        Self {
            mode: Mode::Normal,
            cursor: 0,
            anchor: 0,
            pending: Vec::new(),
            regs: HashMap::new(),
            marks: HashMap::new(),
            want: None,
            last_find: None,
            search: None,
            highlight: false,
            line: None,
            insert: None,
            insert_register: false,
            oneshot: false,
            cmd_keys: Vec::new(),
            cmd_version: 0,
            dot: Vec::new(),
            dot_rec: None,
            replaying: 0,
            recording: None,
            macros: HashMap::new(),
            last_macro: None,
            last_ex: None,
            last_visual: None,
            jumps: Vec::new(),
            jump_at: 0,
            ignorecase: false,
            smartcase: false,
            written: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Where the caret is drawn: on this character in Normal and Visual modes, before it in
    /// Insert mode.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The command line being typed, with its `:`, `/` or `?`.
    pub fn command_line(&self) -> Option<String> {
        self.line.as_ref().map(|(kind, text)| {
            let lead = match kind {
                LineKind::Command => ':',
                LineKind::Search { forward: true } => '/',
                LineKind::Search { forward: false } => '?',
            };
            format!("{lead}{text}")
        })
    }

    /// The keys of a command being typed (`2d`, `"a`), for the status bar.
    pub fn pending(&self) -> String {
        self.pending.iter().map(key_text).collect()
    }

    /// The register being recorded into.
    pub fn recording(&self) -> Option<char> {
        self.recording.as_ref().map(|(r, _)| *r)
    }

    /// The last search, while its matches are highlighted.
    pub fn highlighted(&self) -> Option<&Regex> {
        self.search.as_ref().filter(|_| self.highlight).map(|s| &s.re)
    }

    /// Another file is shown: its marks are not this one's, and commands in progress stop.
    pub fn switch_buffer(&mut self) {
        self.mode = Mode::Normal;
        self.pending.clear();
        self.line = None;
        self.insert = None;
        self.marks.clear();
        self.jumps.clear();
        self.jump_at = 0;
        self.cursor = 0;
        self.anchor = 0;
        self.written = None;
    }

    /// Takes a selection made outside at once (a click places the caret, a drag or a double click
    /// selects in Visual mode), rather than with the next key.
    pub fn sync(&mut self, ctx: &mut Ctx) {
        self.adopt(ctx);
        self.write(ctx);
    }

    /// A command outside Vim acted on Visual mode's selection (commenting it, copying it from a
    /// menu): Vim goes back to Normal mode as after an operator, with the caret at the selection's
    /// start and the `'<` and `'>` marks on it (`gv` selects it again).
    pub fn end_visual(&mut self, ctx: &mut Ctx) {
        self.adopt(ctx);
        if self.mode.is_visual() {
            self.pending.clear();
            self.leave_visual(ctx);
        }
        self.write(ctx);
    }

    /// Handles a key.
    pub fn key(&mut self, key: Key, ctx: &mut Ctx) -> Outcome {
        self.adopt(ctx);
        let mut out = Outcome { handled: true, effects: Vec::new() };
        if let Some((_, keys)) = &mut self.recording {
            keys.push(key);
        }
        self.key_inner(key, ctx, &mut out);
        self.write(ctx);
        out
    }

    fn key_inner(&mut self, key: Key, ctx: &mut Ctx, out: &mut Outcome) {
        if self.line.is_some() {
            self.line_key(key, ctx, out);
        } else if self.mode.is_inserting() {
            self.insert_key(key, ctx, out);
        } else {
            self.normal_key(key, ctx, out);
        }
    }

    /// Notices a selection made outside (a click, a drag, a reload).
    fn adopt(&mut self, ctx: &mut Ctx) {
        let len = ctx.buffer.len_chars();
        if self.written.as_ref() != Some(&*ctx.selection) {
            let r = ctx.selection.primary();
            if r.is_empty() {
                self.cursor = r.head;
                if self.mode.is_visual() {
                    self.mode = Mode::Normal;
                }
            } else {
                // A selection made in Insert mode (a drag) ends the insert, as Escape would, so
                // that the selection is Visual mode's and the commands that act on one see it.
                if self.mode.is_inserting() {
                    self.finish_insert(ctx);
                }
                self.mode = Mode::Visual;
                if r.head > r.anchor {
                    (self.anchor, self.cursor) = (r.anchor, r.head - 1);
                } else {
                    (self.anchor, self.cursor) = (r.anchor.saturating_sub(1), r.head);
                }
            }
            self.want = None;
        }
        self.cursor = self.cursor.min(len);
        self.anchor = self.anchor.min(len);
    }

    /// Sets the selection from Vim's state.
    fn write(&mut self, ctx: &mut Ctx) {
        let b = &*ctx.buffer;
        let len = b.len_chars();
        if self.mode == Mode::Normal && self.line.is_none() {
            let cursor = self.cursor.min(len);
            self.cursor = cursor.min(line_last(b, b.char_to_line(cursor)));
        }
        self.cursor = self.cursor.min(len);
        let sel = match self.mode {
            Mode::Visual => {
                if self.cursor >= self.anchor {
                    Selection::single(self.anchor, (self.cursor + 1).min(len))
                } else {
                    Selection::single((self.anchor + 1).min(len), self.cursor)
                }
            }
            Mode::VisualLine => {
                let (la, lc) = (b.char_to_line(self.anchor), b.char_to_line(self.cursor));
                let line_after = |l: usize| (b.line_end(l) + 1).min(len);
                if lc >= la { Selection::single(b.line_to_char(la), line_after(lc)) } else { Selection::single(line_after(la), b.line_to_char(lc)) }
            }
            _ => Selection::point(self.cursor),
        };
        *ctx.selection = sel.clone();
        self.written = Some(sel);
    }

    // ----- Normal and Visual modes -----

    fn normal_key(&mut self, key: Key, ctx: &mut Ctx, out: &mut Outcome) {
        if self.pending.is_empty() {
            self.cmd_keys.clear();
            self.cmd_version = ctx.buffer.version();
        }
        self.pending.push(key);
        self.cmd_keys.push(key);
        match parse::command(&self.pending, self.mode.is_visual(), self.recording.is_some()) {
            Parse::More => {}
            Parse::Invalid => {
                // A shortcut Vim does not use (Ctrl+S) goes to the window.
                if self.pending.len() == 1 && matches!(key, Key::Ctrl(_)) {
                    out.handled = false;
                }
                self.pending.clear();
            }
            Parse::Done(parsed) => {
                self.pending.clear();
                let visual = self.mode.is_visual();
                let grouped = !matches!(parsed.cmd, Cmd::Simple(Simple::Undo | Simple::Redo | Simple::Repeat | Simple::Play(_)));
                if grouped {
                    ctx.buffer.begin_group();
                }
                self.execute(parsed, ctx, out);
                if self.mode.is_inserting() {
                    if self.replaying == 0 || self.dot_rec.is_none() {
                        self.dot_rec = Some(self.cmd_keys.clone());
                    }
                } else {
                    if grouped {
                        ctx.buffer.end_group();
                    }
                    let changed = ctx.buffer.version() != self.cmd_version;
                    let repeatable = !matches!(parsed.cmd, Cmd::Simple(Simple::Undo | Simple::Redo | Simple::Repeat | Simple::Play(_)));
                    if changed {
                        self.set_mark('.', self.cursor, ctx);
                        if repeatable && !visual && self.replaying == 0 {
                            self.dot = self.cmd_keys.clone();
                        }
                    }
                    if self.oneshot && self.mode == Mode::Normal {
                        self.oneshot = false;
                        self.mode = Mode::Insert;
                    }
                }
            }
        }
    }

    fn execute(&mut self, p: Parsed, ctx: &mut Ctx, out: &mut Outcome) {
        let keep_want = matches!(p.cmd, Cmd::Move(Motion::Up | Motion::Down | Motion::LineEnd));
        match p.cmd {
            Cmd::Move(m) => {
                if let Some(moved) = self.run_motion(m, p.count, false, ctx, out) {
                    if moved.jump {
                        self.record_jump(ctx);
                    }
                    self.cursor = moved.to;
                }
            }
            Cmd::Operate(op, target) if self.mode.is_visual() => self.visual_operate(op, target, p, ctx, out),
            Cmd::Operate(op, target) => {
                let Some(region) = self.target_region(op, target, p.count, ctx, out) else { return };
                self.operate(op, region, p.register, ctx, out);
            }
            Cmd::Simple(s) => self.simple(s, p, ctx, out),
        }
        if !keep_want {
            self.want = None;
        }
    }

    fn target_region(&mut self, op: Op, target: Target, count: usize, ctx: &mut Ctx, out: &mut Outcome) -> Option<Region> {
        let b = &*ctx.buffer;
        let n = count.max(1);
        let pos = self.cursor;
        match target {
            Target::Lines => {
                let line = b.char_to_line(pos);
                let last = (line + n - 1).min(b.len_lines().saturating_sub(1));
                Some(Region::Lines(line, last))
            }
            Target::Object { around, kind } => {
                let span = object::select(b, pos, kind, around, n)?;
                if span.linewise {
                    let last = b.char_to_line(span.end.saturating_sub(1).max(span.start));
                    Some(Region::Lines(b.char_to_line(span.start), last))
                } else {
                    Some(Region::Chars(span.start, span.end))
                }
            }
            Target::Motion(m) => {
                // `cw` on a word changes to its end, like `ce` (`:help cw`).
                let m = match (op, m) {
                    (Op::Change, Motion::WordStart(big)) if b.char_at(pos).is_some_and(|c| !c.is_whitespace()) => Motion::WordEnd(big),
                    _ => m,
                };
                let cw_on_word_end = matches!(m, Motion::WordEnd(_)) && op == Op::Change;
                let mut moved = if cw_on_word_end && self.at_word_end(ctx) && n == 1 {
                    Moved { to: pos, kind: Kind::Inclusive, jump: false }
                } else {
                    self.run_motion(m, count, true, ctx, out)?
                };
                let b = &*ctx.buffer;
                if let Motion::WordStart(_) = m {
                    // The last word of a line: the operator stops at the line's end.
                    let from_line = b.char_to_line(pos);
                    if b.char_to_line(moved.to) > from_line || moved.to >= b.len_chars() {
                        let mut prev = pos;
                        let big = matches!(m, Motion::WordStart(true));
                        for _ in 1..n {
                            prev = motion::word_start(b, prev, big);
                        }
                        let line = b.char_to_line(prev);
                        if b.char_to_line(moved.to) > line {
                            moved.to = b.line_end(line).max(prev);
                        }
                    }
                }
                Some(match moved.kind {
                    Kind::Linewise => Region::Lines(b.char_to_line(pos.min(moved.to)), b.char_to_line(pos.max(moved.to))),
                    Kind::Inclusive => Region::Chars(pos.min(moved.to), (pos.max(moved.to) + 1).min(b.len_chars())),
                    Kind::Exclusive => {
                        let (start, mut end) = (pos.min(moved.to), pos.max(moved.to));
                        // `:help exclusive`: an end in column 0 moves to the end of the line before.
                        let (ls, le) = (b.char_to_line(start), b.char_to_line(end));
                        if end > start && le > ls && column(b, end) == 0 {
                            if start <= first_non_blank(b, ls) && le > 0 {
                                return Some(Region::Lines(ls, le - 1));
                            }
                            end = b.line_end(le - 1);
                        }
                        Region::Chars(start, end)
                    }
                })
            }
        }
    }

    fn at_word_end(&self, ctx: &Ctx) -> bool {
        let b = &*ctx.buffer;
        let here = b.char_at(self.cursor).map(|c| motion::class(c, false));
        let next = b.char_at(self.cursor + 1).map(|c| motion::class(c, false));
        here != next
    }

    fn run_motion(&mut self, m: Motion, count: usize, op: bool, ctx: &mut Ctx, out: &mut Outcome) -> Option<Moved> {
        let b = &*ctx.buffer;
        let n = count.max(1);
        let pos = self.cursor;
        let line = b.char_to_line(pos);
        let last_line = b.len_lines().saturating_sub(1);
        let exclusive = |to| Some(Moved { to, kind: Kind::Exclusive, jump: false });
        let inclusive = |to| Some(Moved { to, kind: Kind::Inclusive, jump: false });
        let linewise = |line: usize, jump| Some(Moved { to: first_non_blank(b, line), kind: Kind::Linewise, jump });
        match m {
            Motion::Left => {
                let col = column(b, pos);
                if col == 0 {
                    return None;
                }
                exclusive(pos - n.min(col))
            }
            Motion::Right => {
                let end = if op || self.mode.is_visual() { b.line_end(line) } else { line_last(b, line) };
                if pos >= end {
                    return None;
                }
                exclusive((pos + n).min(end))
            }
            Motion::BackWrap => {
                let mut p = pos;
                for _ in 0..n {
                    let l = b.char_to_line(p);
                    if column(b, p) > 0 {
                        p -= 1;
                    } else if l > 0 {
                        p = line_last(b, l - 1);
                    }
                }
                exclusive(p)
            }
            Motion::ForwardWrap => {
                let mut p = pos;
                for _ in 0..n {
                    let l = b.char_to_line(p);
                    if p < line_last(b, l) {
                        p += 1;
                    } else if l < last_line {
                        p = b.line_to_char(l + 1);
                    }
                }
                exclusive(p)
            }
            Motion::Down | Motion::Up => {
                let target = if m == Motion::Down { (line + n).min(last_line) } else { line.saturating_sub(n) };
                if target == line {
                    return None;
                }
                let col = self.want.unwrap_or_else(|| column(b, pos));
                self.want = Some(col);
                Some(Moved { to: at_column(b, target, col, false), kind: Kind::Linewise, jump: false })
            }
            Motion::DownFirst => {
                let target = (line + n).min(last_line);
                if target == line { None } else { linewise(target, false) }
            }
            Motion::UpFirst => {
                if line == 0 {
                    None
                } else {
                    linewise(line.saturating_sub(n), false)
                }
            }
            Motion::WordStart(big) => {
                let mut p = pos;
                for _ in 0..n {
                    p = motion::word_start(b, p, big);
                }
                exclusive(p)
            }
            Motion::WordEnd(big) => {
                let mut p = pos;
                for _ in 0..n {
                    p = motion::word_end(b, p, big);
                }
                inclusive(p)
            }
            Motion::WordBack(big) => {
                let mut p = pos;
                for _ in 0..n {
                    p = motion::word_back(b, p, big);
                }
                exclusive(p)
            }
            Motion::WordEndBack(big) => {
                let mut p = pos;
                for _ in 0..n {
                    p = motion::word_end_back(b, p, big);
                }
                inclusive(p)
            }
            Motion::LineStart => exclusive(b.line_to_char(line)),
            Motion::FirstNonBlank => exclusive(first_non_blank(b, line)),
            Motion::LineEnd => {
                let target = (line + n - 1).min(last_line);
                self.want = Some(usize::MAX);
                if b.line(target).is_empty() {
                    return exclusive(b.line_to_char(target));
                }
                inclusive(b.line_end(target) - 1)
            }
            Motion::LastNonBlank => {
                let target = (line + n - 1).min(last_line);
                let start = b.line_to_char(target);
                let text: Vec<char> = b.line(target).chars().collect();
                let last = text.iter().rposition(|c| !c.is_whitespace()).unwrap_or(0);
                inclusive(start + last)
            }
            Motion::Column => exclusive(at_column(b, line, n - 1, false)),
            Motion::GotoLast => linewise(if count > 0 { (count - 1).min(last_line) } else { last_line }, true),
            Motion::GotoFirst => linewise(if count > 0 { (count - 1).min(last_line) } else { 0 }, true),
            Motion::Find { target, forward, till } => {
                self.last_find = Some((target, forward, till));
                let to = motion::find_in_line(b, pos, target, forward, till, n, false)?;
                if forward { inclusive(to) } else { exclusive(to) }
            }
            Motion::RepeatFind { reverse } => {
                let (target, forward, till) = self.last_find?;
                let forward = forward != reverse;
                let to = motion::find_in_line(b, pos, target, forward, till, n, true)?;
                if forward { inclusive(to) } else { exclusive(to) }
            }
            Motion::Percent => {
                if count > 0 {
                    let target = (count.min(100) * b.len_lines()).div_ceil(100).saturating_sub(1);
                    return linewise(target.min(last_line), true);
                }
                Some(Moved { to: motion::percent(b, pos)?, kind: Kind::Inclusive, jump: true })
            }
            Motion::ParagraphForward | Motion::ParagraphBack => {
                Some(Moved { to: motion::paragraph(b, pos, m == Motion::ParagraphForward, n), kind: Kind::Exclusive, jump: true })
            }
            Motion::ViewTop | Motion::ViewMiddle | Motion::ViewBottom => {
                let top = ctx.view_top.min(last_line);
                let bottom = (top + ctx.view_rows.max(1) - 1).min(last_line);
                let target = match m {
                    Motion::ViewTop => (top + n - 1).min(bottom),
                    Motion::ViewBottom => bottom.saturating_sub(n - 1).max(top),
                    _ => top + (bottom - top) / 2,
                };
                linewise(target, true)
            }
            Motion::Mark { name, exact } => {
                let Some(at) = self.mark_pos(name, ctx) else {
                    out.effects.push(Effect::Error(format!("The mark {name} is not set")));
                    return None;
                };
                let b = &*ctx.buffer;
                if exact {
                    Some(Moved { to: at, kind: Kind::Exclusive, jump: true })
                } else {
                    Some(Moved { to: first_non_blank(b, b.char_to_line(at)), kind: Kind::Linewise, jump: true })
                }
            }
            Motion::SearchNext { reverse } => {
                let Some(search) = self.search.clone() else {
                    out.effects.push(Effect::Error("There is no earlier search".to_owned()));
                    return None;
                };
                self.highlight = true;
                let forward = search.forward != reverse;
                let mut p = pos;
                for _ in 0..n {
                    p = find(ctx.buffer, &search.re, p, forward, out)?;
                }
                if p == pos && n > 0 && find(ctx.buffer, &search.re, p, forward, &mut Outcome::default()).is_none() {
                    return None;
                }
                Some(Moved { to: p, kind: Kind::Exclusive, jump: true })
            }
            Motion::SearchWord { forward } => {
                let span = (pos..b.line_end(line))
                    .find_map(|p| b.char_at(p).filter(|c| c.is_alphanumeric() || *c == '_').and_then(|_| object::select(b, p, 'w', false, 1)));
                let Some(span) = span else {
                    out.effects.push(Effect::Error("There is no word under the caret".to_owned()));
                    return None;
                };
                let word = b.slice(span.start, span.end);
                let source = format!("\\<{word}\\>");
                let re = match ex::pattern(&source, self.ignorecase, false) {
                    Ok(re) => re,
                    Err(e) => {
                        out.effects.push(Effect::Error(e));
                        return None;
                    }
                };
                self.search = Some(Search { source, forward, re: re.clone() });
                self.highlight = true;
                let mut p = span.start;
                for _ in 0..n {
                    p = find(ctx.buffer, &re, p, forward, out)?;
                }
                Some(Moved { to: p, kind: Kind::Exclusive, jump: true })
            }
        }
    }

    // ----- Operators -----

    fn operate(&mut self, op: Op, region: Region, register: Option<char>, ctx: &mut Ctx, out: &mut Outcome) {
        let b = &*ctx.buffer;
        let len = b.len_chars();
        match (op, region) {
            (Op::Reindent, _) => out.effects.push(Effect::Error("= (re-indent) is not supported yet".to_owned())),
            (Op::Yank, Region::Chars(s, e)) => {
                let text = b.slice(s, e);
                self.store(register, text, false, true, out);
                self.cursor = s.min(self.cursor);
            }
            (Op::Yank, Region::Lines(first, last)) => {
                let text = lines_text(b, first, last);
                self.store(register, text, true, true, out);
                let col = column(b, self.cursor);
                if b.char_to_line(self.cursor) > first {
                    self.cursor = at_column(b, first, col, false);
                }
                if last > first + 1 {
                    out.effects.push(Effect::Message(format!("{} lines yanked", last - first + 1)));
                }
            }
            (Op::Delete, Region::Chars(s, e)) => {
                if s >= e {
                    return;
                }
                let text = b.slice(s, e);
                self.store(register, text, false, false, out);
                self.edit(ctx, s, e, "");
                self.cursor = s;
            }
            (Op::Delete, Region::Lines(first, last)) => {
                let text = lines_text(b, first, last);
                let (mut s, e) = (b.line_to_char(first), b.line_to_char(last + 1).min(len));
                let through_end = last + 1 >= b.len_lines();
                if through_end && first > 0 {
                    s = b.line_end(first - 1);
                }
                self.store(register, text, true, false, out);
                self.edit(ctx, s, e, "");
                let b = &*ctx.buffer;
                let line = first.min(b.len_lines().saturating_sub(1));
                self.cursor = first_non_blank(b, line);
                if last > first + 1 {
                    out.effects.push(Effect::Message(format!("{} fewer lines", last - first + 1)));
                }
            }
            (Op::Change, Region::Chars(s, e)) => {
                let text = b.slice(s, e);
                self.store(register, text, false, false, out);
                self.edit(ctx, s, e, "");
                self.start_insert(s, 1, Repeat::Here, false);
            }
            (Op::Change, Region::Lines(first, last)) => {
                let text = lines_text(b, first, last);
                let indent: String = b.line(first).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                let (s, e) = (b.line_to_char(first), b.line_end(last));
                self.store(register, text, true, false, out);
                self.edit(ctx, s, e, &indent);
                self.start_insert(s + indent.chars().count(), 1, Repeat::Here, false);
            }
            (Op::Indent | Op::Outdent, region) => {
                let (first, last) = region_lines(b, region);
                self.shift(first, last, op == Op::Indent, 1, ctx);
            }
            (Op::Lower | Op::Upper | Op::Toggle, region) => {
                let (s, e) = match region {
                    Region::Chars(s, e) => (s, e),
                    Region::Lines(first, last) => (b.line_to_char(first), b.line_end(last)),
                };
                let text: String = b.slice(s, e).chars().map(|c| change_case(c, op)).collect();
                self.edit(ctx, s, e, &text);
                self.cursor = s;
            }
        }
    }

    fn shift(&mut self, first: usize, last: usize, right: bool, times: usize, ctx: &mut Ctx) {
        let b = &*ctx.buffer;
        let unit = ctx.indent.to_owned();
        let width = unit.chars().count().max(1);
        let mut changes = Vec::new();
        for l in first..=last {
            let text = b.line(l);
            if text.is_empty() {
                continue;
            }
            let start = b.line_to_char(l);
            if right {
                changes.push(Change { start, end: start, text: unit.repeat(times) });
            } else {
                let mut remove = 0;
                let mut cols = 0;
                for c in text.chars() {
                    if cols >= width * times {
                        break;
                    }
                    match c {
                        '\t' => cols += width,
                        ' ' => cols += 1,
                        _ => break,
                    }
                    remove += 1;
                }
                if remove > 0 {
                    changes.push(Change { start, end: start + remove, text: String::new() });
                }
            }
        }
        if let Ok(tx) = Transaction::new(changes) {
            let sel = Selection::point(self.cursor);
            let _ = ctx.buffer.edit(tx, &sel, &sel, EditKind::Other);
        }
        self.cursor = first_non_blank(ctx.buffer, first);
    }

    fn visual_operate(&mut self, op: Op, target: Target, p: Parsed, ctx: &mut Ctx, out: &mut Outcome) {
        if let Target::Object { around, kind } = target {
            let b = &*ctx.buffer;
            let Some(span) = object::select(b, self.cursor, kind, around, p.count.max(1)) else { return };
            if self.anchor == self.cursor || span.start < self.anchor.min(self.cursor) {
                self.anchor = span.start;
            }
            self.cursor = span.end.saturating_sub(1).max(span.start);
            if span.linewise {
                self.mode = Mode::VisualLine;
            }
            return;
        }
        let region = self.visual_region(ctx);
        self.leave_visual(ctx);
        match op {
            Op::Indent | Op::Outdent => {
                let (first, last) = region_lines(ctx.buffer, region);
                self.shift(first, last, op == Op::Indent, p.count.max(1), ctx);
            }
            _ => self.operate(op, region, p.register, ctx, out),
        }
    }

    fn visual_region(&self, ctx: &Ctx) -> Region {
        let b = &*ctx.buffer;
        let (s, e) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
        match self.mode {
            Mode::VisualLine => Region::Lines(b.char_to_line(s), b.char_to_line(e)),
            _ => Region::Chars(s, (e + 1).min(b.len_chars())),
        }
    }

    fn leave_visual(&mut self, ctx: &mut Ctx) {
        let (s, e) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
        self.set_mark('<', s, ctx);
        self.set_mark('>', e, ctx);
        self.last_visual = Some(self.mode);
        self.mode = Mode::Normal;
        self.cursor = s;
    }

    // ----- Other commands -----

    fn simple(&mut self, s: Simple, p: Parsed, ctx: &mut Ctx, out: &mut Outcome) {
        let n = p.count.max(1);
        let b = &*ctx.buffer;
        let pos = self.cursor;
        let line = b.char_to_line(pos);
        let visual = self.mode.is_visual();
        match s {
            Simple::Cancel => {
                if visual {
                    self.leave_visual(ctx);
                    self.cursor = pos;
                }
            }
            Simple::DeleteChar => {
                let end = (pos + n).min(b.line_end(line));
                self.operate(Op::Delete, Region::Chars(pos, end), p.register, ctx, out);
            }
            Simple::DeleteCharBack => {
                let start = pos.saturating_sub(n).max(b.line_to_char(line));
                self.operate(Op::Delete, Region::Chars(start, pos), p.register, ctx, out);
            }
            Simple::Substitute => {
                let end = (pos + n).min(b.line_end(line));
                self.operate(Op::Change, Region::Chars(pos, end), p.register, ctx, out);
            }
            Simple::SubstituteLine => {
                let last = (line + n - 1).min(b.len_lines().saturating_sub(1));
                self.operate(Op::Change, Region::Lines(line, last), p.register, ctx, out);
            }
            Simple::DeleteToEnd | Simple::ChangeToEnd => {
                let last = (line + n - 1).min(b.len_lines().saturating_sub(1));
                let op = if s == Simple::DeleteToEnd { Op::Delete } else { Op::Change };
                self.operate(op, Region::Chars(pos, b.line_end(last)), p.register, ctx, out);
            }
            Simple::YankLine => {
                let last = (line + n - 1).min(b.len_lines().saturating_sub(1));
                self.operate(Op::Yank, Region::Lines(line, last), p.register, ctx, out);
            }
            Simple::VisualLines(op) => {
                self.mode = Mode::VisualLine;
                let region = self.visual_region(ctx);
                self.leave_visual(ctx);
                self.operate(op, region, p.register, ctx, out);
            }
            Simple::Replace(c) => {
                if visual {
                    let region = self.visual_region(ctx);
                    self.leave_visual(ctx);
                    let b = &*ctx.buffer;
                    let (s, e) = match region {
                        Region::Chars(s, e) => (s, e),
                        Region::Lines(f, l) => (b.line_to_char(f), b.line_end(l)),
                    };
                    let text: String = b.slice(s, e).chars().map(|x| if x == '\n' { x } else { c }).collect();
                    self.edit(ctx, s, e, &text);
                    self.cursor = s;
                    return;
                }
                if pos + n > b.line_end(line) {
                    return;
                }
                if c == '\n' {
                    self.edit(ctx, pos, pos + n, "\n");
                    self.cursor = pos + 1;
                } else {
                    self.edit(ctx, pos, pos + n, &c.to_string().repeat(n));
                    self.cursor = pos + n - 1;
                }
            }
            Simple::Join { spaces } => {
                let (first, count) = if visual {
                    let region = self.visual_region(ctx);
                    self.leave_visual(ctx);
                    let (f, l) = region_lines(ctx.buffer, region);
                    (f, (l - f + 1).max(2))
                } else {
                    (line, n.max(2))
                };
                self.join(first, count, spaces, ctx);
            }
            Simple::Put { before } => self.put(p.register, before, n, ctx, out),
            Simple::Undo | Simple::Redo => {
                for _ in 0..n {
                    let sel = if s == Simple::Undo { ctx.buffer.undo() } else { ctx.buffer.redo() };
                    match sel {
                        Some(sel) => self.cursor = sel.primary().start(),
                        None => {
                            let which = if s == Simple::Undo { "Already at the oldest change" } else { "Already at the newest change" };
                            out.effects.push(Effect::Message(which.to_owned()));
                            break;
                        }
                    }
                }
            }
            Simple::Repeat => self.repeat(p.count, ctx, out),
            Simple::ToggleCase => {
                if visual {
                    return;
                }
                let end = (pos + n).min(b.line_end(line));
                let text: String = b.slice(pos, end).chars().map(|c| change_case(c, Op::Toggle)).collect();
                self.edit(ctx, pos, end, &text);
                self.cursor = end;
            }
            Simple::Insert => self.start_insert(pos, n, Repeat::Here, false),
            Simple::Append => {
                let at = if b.line(line).is_empty() { pos } else { (pos + 1).min(b.line_end(line)) };
                self.start_insert(at, n, Repeat::Here, false);
            }
            Simple::InsertFirst => {
                let at = if visual {
                    let (s, _) = (self.anchor.min(self.cursor), 0);
                    self.leave_visual(ctx);
                    s
                } else {
                    first_non_blank(b, line)
                };
                self.start_insert(at, n, Repeat::Here, false);
            }
            Simple::InsertColumnZero => self.start_insert(b.line_to_char(line), n, Repeat::Here, false),
            Simple::AppendEnd => {
                let at = if visual {
                    let e = self.anchor.max(self.cursor);
                    self.leave_visual(ctx);
                    (e + 1).min(ctx.buffer.len_chars())
                } else {
                    b.line_end(line)
                };
                self.start_insert(at, n, Repeat::Here, false);
            }
            Simple::OpenBelow | Simple::OpenAbove => {
                let indent: String = b.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                let width = indent.chars().count();
                if s == Simple::OpenBelow {
                    let at = b.line_end(line);
                    self.edit(ctx, at, at, &format!("\n{indent}"));
                    self.start_insert(at + 1 + width, n, Repeat::Lines, false);
                } else {
                    let at = b.line_to_char(line);
                    self.edit(ctx, at, at, &format!("{indent}\n"));
                    self.start_insert(at + width, n, Repeat::Lines, false);
                }
            }
            Simple::ReplaceMode => self.start_insert(pos, n, Repeat::Here, true),
            Simple::Visual | Simple::VisualLine => {
                let want = if s == Simple::Visual { Mode::Visual } else { Mode::VisualLine };
                if self.mode == want {
                    self.leave_visual(ctx);
                    self.cursor = pos;
                } else {
                    if !visual {
                        self.anchor = pos;
                    }
                    self.mode = want;
                }
            }
            Simple::VisualBlock => out.effects.push(Effect::Error("Visual block mode is not supported yet".to_owned())),
            Simple::Reselect => {
                let (Some(s), Some(e), Some(mode)) = (self.mark_pos('<', ctx), self.mark_pos('>', ctx), self.last_visual) else {
                    out.effects.push(Effect::Error("There is no earlier selection".to_owned()));
                    return;
                };
                self.anchor = s;
                self.cursor = e;
                self.mode = mode;
            }
            Simple::SearchForward | Simple::SearchBackward => {
                self.line = Some((LineKind::Search { forward: s == Simple::SearchForward }, String::new()));
            }
            Simple::CommandLine => {
                let text = if visual {
                    self.leave_visual(ctx);
                    self.cursor = pos;
                    "'<,'>".to_owned()
                } else if p.count > 1 {
                    format!(".,.+{}", p.count - 1)
                } else {
                    String::new()
                };
                self.line = Some((LineKind::Command, text));
            }
            Simple::SetMark(c) => {
                if c.is_ascii_lowercase() || c == '\'' || c == '`' {
                    let c = if c == '`' { '\'' } else { c };
                    self.set_mark(c, pos, ctx);
                } else {
                    out.effects.push(Effect::Error(format!("Marks are a to z, not {c}")));
                }
            }
            Simple::Record(Some(c)) => {
                if c.is_ascii_alphanumeric() || c == '"' {
                    self.recording = Some((c.to_ascii_lowercase(), Vec::new()));
                } else {
                    out.effects.push(Effect::Error(format!("Cannot record into register {c}")));
                }
            }
            Simple::Record(None) => {
                if let Some((reg, mut keys)) = self.recording.take() {
                    keys.pop();
                    self.macros.insert(reg, keys);
                }
            }
            Simple::Play(c) => self.play(c, n, ctx, out),
            Simple::SaveQuit => {
                out.effects.push(Effect::Save);
                out.effects.push(Effect::Close { force: false });
            }
            Simple::QuitForce => out.effects.push(Effect::Close { force: true }),
            Simple::ScrollCenter => out.effects.push(Effect::Scroll(Scroll::Center)),
            Simple::ScrollTop => out.effects.push(Effect::Scroll(Scroll::Top)),
            Simple::ScrollBottom => out.effects.push(Effect::Scroll(Scroll::Bottom)),
            Simple::HalfDown | Simple::HalfUp | Simple::PageDown | Simple::PageUp => {
                let rows = ctx.view_rows.max(2);
                let amount = match s {
                    Simple::HalfDown | Simple::HalfUp if p.count > 0 => p.count,
                    Simple::HalfDown | Simple::HalfUp => rows / 2,
                    _ => (rows - 2).max(1) * n,
                } as isize;
                let amount = if matches!(s, Simple::HalfUp | Simple::PageUp) { -amount } else { amount };
                let last = b.len_lines().saturating_sub(1) as isize;
                let target = (line as isize + amount).clamp(0, last) as usize;
                self.cursor = first_non_blank(b, target);
                out.effects.push(Effect::Scroll(Scroll::Lines(amount)));
            }
            Simple::LineDown => out.effects.push(Effect::Scroll(Scroll::Lines(n as isize))),
            Simple::LineUp => out.effects.push(Effect::Scroll(Scroll::Lines(-(n as isize)))),
            Simple::JumpOlder => self.jump_older(n, ctx),
            Simple::JumpNewer => self.jump_newer(n, ctx),
            Simple::Increment(up) => self.increment(if up { n as i64 } else { -(n as i64) }, ctx),
            Simple::SwapEnds => std::mem::swap(&mut self.anchor, &mut self.cursor),
        }
    }

    fn join(&mut self, first: usize, count: usize, spaces: bool, ctx: &mut Ctx) {
        for _ in 0..count - 1 {
            let b = &*ctx.buffer;
            if first + 1 >= b.len_lines() {
                break;
            }
            let end = b.line_end(first);
            let mut next = end + 1;
            if spaces {
                while b.char_at(next).is_some_and(|c| c == ' ' || c == '\t') {
                    next += 1;
                }
            }
            let before = if end > b.line_to_char(first) { b.char_at(end - 1) } else { None };
            let after = b.char_at(next);
            let space = spaces && before.is_some_and(|c| !c.is_whitespace()) && after.is_some_and(|c| c != '\n' && c != ')');
            self.edit(ctx, end, next, if space { " " } else { "" });
            self.cursor = end;
        }
    }

    fn put(&mut self, register: Option<char>, before: bool, n: usize, ctx: &mut Ctx, out: &mut Outcome) {
        let Some(reg) = self.fetch(register, ctx) else {
            out.effects.push(Effect::Error(format!("The register {} is empty", register.unwrap_or('"'))));
            return;
        };
        if self.mode.is_visual() {
            let region = self.visual_region(ctx);
            self.leave_visual(ctx);
            let b = &*ctx.buffer;
            let (s, e) = match region {
                Region::Chars(s, e) => (s, e),
                Region::Lines(f, l) => (b.line_to_char(f), b.line_to_char(l + 1).min(b.len_chars())),
            };
            let old = b.slice(s, e);
            let mut text = reg.text.repeat(n);
            if reg.linewise && matches!(region, Region::Chars(..)) {
                text = format!("\n{text}");
            }
            self.edit(ctx, s, e, &text);
            self.store(None, old, matches!(region, Region::Lines(..)), false, out);
            self.cursor = s;
            return;
        }
        let b = &*ctx.buffer;
        let line = b.char_to_line(self.cursor);
        if reg.linewise {
            let mut text = reg.text.repeat(n);
            let at = if before {
                b.line_to_char(line)
            } else if line + 1 < b.len_lines() {
                b.line_to_char(line + 1)
            } else {
                // After the last line: the break goes first.
                text = format!("\n{}", text.strip_suffix('\n').unwrap_or(&text));
                b.len_chars()
            };
            let first = if text.starts_with('\n') && !before && line + 1 >= b.len_lines() { at + 1 } else { at };
            self.edit(ctx, at, at, &text);
            let b = &*ctx.buffer;
            self.cursor = first_non_blank(b, b.char_to_line(first));
        } else {
            let text = reg.text.repeat(n);
            let at = if before || b.line(line).is_empty() { self.cursor } else { (self.cursor + 1).min(b.line_end(line)) };
            self.edit(ctx, at, at, &text);
            let len = text.chars().count();
            self.cursor = if text.contains('\n') { at } else { at + len.saturating_sub(1) };
        }
    }

    fn increment(&mut self, by: i64, ctx: &mut Ctx) {
        let b = &*ctx.buffer;
        let line = b.char_to_line(self.cursor);
        let start = b.line_to_char(line);
        let chars: Vec<char> = b.line(line).chars().collect();
        let col = self.cursor - start;
        let Some(mut first) = (col..chars.len()).find(|i| chars[*i].is_ascii_digit()).or_else(|| (0..col).rev().find(|i| chars[*i].is_ascii_digit()))
        else {
            return;
        };
        while first > 0 && chars[first - 1].is_ascii_digit() {
            first -= 1;
        }
        let mut last = first;
        while last + 1 < chars.len() && chars[last + 1].is_ascii_digit() {
            last += 1;
        }
        let negative = first > 0 && chars[first - 1] == '-';
        let digits: String = chars[first..=last].iter().collect();
        let Ok(value) = digits.parse::<i64>() else { return };
        let value = if negative { -value } else { value };
        let new = value.saturating_add(by).to_string();
        let from = if negative { first - 1 } else { first };
        self.edit(ctx, start + from, start + last + 1, &new);
        self.cursor = start + from + new.chars().count() - 1;
    }

    // ----- Insert and Replace modes -----

    fn start_insert(&mut self, at: usize, count: usize, repeat: Repeat, replace: bool) {
        self.cursor = at;
        self.mode = if replace { Mode::Replace } else { Mode::Insert };
        self.insert = Some(InsertSession { count, repeat, typed: String::new(), replaced: Vec::new() });
    }

    fn insert_key(&mut self, key: Key, ctx: &mut Ctx, out: &mut Outcome) {
        if let Some(rec) = &mut self.dot_rec
            && self.replaying == 0
        {
            rec.push(key);
        }
        let at = ctx.now;
        if std::mem::take(&mut self.insert_register) {
            if let Key::Char(c) = key
                && let Some(reg) = self.fetch(Some(c), ctx)
            {
                self.type_text(&reg.text, ctx, at);
            }
            return;
        }
        let b = &*ctx.buffer;
        let pos = self.cursor;
        let line = b.char_to_line(pos);
        match key {
            Key::Esc | Key::Ctrl('[' | 'c') => self.leave_insert(ctx),
            Key::Char(c) => self.type_text(&c.to_string(), ctx, at),
            Key::Enter => {
                let indent: String = b.line(line).chars().take(column(b, pos)).take_while(|c| *c == ' ' || *c == '\t').collect();
                self.type_text(&format!("\n{indent}"), ctx, at);
            }
            Key::Tab => {
                let unit = ctx.indent.to_owned();
                self.type_text(&unit, ctx, at);
            }
            Key::Backspace | Key::Ctrl('h') => {
                if self.mode == Mode::Replace {
                    if pos == 0 {
                        return;
                    }
                    match self.insert.as_mut().and_then(|s| s.replaced.pop()) {
                        Some(Some(original)) => {
                            self.edit_kind(ctx, pos - 1, pos, &original.to_string(), EditKind::Deleting { at });
                        }
                        Some(None) => self.edit_kind(ctx, pos - 1, pos, "", EditKind::Deleting { at }),
                        None => {}
                    }
                    self.cursor = pos - 1;
                    return;
                }
                if pos > 0 {
                    self.edit_kind(ctx, pos - 1, pos, "", EditKind::Deleting { at });
                    self.cursor = pos - 1;
                    if let Some(s) = &mut self.insert {
                        s.typed.pop();
                    }
                }
            }
            Key::Delete => {
                if pos < b.len_chars() {
                    self.edit_kind(ctx, pos, pos + 1, "", EditKind::Deleting { at });
                }
            }
            Key::Ctrl('w') => {
                if pos == 0 {
                    return;
                }
                let start = b.line_to_char(line);
                let from = if pos == start {
                    pos - 1
                } else {
                    let mut p = pos;
                    while p > start && b.char_at(p - 1).is_some_and(|c| c == ' ' || c == '\t') {
                        p -= 1;
                    }
                    let class = b.char_at(p.saturating_sub(1)).map(|c| motion::class(c, false));
                    while p > start && b.char_at(p - 1).map(|c| motion::class(c, false)) == class {
                        p -= 1;
                    }
                    p
                };
                self.edit_kind(ctx, from, pos, "", EditKind::Deleting { at });
                self.cursor = from;
            }
            Key::Ctrl('u') => {
                let start = b.line_to_char(line);
                let first = first_non_blank(b, line);
                let from = if pos > first { first } else { start };
                self.edit_kind(ctx, from, pos, "", EditKind::Deleting { at });
                self.cursor = from;
            }
            Key::Ctrl('r') => self.insert_register = true,
            Key::Ctrl('o') => {
                self.oneshot = true;
                self.mode = Mode::Normal;
            }
            Key::Left => self.cursor = pos.saturating_sub(1).max(b.line_to_char(line)),
            Key::Right => self.cursor = (pos + 1).min(b.line_end(line)),
            // As in Vim, a word (`w`, `b`) for both Ctrl and Shift.
            Key::CtrlLeft | Key::ShiftLeft => self.cursor = motion::word_back(b, pos, false),
            Key::CtrlRight | Key::ShiftRight => self.cursor = motion::word_start(b, pos, false).min(b.len_chars()),
            Key::Up | Key::Down => {
                let target = if key == Key::Up { line.checked_sub(1) } else { Some(line + 1).filter(|l| *l < b.len_lines()) };
                if let Some(target) = target {
                    self.cursor = at_column(b, target, column(b, pos), true);
                }
            }
            Key::Home => self.cursor = b.line_to_char(line),
            Key::End => self.cursor = b.line_end(line),
            Key::PageUp | Key::PageDown => {}
            Key::Ctrl(_) => out.handled = false,
        }
    }

    /// Types `text` at the caret: inserted, or over the line's characters in Replace mode.
    fn type_text(&mut self, text: &str, ctx: &mut Ctx, at: f64) {
        let pos = self.cursor;
        let kind = EditKind::Typing { at };
        if self.mode == Mode::Replace && !text.contains('\n') {
            let b = &*ctx.buffer;
            let line_end = b.line_end(b.char_to_line(pos));
            let n = text.chars().count();
            let over = n.min(line_end - pos);
            let originals: Vec<Option<char>> = (0..n).map(|i| if i < over { b.char_at(pos + i) } else { None }).collect();
            self.edit_kind(ctx, pos, pos + over, text, kind);
            if let Some(s) = &mut self.insert {
                s.replaced.extend(originals);
            }
        } else {
            self.edit_kind(ctx, pos, pos, text, kind);
        }
        self.cursor = pos + text.chars().count();
        if let Some(s) = &mut self.insert {
            s.typed.push_str(text);
        }
    }

    fn leave_insert(&mut self, ctx: &mut Ctx) {
        if let Some(session) = self.insert.take()
            && session.count > 1
            && !session.typed.is_empty()
        {
            let b = &*ctx.buffer;
            match session.repeat {
                Repeat::Here => {
                    let text = session.typed.repeat(session.count - 1);
                    let pos = self.cursor;
                    self.edit(ctx, pos, pos, &text);
                    self.cursor = pos + text.chars().count();
                }
                Repeat::Lines => {
                    let at = b.line_end(b.char_to_line(self.cursor));
                    let indent: String = b.line(b.char_to_line(self.cursor)).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                    let text = format!("\n{indent}{}", session.typed).repeat(session.count - 1);
                    self.edit(ctx, at, at, &text);
                    self.cursor = at + text.chars().count();
                }
            }
        }
        let b = &*ctx.buffer;
        if column(b, self.cursor) > 0 {
            self.cursor -= 1;
        }
        self.finish_insert(ctx);
    }

    /// Back to Normal mode from Insert mode: the insert is one undo step and what `.` repeats.
    fn finish_insert(&mut self, ctx: &mut Ctx) {
        self.insert = None;
        self.mode = Mode::Normal;
        self.oneshot = false;
        ctx.buffer.end_group();
        if self.replaying == 0
            && let Some(keys) = self.dot_rec.take()
        {
            self.dot = keys;
        }
        let pos = self.cursor;
        self.set_mark('.', pos, ctx);
    }

    // ----- The command line -----

    fn line_key(&mut self, key: Key, ctx: &mut Ctx, out: &mut Outcome) {
        let Some((kind, text)) = self.line.as_mut() else { return };
        match key {
            Key::Esc | Key::Ctrl('[' | 'c') => self.line = None,
            Key::Char(c) => text.push(c),
            // The guard deletes the last character; with none left, Backspace leaves the line.
            Key::Backspace | Key::Ctrl('h') if text.pop().is_none() => self.line = None,
            Key::Backspace | Key::Ctrl('h') => {}
            Key::Ctrl('u') => text.clear(),
            Key::Ctrl('w') => {
                let trimmed = text.trim_end().len();
                let keep = text[..trimmed].rfind(|c: char| !c.is_alphanumeric() && c != '_').map_or(0, |i| i + 1);
                text.truncate(keep);
            }
            Key::Enter => {
                let (kind, text) = (*kind, std::mem::take(text));
                self.line = None;
                match kind {
                    LineKind::Command => self.ex(&text, ctx, out),
                    LineKind::Search { forward } => self.search_for(&text, forward, ctx, out),
                }
            }
            _ => {}
        }
    }

    fn search_for(&mut self, text: &str, forward: bool, ctx: &mut Ctx, out: &mut Outcome) {
        let source = if text.is_empty() {
            match &self.search {
                Some(s) => s.source.clone(),
                None => {
                    out.effects.push(Effect::Error("There is no earlier search".to_owned()));
                    return;
                }
            }
        } else {
            text.to_owned()
        };
        let re = match ex::pattern(&source, self.ignorecase, self.smartcase) {
            Ok(re) => re,
            Err(e) => {
                out.effects.push(Effect::Error(e));
                return;
            }
        };
        self.search = Some(Search { source, forward, re: re.clone() });
        self.highlight = true;
        if let Some(to) = find(ctx.buffer, &re, self.cursor, forward, out) {
            if !self.mode.is_visual() {
                self.record_jump(ctx);
            }
            self.cursor = to;
        }
    }

    fn ex(&mut self, text: &str, ctx: &mut Ctx, out: &mut Outcome) {
        if text.trim().is_empty() {
            return;
        }
        self.last_ex = Some(text.to_owned());
        let line = ex::parse(text);
        let current = ctx.buffer.char_to_line(self.cursor);
        let (first, last) = match &line.range {
            None => (current, current),
            Some(range) => {
                let Some(from) = self.address(&range.from, ctx, out) else { return };
                let to = match &range.to {
                    Some(to) => match self.address(to, ctx, out) {
                        Some(t) => t,
                        None => return,
                    },
                    None => from,
                };
                (from.min(to), from.max(to))
            }
        };
        let b = &*ctx.buffer;
        let name = line.name.as_str();
        match name {
            "" => {
                if line.range.is_some() {
                    self.record_jump(ctx);
                    self.cursor = first_non_blank(ctx.buffer, last);
                }
            }
            "w" | "write" | "up" | "update" => {
                if line.args.is_empty() {
                    out.effects.push(Effect::Save);
                } else {
                    out.effects
                        .push(Effect::Error("Saving under another name from the command line is not supported yet; use File › Save As".to_owned()));
                }
            }
            "wa" | "wall" => out.effects.push(Effect::SaveAll),
            "q" | "quit" | "clo" | "close" => out.effects.push(Effect::Close { force: line.bang }),
            "wq" | "x" | "xit" | "exi" | "exit" => {
                out.effects.push(Effect::Save);
                out.effects.push(Effect::Close { force: false });
            }
            "e" | "edit" => {
                if line.args.is_empty() {
                    out.effects.push(Effect::Error("Give a file name: :e FILE".to_owned()));
                } else {
                    out.effects.push(Effect::Open(line.args.clone()));
                }
            }
            "s" | "su" | "substitute" => self.substitute(&line.args, first, last, ctx, out),
            "comment" => {
                let (start, end) = (b.line_to_char(first), b.line_end(last));
                out.effects.push(Effect::Comment { start, end, text: line.args.trim().to_owned() });
            }
            "noh" | "nohlsearch" => self.highlight = false,
            "d" | "de" | "del" | "delete" => {
                let reg = line.args.chars().next().filter(char::is_ascii_alphabetic);
                self.operate(Op::Delete, Region::Lines(first, last), reg, ctx, out);
            }
            "y" | "ya" | "yank" => {
                let reg = line.args.chars().next().filter(char::is_ascii_alphabetic);
                let keep = self.cursor;
                self.operate(Op::Yank, Region::Lines(first, last), reg, ctx, out);
                self.cursor = keep;
            }
            "j" | "join" => {
                let count = if line.range.as_ref().is_some_and(|r| r.to.is_some()) { last - first + 1 } else { 2 };
                self.join(first, count, !line.bang, ctx);
            }
            ">" | "<" => self.shift(first, last, name == ">", 1, ctx),
            "u" | "un" | "undo" => {
                if let Some(sel) = ctx.buffer.undo() {
                    self.cursor = sel.primary().start();
                }
            }
            "red" | "redo" => {
                if let Some(sel) = ctx.buffer.redo() {
                    self.cursor = sel.primary().start();
                }
            }
            "norm" | "normal" => {
                if self.replaying >= MAX_REPLAY_DEPTH {
                    return;
                }
                let keys: Vec<Key> = line.args.chars().map(Key::Char).collect();
                self.replaying += 1;
                for l in first..=last.min(b.len_lines().saturating_sub(1)) {
                    self.cursor = ctx.buffer.line_to_char(l);
                    self.mode = Mode::Normal;
                    for k in &keys {
                        self.key_inner(*k, ctx, out);
                    }
                    if self.mode.is_inserting() {
                        self.leave_insert(ctx);
                    }
                    self.pending.clear();
                }
                self.replaying -= 1;
            }
            "se" | "set" => {
                for word in line.args.split_whitespace() {
                    match word {
                        "ic" | "ignorecase" => self.ignorecase = true,
                        "noic" | "noignorecase" => self.ignorecase = false,
                        "scs" | "smartcase" => self.smartcase = true,
                        "noscs" | "nosmartcase" => self.smartcase = false,
                        "hls" | "hlsearch" => self.highlight = true,
                        "nohls" | "nohlsearch" => self.highlight = false,
                        "suggest" => out.effects.push(Effect::Suggest(true)),
                        "nosuggest" => out.effects.push(Effect::Suggest(false)),
                        _ => out.effects.push(Effect::Error(format!("Unknown option: {word}"))),
                    }
                }
            }
            _ => out.effects.push(Effect::Error(format!("Not an editor command: {}", text.trim()))),
        }
    }

    fn address(&self, a: &(ex::Address, isize), ctx: &Ctx, out: &mut Outcome) -> Option<usize> {
        let b = &*ctx.buffer;
        let last = b.len_lines().saturating_sub(1) as isize;
        let base = match a.0 {
            ex::Address::Current => b.char_to_line(self.cursor) as isize,
            ex::Address::Last => last,
            ex::Address::Line(n) => n as isize - 1,
            ex::Address::Mark(m) => match self.mark_pos(m, ctx) {
                Some(p) => b.char_to_line(p) as isize,
                None => {
                    out.effects.push(Effect::Error(format!("The mark {m} is not set")));
                    return None;
                }
            },
        };
        Some((base + a.1).clamp(0, last) as usize)
    }

    fn substitute(&mut self, args: &str, first: usize, last: usize, ctx: &mut Ctx, out: &mut Outcome) {
        let Some((pat, rep, flags)) = ex::substitute_args(args) else {
            out.effects.push(Effect::Error("Use :s/pattern/replacement/flags".to_owned()));
            return;
        };
        if flags.contains('c') {
            out.effects.push(Effect::Error("Confirming each substitution (the c flag) is not supported yet".to_owned()));
            return;
        }
        let source = if pat.is_empty() {
            match &self.search {
                Some(s) => s.source.clone(),
                None => {
                    out.effects.push(Effect::Error("There is no earlier search".to_owned()));
                    return;
                }
            }
        } else {
            pat
        };
        let ignore = if flags.contains('i') {
            true
        } else if flags.contains('I') {
            false
        } else {
            self.ignorecase
        };
        let re = match ex::pattern(&source, ignore, self.smartcase && !flags.contains('i')) {
            Ok(re) => re,
            Err(e) => {
                out.effects.push(Effect::Error(e));
                return;
            }
        };
        let rep = match ex::replacement(&rep) {
            Ok(r) => r,
            Err(e) => {
                out.effects.push(Effect::Error(e));
                return;
            }
        };
        self.search = Some(Search { source: source.clone(), forward: true, re: re.clone() });
        let global = flags.contains('g');
        let b = &*ctx.buffer;
        let mut changes = Vec::new();
        let (mut count, mut last_line) = (0, None);
        for l in first..=last.min(b.len_lines().saturating_sub(1)) {
            let old = b.line(l);
            let found = if global { re.find_iter(&old).count() } else { usize::from(re.is_match(&old)) };
            if found == 0 {
                continue;
            }
            let new = if global { re.replace_all(&old, rep.as_str()) } else { re.replace(&old, rep.as_str()) };
            count += found;
            last_line = Some(l);
            changes.push(Change { start: b.line_to_char(l), end: b.line_end(l), text: new.into_owned() });
        }
        let lines = changes.len();
        let Some(last_line) = last_line else {
            if !flags.contains('e') {
                out.effects.push(Effect::Error(format!("Pattern not found: {source}")));
            }
            return;
        };
        let last_start = changes.iter().map(|c| c.start).max().unwrap_or(0);
        if let Ok(tx) = Transaction::new(changes) {
            let sel = Selection::point(self.cursor);
            let after = Selection::point(last_start);
            let _ = ctx.buffer.edit(tx, &sel, &after, EditKind::Other);
        }
        let b = &*ctx.buffer;
        self.cursor = first_non_blank(b, last_line.min(b.len_lines().saturating_sub(1)));
        if lines > 1 {
            out.effects.push(Effect::Message(format!("{count} substitutions on {lines} lines")));
        }
    }

    // ----- Repeating -----

    fn repeat(&mut self, count: usize, ctx: &mut Ctx, out: &mut Outcome) {
        if self.dot.is_empty() || self.replaying >= MAX_REPLAY_DEPTH {
            return;
        }
        let mut keys = self.dot.clone();
        if count > 0 {
            let skip = if keys.first() == Some(&Key::Char('"')) { 2 } else { 0 };
            let mut end = skip;
            while matches!(keys.get(end), Some(Key::Char('0'..='9'))) {
                end += 1;
            }
            let digits = count.to_string().chars().map(Key::Char).collect::<Vec<_>>();
            keys.splice(skip..end, digits);
        }
        self.replaying += 1;
        for k in keys {
            self.key_inner(k, ctx, out);
        }
        self.replaying -= 1;
    }

    fn play(&mut self, reg: char, n: usize, ctx: &mut Ctx, out: &mut Outcome) {
        if self.replaying >= MAX_REPLAY_DEPTH {
            return;
        }
        if reg == ':' {
            if let Some(text) = self.last_ex.clone() {
                for _ in 0..n {
                    self.ex(&text, ctx, out);
                }
            }
            return;
        }
        let reg = if reg == '@' {
            match self.last_macro {
                Some(r) => r,
                None => return,
            }
        } else {
            reg.to_ascii_lowercase()
        };
        let Some(keys) = self.macros.get(&reg).cloned() else {
            out.effects.push(Effect::Error(format!("The register {reg} has no recording")));
            return;
        };
        self.last_macro = Some(reg);
        self.replaying += 1;
        for _ in 0..n {
            for k in &keys {
                self.key_inner(*k, ctx, out);
            }
        }
        self.replaying -= 1;
    }

    // ----- Registers, marks and jumps -----

    fn store(&mut self, register: Option<char>, text: String, linewise: bool, yank: bool, out: &mut Outcome) {
        let reg = Reg { text, linewise };
        match register {
            Some('_') => return,
            Some('+' | '*') => {
                out.effects.push(Effect::Copy(reg.text.clone()));
                self.regs.insert('+', reg.clone());
            }
            Some(c) if c.is_ascii_uppercase() => {
                let lower = c.to_ascii_lowercase();
                let entry = self.regs.entry(lower).or_insert_with(|| Reg { text: String::new(), linewise });
                entry.text.push_str(&reg.text);
                entry.linewise |= linewise;
                let appended = entry.clone();
                self.regs.insert('"', appended);
                return;
            }
            Some(c) if c != '"' => {
                self.regs.insert(c, reg.clone());
            }
            _ => {
                if yank {
                    self.regs.insert('0', reg.clone());
                } else if linewise || reg.text.contains('\n') {
                    for i in (1..9).rev() {
                        let (from, to) = (char::from(b'0' + i), char::from(b'0' + i + 1));
                        if let Some(r) = self.regs.remove(&from) {
                            self.regs.insert(to, r);
                        }
                    }
                    self.regs.insert('1', reg.clone());
                } else {
                    self.regs.insert('-', reg.clone());
                }
            }
        }
        self.regs.insert('"', reg);
    }

    fn fetch(&self, register: Option<char>, ctx: &Ctx) -> Option<Reg> {
        match register {
            Some('+' | '*') => match &ctx.clipboard {
                Some(text) => Some(Reg { text: text.clone(), linewise: text.ends_with('\n') }),
                None => self.regs.get(&'+').cloned(),
            },
            Some(c) => self.regs.get(&c.to_ascii_lowercase()).cloned(),
            None => self.regs.get(&'"').cloned(),
        }
    }

    fn set_mark(&mut self, name: char, pos: usize, ctx: &mut Ctx) {
        if let Some(old) = self.marks.remove(&name) {
            ctx.buffer.remove_mark(old);
        }
        let id = ctx.buffer.add_mark(pos, Bias::Left);
        self.marks.insert(name, id);
    }

    fn mark_pos(&self, name: char, ctx: &Ctx) -> Option<usize> {
        let name = if name == '`' { '\'' } else { name };
        self.marks.get(&name).and_then(|id| ctx.buffer.mark(*id))
    }

    /// Before a jump: the jump list and the `''` mark keep where it started.
    fn record_jump(&mut self, ctx: &mut Ctx) {
        let pos = self.cursor;
        self.set_mark('\'', pos, ctx);
        self.jumps.truncate(self.jump_at);
        self.jumps.push(ctx.buffer.add_mark(pos, Bias::Left));
        self.jump_at = self.jumps.len();
    }

    fn jump_older(&mut self, n: usize, ctx: &mut Ctx) {
        if self.jump_at < n {
            return;
        }
        if self.jump_at == self.jumps.len() {
            self.jumps.push(ctx.buffer.add_mark(self.cursor, Bias::Left));
        }
        self.jump_at -= n;
        if let Some(p) = self.jumps.get(self.jump_at).and_then(|id| ctx.buffer.mark(*id)) {
            self.cursor = p;
        }
    }

    fn jump_newer(&mut self, n: usize, ctx: &mut Ctx) {
        if self.jump_at + n >= self.jumps.len() {
            return;
        }
        self.jump_at += n;
        if let Some(p) = self.jumps.get(self.jump_at).and_then(|id| ctx.buffer.mark(*id)) {
            self.cursor = p;
        }
    }

    // ----- Editing -----

    fn edit(&mut self, ctx: &mut Ctx, start: usize, end: usize, text: &str) {
        self.edit_kind(ctx, start, end, text, EditKind::Other);
    }

    fn edit_kind(&mut self, ctx: &mut Ctx, start: usize, end: usize, text: &str, kind: EditKind) {
        let before = Selection::point(self.cursor);
        let after = Selection::point(start + text.chars().count());
        let _ = ctx.buffer.edit(Transaction::replace(start, end, text), &before, &after, kind);
    }
}

/// The next (or previous) match of `re` from `from`, wrapping around the text's ends.
fn find(b: &Buffer, re: &Regex, from: usize, forward: bool, out: &mut Outcome) -> Option<usize> {
    let text = b.text();
    let from_byte = b.char_to_byte(from);
    let found = if forward {
        let next = b.char_to_byte(from + 1).max(from_byte);
        re.find_at(&text, next.min(text.len())).map(|m| m.start()).filter(|s| *s > from_byte || text.is_empty()).or_else(|| {
            let wrapped = re.find(&text).map(|m| m.start());
            if wrapped.is_some() {
                out.effects.push(Effect::Message("Search reached the end and went on from the start".to_owned()));
            }
            wrapped
        })
    } else {
        let all: Vec<usize> = re.find_iter(&text).map(|m| m.start()).collect();
        all.iter().rev().copied().find(|s| *s < from_byte).or_else(|| {
            let wrapped = all.last().copied();
            if wrapped.is_some() {
                out.effects.push(Effect::Message("Search reached the start and went on from the end".to_owned()));
            }
            wrapped
        })
    };
    match found {
        Some(byte) => Some(b.byte_to_char(byte)),
        None => {
            out.effects.push(Effect::Error(format!("Pattern not found: {}", re.as_str().trim_start_matches("(?m)").trim_start_matches("(?mi)"))));
            None
        }
    }
}

fn lines_text(b: &Buffer, first: usize, last: usize) -> String {
    let mut text = b.slice(b.line_to_char(first), b.line_to_char(last + 1).min(b.len_chars()));
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn region_lines(b: &Buffer, region: Region) -> (usize, usize) {
    match region {
        Region::Lines(f, l) => (f, l),
        Region::Chars(s, e) => (b.char_to_line(s), b.char_to_line(e.saturating_sub(1).max(s))),
    }
}

fn change_case(c: char, op: Op) -> String {
    match op {
        Op::Lower => c.to_lowercase().collect(),
        Op::Upper => c.to_uppercase().collect(),
        _ if c.is_uppercase() => c.to_lowercase().collect(),
        _ => c.to_uppercase().collect(),
    }
}

fn key_text(k: &Key) -> String {
    match k {
        Key::Char(c) => c.to_string(),
        Key::Ctrl(c) => format!("^{}", c.to_ascii_uppercase()),
        Key::Esc => "<Esc>".to_owned(),
        Key::Enter => "<CR>".to_owned(),
        Key::Backspace => "<BS>".to_owned(),
        Key::Delete => "<Del>".to_owned(),
        Key::Tab => "<Tab>".to_owned(),
        _ => String::new(),
    }
}
