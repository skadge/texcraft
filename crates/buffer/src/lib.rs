//! The editor's text (docs/ROADMAP.md).
//!
//! A [`Buffer`] is a rope ([`ropey`]) changed only by [`Transaction`]s: sorted, non-overlapping
//! replacements of the text as it was before the transaction. Each one is kept with its inverse
//! and the selections before and after it, so undo and redo restore the caret too.
//!
//! - **Undo groups.** Typing joins the group before it while it continues where the last typing
//!   stopped, within [`PAUSE`] seconds, and without starting a new word; a deletion by Backspace or
//!   Delete joins earlier ones the same way. [`Buffer::begin_group`] and [`Buffer::end_group`] make
//!   everything between them one group (Vim's insert session is one change).
//! - **Origins.** Every change the buffer makes is kept with its [`Origin`] until its owner takes
//!   it ([`Buffer::take_edits`]), so the session, review anchors and later the shared document
//!   learn what changed, not only the new text. Edits made elsewhere (a peer, a file changed on
//!   disk) come in through [`Buffer::receive`]: they are not in the undo history, which is rebased
//!   over them, so undo takes back only one's own edits and leaves theirs.
//! - **Marks** are positions that follow edits: diagnostics, bookmarks, Vim's marks, the build's
//!   line map. Each has a [`Bias`] for text inserted exactly at it.
//! - **Positions** are char indices (Unicode scalar values). [`Buffer::to_utf16`] and
//!   [`Buffer::from_utf16`] convert to the line and UTF-16 column of AccessKit and LSP;
//!   [`Buffer::char_to_byte`] and [`Buffer::byte_to_char`] to byte offsets.
//!
//! Lines end at LF only, as for TeX; a CR before it stays in the line.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod selection;
mod transaction;

use std::collections::BTreeMap;

pub use ropey::{Rope, RopeSlice};
pub use selection::{Range, Selection};
pub use transaction::{Bias, Change, Transaction};

/// Typing more than this many seconds after the last starts a new undo group.
pub const PAUSE: f64 = 1.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("a change ends at {end}, after the end of the text ({len})")]
    OutOfBounds { end: usize, len: usize },
    #[error("changes overlap or are out of order at {at}")]
    Unordered { at: usize },
    #[error("line {line} column {column} is not in the text")]
    NoPosition { line: usize, column: usize },
}

pub type Result<T> = std::result::Result<T, Error>;

/// How an edit is grouped for undo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditKind {
    /// Characters typed, at this time (seconds, the caller's clock).
    Typing { at: f64 },
    /// Text deleted by Backspace or Delete, at this time.
    Deleting { at: f64 },
    /// Anything else: a paste, a command, an agent's edit. Always its own group.
    Other,
}

/// Where an edit came from (docs/collaboration-roadmap.md, F1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Typed, pasted, or made by a command on this machine (an agent's edit included).
    Local,
    Undo,
    Redo,
    /// A peer's edit, in a shared session.
    Remote,
    /// Accepting a suggestion.
    Review,
    /// The file changed outside TeXCraft (another editor, a git pull).
    External,
}

impl Origin {
    pub const ALL: [Origin; 6] = [Origin::Local, Origin::Undo, Origin::Redo, Origin::Remote, Origin::Review, Origin::External];

    /// The name automation uses (`file.edit`'s `origin`).
    pub fn name(self) -> &'static str {
        match self {
            Origin::Local => "local",
            Origin::Undo => "undo",
            Origin::Redo => "redo",
            Origin::Remote => "remote",
            Origin::Review => "review",
            Origin::External => "external",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.name() == name)
    }

    /// Made by someone else: such edits stay out of the undo history.
    pub fn is_foreign(self) -> bool {
        matches!(self, Origin::Remote | Origin::External)
    }
}

/// A mark's handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarkId(u64);

#[derive(Debug, Clone)]
struct Step {
    forward: Transaction,
    inverse: Transaction,
    before: Selection,
    after: Selection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GroupKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Debug, Clone)]
struct Group {
    steps: Vec<Step>,
    kind: GroupKind,
    /// When the group's last edit was made, for typing and deleting.
    last: f64,
    /// Where the group's last edit left the caret, for typing and deleting.
    end: usize,
    /// The last character typed was whitespace.
    after_space: bool,
}

/// Text with its undo history and marks.
#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Edits join the group on top of `undo` while this is above zero.
    open: usize,
    /// The next edit starts a new group, even typing that would join.
    sealed: bool,
    marks: BTreeMap<MarkId, (usize, Bias)>,
    next_mark: u64,
    version: u64,
    /// The changes made since the owner last took them.
    log: Vec<(Transaction, Origin)>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new("")
    }
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            undo: Vec::new(),
            redo: Vec::new(),
            open: 0,
            sealed: false,
            marks: BTreeMap::new(),
            next_mark: 0,
            version: 0,
            log: Vec::new(),
        }
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Goes up by one with every change, undo and redo included.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// The number of lines: one more than the number of LFs.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Line `line` without its LF; empty past the last line.
    pub fn line(&self, line: usize) -> String {
        if line >= self.len_lines() {
            return String::new();
        }
        let mut s = self.rope.line(line).to_string();
        if s.ends_with('\n') {
            s.pop();
        }
        s
    }

    /// The line `pos` is on (the last line past the end).
    pub fn char_to_line(&self, pos: usize) -> usize {
        self.rope.char_to_line(pos.min(self.len_chars()))
    }

    /// Where line `line` starts (the end of the text past the last line).
    pub fn line_to_char(&self, line: usize) -> usize {
        if line >= self.len_lines() { self.len_chars() } else { self.rope.line_to_char(line) }
    }

    /// Where line `line` ends, before its LF.
    pub fn line_end(&self, line: usize) -> usize {
        self.line_to_char(line) + self.line(line).chars().count()
    }

    /// The character at `pos`, if any.
    pub fn char_at(&self, pos: usize) -> Option<char> {
        (pos < self.len_chars()).then(|| self.rope.char(pos))
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let (start, end) = (start.min(self.len_chars()), end.min(self.len_chars()));
        if start >= end { String::new() } else { self.rope.slice(start..end).to_string() }
    }

    pub fn char_to_byte(&self, pos: usize) -> usize {
        self.rope.char_to_byte(pos.min(self.len_chars()))
    }

    /// The char at or before byte `byte`.
    pub fn byte_to_char(&self, byte: usize) -> usize {
        self.rope.byte_to_char(byte.min(self.rope.len_bytes()))
    }

    /// `pos` as a line and a UTF-16 column (AccessKit's and LSP's positions).
    pub fn to_utf16(&self, pos: usize) -> (usize, usize) {
        let pos = pos.min(self.len_chars());
        let line = self.rope.char_to_line(pos);
        let start = self.rope.line_to_char(line);
        let column = self.rope.char_to_utf16_cu(pos) - self.rope.char_to_utf16_cu(start);
        (line, column)
    }

    /// The position of a line and a UTF-16 column; a column inside a surrogate pair is refused.
    pub fn from_utf16(&self, line: usize, column: usize) -> Result<usize> {
        let missing = Error::NoPosition { line, column };
        if line >= self.len_lines() {
            return Err(missing);
        }
        let start = self.rope.line_to_char(line);
        let target = self.rope.char_to_utf16_cu(start) + column;
        if target > self.rope.char_to_utf16_cu(self.line_end(line)) {
            return Err(missing);
        }
        let pos = self.rope.utf16_cu_to_char(target);
        if self.rope.char_to_utf16_cu(pos) != target {
            return Err(missing);
        }
        Ok(pos)
    }

    /// Applies `tx`, recording it for undo. `before` and `after` are the selections around it,
    /// restored by undo and redo.
    pub fn edit(&mut self, tx: Transaction, before: &Selection, after: &Selection, kind: EditKind) -> Result<()> {
        tx.check(self.len_chars())?;
        if tx.is_empty() {
            return Ok(());
        }
        let inverse = tx.invert(&self.rope);
        let step = Step { forward: tx, inverse, before: before.clone(), after: after.clone() };
        let (group_kind, at) = match kind {
            EditKind::Typing { at } => (GroupKind::Typing, at),
            EditKind::Deleting { at } => (GroupKind::Deleting, at),
            EditKind::Other => (GroupKind::Other, 0.0),
        };
        let typed = step.forward.inserted_text();
        let starts_word = typed.chars().next().is_some_and(|c| !c.is_whitespace());
        let (first_start, first_end) = (step.forward.first_start(), step.forward.first_end());
        let continues = |g: &Group| match group_kind {
            // Typing goes on where it stopped.
            GroupKind::Typing => first_start == Some(g.end) && !(g.after_space && starts_word),
            // Backspace deletes up to where the caret was; Delete from there.
            GroupKind::Deleting => first_end == Some(g.end) || first_start == Some(g.end),
            GroupKind::Other => false,
        };
        let joins = self.open > 0 || (!self.sealed && self.undo.last().is_some_and(|g| g.kind == group_kind && at - g.last < PAUSE && continues(g)));
        self.apply(&step.forward, Origin::Local);
        let end = after.primary().head;
        let after_space = typed.chars().last().is_some_and(char::is_whitespace);
        match self.undo.last_mut() {
            Some(g) if joins => {
                g.steps.push(step);
                g.last = at;
                g.end = end;
                g.after_space = after_space;
            }
            _ => self.undo.push(Group { steps: vec![step], kind: group_kind, last: at, end, after_space }),
        }
        self.sealed = false;
        self.redo.clear();
        Ok(())
    }

    /// The next edit starts a new undo group (the caret moved, the window lost focus).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Everything edited until the matching [`Buffer::end_group`] is undone as one.
    pub fn begin_group(&mut self) {
        if self.open == 0 {
            // An empty group, which the first edit fills (and an unfilled one is dropped).
            self.undo.push(Group { steps: Vec::new(), kind: GroupKind::Other, last: 0.0, end: 0, after_space: false });
        }
        self.open += 1;
    }

    pub fn end_group(&mut self) {
        self.open = self.open.saturating_sub(1);
        if self.open == 0 {
            if self.undo.last().is_some_and(|g| g.steps.is_empty()) {
                self.undo.pop();
            }
            self.sealed = true;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo.iter().any(|g| !g.steps.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last group; returns the selection before it.
    pub fn undo(&mut self) -> Option<Selection> {
        while self.undo.last().is_some_and(|g| g.steps.is_empty()) && self.open == 0 {
            self.undo.pop();
        }
        let group = self.undo.pop()?;
        for step in group.steps.iter().rev() {
            self.apply(&step.inverse, Origin::Undo);
        }
        let before = group.steps.first().map(|s| s.before.clone());
        self.redo.push(group);
        self.sealed = true;
        before
    }

    /// Redoes the last undone group; returns the selection after it.
    pub fn redo(&mut self) -> Option<Selection> {
        let group = self.redo.pop()?;
        for step in &group.steps {
            self.apply(&step.forward, Origin::Redo);
        }
        let after = group.steps.last().map(|s| s.after.clone());
        self.undo.push(group);
        self.sealed = true;
        after
    }

    /// Replaces the whole text as one undoable edit (a file changed on disk, an agent's write).
    pub fn replace_all(&mut self, text: &str, before: &Selection) -> Result<()> {
        if self.rope == text {
            return Ok(());
        }
        let tx = Transaction::replace(0, self.len_chars(), text);
        let end = text.chars().count();
        let after = before.clone().clamp(end);
        self.edit(tx, before, &after, EditKind::Other)
    }

    /// Applies an edit made elsewhere (`origin` is usually [`Origin::Remote`] or
    /// [`Origin::External`]) without recording it for undo. The undo and redo histories are rebased
    /// over it, so undoing afterwards takes back one's own changes where they now are and keeps
    /// the other's text. The caller maps its selection with [`Selection::map`].
    pub fn receive(&mut self, tx: Transaction, origin: Origin) -> Result<()> {
        tx.check(self.len_chars())?;
        if tx.is_empty() {
            return Ok(());
        }
        // The undo history is a chain back from the current text: the top step's inverse applies
        // to it, the next one's to what that gives, and so on. `over` is the received edit as
        // it applies at each link.
        let mut over = tx.clone();
        for group in self.undo.iter_mut().rev() {
            for step in group.steps.iter_mut().rev() {
                over = rebase_step(&mut step.inverse, &mut step.forward, &mut step.after, &mut step.before, &over);
            }
        }
        // The redo history is a chain forward from the current text.
        let mut over = tx.clone();
        for group in self.redo.iter_mut().rev() {
            for step in group.steps.iter_mut() {
                over = rebase_step(&mut step.forward, &mut step.inverse, &mut step.before, &mut step.after, &over);
            }
        }
        if let Some(g) = self.undo.last_mut() {
            g.end = tx.map(g.end, Bias::Right);
        }
        self.apply(&tx, origin);
        Ok(())
    }

    /// The changes made since the last call, oldest first, each with its origin.
    pub fn take_edits(&mut self) -> Vec<(Transaction, Origin)> {
        std::mem::take(&mut self.log)
    }

    /// A mark at `pos` (clamped to the text).
    pub fn add_mark(&mut self, pos: usize, bias: Bias) -> MarkId {
        let id = MarkId(self.next_mark);
        self.next_mark += 1;
        self.marks.insert(id, (pos.min(self.len_chars()), bias));
        id
    }

    pub fn mark(&self, id: MarkId) -> Option<usize> {
        self.marks.get(&id).map(|(p, _)| *p)
    }

    pub fn remove_mark(&mut self, id: MarkId) {
        self.marks.remove(&id);
    }

    fn apply(&mut self, tx: &Transaction, origin: Origin) {
        for change in tx.changes().iter().rev() {
            if change.end > change.start {
                self.rope.remove(change.start..change.end);
            }
            if !change.text.is_empty() {
                self.rope.insert(change.start, &change.text);
            }
        }
        for (pos, bias) in self.marks.values_mut() {
            *pos = tx.map(*pos, *bias);
        }
        self.version += 1;
        self.log.push((tx.clone(), origin));
    }
}

/// Rebases one step of a history chain over `over`, an edit of the text `near` applies to
/// (`far` undoes `near`). Returns `over` as it applies to the text `near` makes.
fn rebase_step(near: &mut Transaction, far: &mut Transaction, near_sel: &mut Selection, far_sel: &mut Selection, over: &Transaction) -> Transaction {
    let next = over.rebase(near);
    *near = near.rebase(over);
    *far = far.rebase(&next);
    *near_sel = near_sel.map(over);
    *far_sel = far_sel.map(&next);
    next
}
