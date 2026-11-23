//! Where a thread is in the text. Live, an anchor is a range of characters that follows the
//! text's edits ([`Anchor::follow`]). Saved, it is also a quote selector ([`Quote`]): the passage
//! and some text around it, so it can be found again in a text changed elsewhere
//! ([`Anchor::place`]).

use serde::{Deserialize, Serialize};

/// How many characters of context a quote keeps on each side.
pub const CONTEXT: usize = 32;

/// The passage as it was when the anchor was saved.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Quote {
    /// The passage itself.
    pub exact: String,
    /// Up to [`CONTEXT`] characters before it.
    #[serde(default)]
    pub prefix: String,
    /// Up to [`CONTEXT`] characters after it.
    #[serde(default)]
    pub suffix: String,
    /// The line it started on, from 1: a hint for finding it again.
    #[serde(default)]
    pub line: usize,
}

impl Quote {
    /// The quote of `start..end` (characters) in `text`.
    pub fn capture(text: &str, start: usize, end: usize) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let (start, end) = (start.min(chars.len()), end.min(chars.len()).max(start.min(chars.len())));
        let line = chars[..start].iter().filter(|c| **c == '\n').count() + 1;
        Self {
            exact: chars[start..end].iter().collect(),
            prefix: chars[start.saturating_sub(CONTEXT)..start].iter().collect(),
            suffix: chars[end..(end + CONTEXT).min(chars.len())].iter().collect(),
            line,
        }
    }
}

/// Whether an anchor is on its passage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// On its passage.
    #[default]
    Anchored,
    /// The passage was deleted: the anchor is empty, where it was, and keeps its quote.
    Removed,
    /// The passage was not found when the file was read; the range means nothing.
    Unanchored,
}

impl Placement {
    fn is_anchored(&self) -> bool {
        *self == Placement::Anchored
    }
}

/// A thread's place: a file and a range of characters in it, with its quote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// The file, as a project path.
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub quote: Quote,
    #[serde(default, skip_serializing_if = "Placement::is_anchored")]
    pub placement: Placement,
}

/// Where a position goes when text is inserted exactly at it.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Before,
    After,
}

/// Where `pos` is after `changes` (`(start, end, inserted characters)`, sorted, not overlapping,
/// in the text before them). A position inside replaced text goes to the new text's start
/// (`Before`) or end (`After`).
fn map(pos: usize, changes: &[(usize, usize, usize)], side: Side) -> usize {
    let mut delta: isize = 0;
    for &(start, end, len) in changes {
        if pos < start {
            break;
        }
        let past = if start < end { pos >= end } else { pos > start };
        if !past {
            let at = start.saturating_add_signed(delta);
            return if side == Side::Before { at } else { at + len };
        }
        delta += len as isize - (end - start) as isize;
    }
    pos.saturating_add_signed(delta)
}

impl Anchor {
    /// An anchor on `start..end` of `text`, the text of `path`.
    pub fn new(path: impl Into<String>, text: &str, start: usize, end: usize) -> Self {
        Self { path: path.into(), start, end, quote: Quote::capture(text, start, end), placement: Placement::Anchored }
    }

    /// Moves the range with an edit of its file. Text typed just before or just after the
    /// passage stays outside it; a passage deleted entirely leaves an empty anchor marked
    /// [`Placement::Removed`], never dropped.
    pub fn follow(&mut self, changes: &[(usize, usize, usize)]) {
        if self.placement == Placement::Unanchored {
            return;
        }
        if self.start == self.end {
            let at = map(self.start, changes, Side::Before);
            (self.start, self.end) = (at, at);
            return;
        }
        let start = map(self.start, changes, Side::After);
        let end = map(self.end, changes, Side::Before);
        if end <= start {
            (self.start, self.end) = (end, end);
            if self.placement == Placement::Anchored {
                self.placement = Placement::Removed;
            }
        } else {
            (self.start, self.end) = (start, end);
        }
    }

    /// Updates the quote from the text as it is now (when the file is saved), unless the passage
    /// is gone: then the quote is all that is left of it.
    pub fn requote(&mut self, text: &str) {
        if self.placement == Placement::Anchored {
            self.quote = Quote::capture(text, self.start, self.end);
        }
    }

    /// Puts a saved anchor on its passage in `text`, the file as read: at its saved range when the
    /// quote is there, else at the occurrence of the quote nearest its line, else nowhere
    /// ([`Placement::Unanchored`]). Re-anchoring by context and fuzzy matching come later
    /// (docs/collaboration-roadmap.md, R4).
    pub fn place(&mut self, text: &str) {
        let len = text.chars().count();
        let exact = &self.quote.exact;
        if self.placement == Placement::Removed || exact.is_empty() {
            let at = self.start.min(len);
            (self.start, self.end) = (at, self.end.clamp(at, len));
            return;
        }
        let here: String = text.chars().skip(self.start).take(self.end.saturating_sub(self.start)).collect();
        if self.end <= len && &here == exact {
            self.placement = Placement::Anchored;
            return;
        }
        // Every occurrence, as (line from 1, character offset).
        let mut found = Vec::new();
        let (mut line, mut chars, mut scanned) = (1, 0, 0);
        let mut from = 0;
        while let Some(i) = text.get(from..).and_then(|t| t.find(exact.as_str())) {
            let at = from + i;
            let between = &text[scanned..at];
            line += between.matches('\n').count();
            chars += between.chars().count();
            scanned = at;
            found.push((line, chars));
            from = at + exact.chars().next().map_or(1, char::len_utf8);
        }
        let hint = self.quote.line;
        match found.into_iter().min_by_key(|(l, c)| (l.abs_diff(hint), c.abs_diff(self.start))) {
            Some((_, at)) => {
                (self.start, self.end) = (at, at + exact.chars().count());
                self.placement = Placement::Anchored;
            }
            None => {
                (self.start, self.end) = (0, 0);
                self.placement = Placement::Unanchored;
            }
        }
    }
}
