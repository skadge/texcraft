//! Carets and selections.

use crate::{Bias, Transaction};

/// A selection from `anchor` (where it started) to `head` (the caret); empty when they are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Range {
    pub anchor: usize,
    pub head: usize,
}

impl Range {
    pub fn point(pos: usize) -> Self {
        Self { anchor: pos, head: pos }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The range after `tx`: an empty one stays a caret after text typed at it.
    pub fn map(&self, tx: &Transaction) -> Self {
        if self.is_empty() {
            return Self::point(tx.map(self.head, Bias::Right));
        }
        let (start_bias, end_bias) = (Bias::Left, Bias::Right);
        let (start, end) = (tx.map(self.start(), start_bias), tx.map(self.end(), end_bias));
        if self.anchor <= self.head { Self::new(start, end) } else { Self::new(end, start) }
    }
}

/// One or more ranges, one of them primary (the one the view follows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    ranges: Vec<Range>,
    primary: usize,
}

impl Default for Selection {
    fn default() -> Self {
        Self::point(0)
    }
}

impl Selection {
    pub fn point(pos: usize) -> Self {
        Self { ranges: vec![Range::point(pos)], primary: 0 }
    }

    pub fn single(anchor: usize, head: usize) -> Self {
        Self { ranges: vec![Range::new(anchor, head)], primary: 0 }
    }

    /// Several ranges; the last is primary. An empty list is a caret at 0.
    pub fn many(ranges: Vec<Range>) -> Self {
        if ranges.is_empty() {
            return Self::point(0);
        }
        let primary = ranges.len() - 1;
        Self { ranges, primary }
    }

    /// Several ranges with `primary` (an index into them, clamped) the one the view follows.
    pub fn with_primary(ranges: Vec<Range>, primary: usize) -> Self {
        let mut s = Self::many(ranges);
        s.primary = primary.min(s.ranges.len() - 1);
        s
    }

    /// The ranges in order of position, those that overlap (or are the same caret) made one; the
    /// primary is the one that holds the old primary's caret.
    pub fn merged(&self) -> Self {
        let head = self.primary().head;
        let mut ranges = self.ranges.clone();
        ranges.sort_by_key(|r| (r.start(), r.end()));
        let mut out: Vec<Range> = Vec::with_capacity(ranges.len());
        for r in ranges {
            match out.last_mut() {
                Some(last) if r.start() < last.end() || (r.start() == last.end() && (r.is_empty() || last.is_empty())) => {
                    let (start, end) = (last.start(), last.end().max(r.end()));
                    // The merged range keeps the direction of the first.
                    *last = if last.anchor <= last.head { Range::new(start, end) } else { Range::new(end, start) };
                }
                _ => out.push(r),
            }
        }
        let primary = out.iter().position(|r| r.start() <= head && head <= r.end()).unwrap_or(out.len().saturating_sub(1));
        Self::with_primary(out, primary)
    }

    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }

    /// Which of [`Self::ranges`] is the primary.
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn primary(&self) -> Range {
        self.ranges.get(self.primary).copied().unwrap_or_default()
    }

    pub fn map(&self, tx: &Transaction) -> Self {
        Self { ranges: self.ranges.iter().map(|r| r.map(tx)).collect(), primary: self.primary }
    }

    /// Every position at most `len`.
    pub fn clamp(mut self, len: usize) -> Self {
        for r in &mut self.ranges {
            r.anchor = r.anchor.min(len);
            r.head = r.head.min(len);
        }
        self
    }
}
