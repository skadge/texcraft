//! Changes to the text.

use ropey::Rope;

use crate::{Error, Result};

/// Which side of text inserted exactly at a position the position stays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bias {
    /// Before it: the position stays put.
    #[default]
    Left,
    /// After it: the position moves past it.
    Right,
}

/// `start..end` (chars, in the text before the transaction) replaced by `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Sorted, non-overlapping changes, all made at once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transaction {
    changes: Vec<Change>,
}

impl Transaction {
    /// Changes in any order; overlapping ones are refused.
    pub fn new(mut changes: Vec<Change>) -> Result<Self> {
        changes.sort_by_key(|c| (c.start, c.end));
        for pair in changes.windows(2) {
            if let [a, b] = pair
                && b.start < a.end
            {
                return Err(Error::Unordered { at: b.start });
            }
        }
        if let Some(c) = changes.iter().find(|c| c.end < c.start) {
            return Err(Error::Unordered { at: c.start });
        }
        Ok(Self { changes })
    }

    pub fn insert(at: usize, text: &str) -> Self {
        Self::replace(at, at, text)
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Self::replace(start, end, "")
    }

    pub fn replace(start: usize, end: usize, text: &str) -> Self {
        let (start, end) = (start.min(end), start.max(end));
        Self { changes: vec![Change { start, end, text: text.to_owned() }] }
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// No change makes a difference.
    pub fn is_empty(&self) -> bool {
        self.changes.iter().all(|c| c.start == c.end && c.text.is_empty())
    }

    pub(crate) fn first_start(&self) -> Option<usize> {
        self.changes.first().map(|c| c.start)
    }

    pub(crate) fn first_end(&self) -> Option<usize> {
        self.changes.first().map(|c| c.end)
    }

    pub(crate) fn inserted_text(&self) -> String {
        self.changes.iter().map(|c| c.text.as_str()).collect()
    }

    pub(crate) fn check(&self, len: usize) -> Result<()> {
        match self.changes.last() {
            Some(c) if c.end > len => Err(Error::OutOfBounds { end: c.end, len }),
            _ => Ok(()),
        }
    }

    /// The transaction that undoes this one, made on `before` (the text this one applies to).
    pub(crate) fn invert(&self, before: &Rope) -> Transaction {
        let mut delta: isize = 0;
        let changes = self
            .changes
            .iter()
            .map(|c| {
                let start = c.start.saturating_add_signed(delta);
                let len = c.text.chars().count();
                delta += len as isize - (c.end - c.start) as isize;
                Change { start, end: start + len, text: before.slice(c.start..c.end).to_string() }
            })
            .collect();
        Transaction { changes }
    }

    /// The smallest single change that turns `old` into `new`: what is between their common
    /// beginning and their common end. Empty when they are equal.
    pub fn diff(old: &str, new: &str) -> Self {
        let prefix: usize = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
        let (old_len, new_len) = (old.chars().count(), new.chars().count());
        let room = old_len.min(new_len) - prefix;
        let suffix = old.chars().rev().zip(new.chars().rev()).take(room).take_while(|(a, b)| a == b).count();
        if prefix == old_len && prefix == new_len {
            return Self::default();
        }
        let text: String = new.chars().skip(prefix).take(new_len - prefix - suffix).collect();
        Self::replace(prefix, old_len - suffix, &text)
    }

    /// This transaction made after `over` instead of before it: both apply to the same text, and
    /// the result applies to the text `over` made. Text `over` inserted inside a range this one
    /// replaces is kept out of it, so undoing a local edit after a peer's leaves the peer's text.
    pub fn rebase(&self, over: &Transaction) -> Transaction {
        // `over`'s inserted text, as ranges of the text after it.
        let mut delta: isize = 0;
        let inserted: Vec<(usize, usize)> = over
            .changes
            .iter()
            .filter_map(|c| {
                let start = c.start.saturating_add_signed(delta);
                let len = c.text.chars().count();
                delta += len as isize - (c.end - c.start) as isize;
                (len > 0).then_some((start, start + len))
            })
            .collect();
        let mut changes = Vec::with_capacity(self.changes.len());
        for c in &self.changes {
            if c.start == c.end {
                let at = over.map(c.start, Bias::Left);
                changes.push(Change { start: at, end: at, text: c.text.clone() });
                continue;
            }
            let start = over.map(c.start, Bias::Right);
            let end = over.map(c.end, Bias::Left).max(start);
            let mut pieces = Vec::new();
            let mut from = start;
            for &(a, b) in inserted.iter().filter(|(a, b)| *a >= start && *b <= end) {
                if a > from {
                    pieces.push((from, a));
                }
                from = from.max(b);
            }
            if end > from || pieces.is_empty() {
                pieces.push((from, end.max(from)));
            }
            for (i, (s, e)) in pieces.into_iter().enumerate() {
                let text = if i == 0 { c.text.clone() } else { String::new() };
                changes.push(Change { start: s, end: e, text });
            }
        }
        Transaction { changes }
    }

    /// Where `pos` (in the text before) is after the transaction. A position inside replaced
    /// text, or where text is inserted, goes to the new text's start (`Left`) or end (`Right`);
    /// the end of replaced text is after the new text.
    pub fn map(&self, pos: usize, bias: Bias) -> usize {
        let mut delta: isize = 0;
        for c in &self.changes {
            let len = c.text.chars().count();
            let past = if c.start < c.end { pos >= c.end } else { pos > c.start };
            if pos < c.start {
                break;
            }
            if !past {
                let start = c.start.saturating_add_signed(delta);
                return match bias {
                    Bias::Left => start,
                    Bias::Right => start + len,
                };
            }
            delta += len as isize - (c.end - c.start) as isize;
        }
        pos.saturating_add_signed(delta)
    }
}
