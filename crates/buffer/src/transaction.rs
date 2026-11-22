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
