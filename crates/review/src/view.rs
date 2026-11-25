//! The text with its suggestions shown in it (docs/collaboration-roadmap.md, R2), for Suggest
//! mode: each open suggestion's passage stays, to be struck through, and its replacement follows
//! it, to be underlined, though it is not in the file. [`View`] maps between the shown text and the
//! file's, and turns an edit of the shown text into changes of one's own suggestions
//! ([`View::interpret`]): typing creates or extends a suggestion, deleting marks text for deletion.

use crate::{Author, Id, Placement, Thread};

/// What a stretch of the shown text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The file's text, from this character of it.
    Text { source: usize },
    /// The file's text, from this character, that suggestion `id` deletes or replaces.
    Deleted { id: Id, source: usize },
    /// Suggestion `id`'s replacement, not in the file; it goes at character `at` of the file.
    Inserted { id: Id, at: usize },
}

/// A stretch of the shown text: characters `start..end` of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// A suggestion shown in the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub id: Id,
    pub author: Author,
    /// Its passage in the file.
    pub source: (usize, usize),
    /// Its passage and replacement in the shown text.
    pub view: (usize, usize),
    /// Where its replacement starts in the shown text.
    pub inserted_at: usize,
}

/// What an edit of the shown text does to the suggestions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// A new suggestion: replace `start..end` of the file with `text`.
    New { start: usize, end: usize, text: String },
    /// One's own suggestion `id` now replaces `start..end` with `text`.
    Revise { id: Id, start: usize, end: usize, text: String },
    /// One's own suggestion `id` no longer changes anything.
    Remove { id: Id },
}

/// Where the caret goes once the edit is made and the text shown again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caret {
    /// Before this character of the file.
    Source(usize),
    /// After this many characters of the replacement of the suggestion made or revised.
    Inserted(usize),
}

/// What [`View::interpret`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub ops: Vec<Op>,
    pub caret: Caret,
}

/// A file's text with its open suggestions shown in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// The shown text.
    pub text: String,
    /// Its stretches, in order, covering it.
    pub spans: Vec<Span>,
    /// The suggestions shown, in order.
    pub shown: Vec<Shown>,
    source_len: usize,
}

impl View {
    /// `source` (the file's text) with `threads`' open suggestions of it shown. Of suggestions
    /// that overlap, the first in the text is shown; the others only have their cards.
    pub fn new<'a>(source: &str, threads: impl IntoIterator<Item = &'a Thread>) -> Self {
        let chars: Vec<char> = source.chars().collect();
        let len = chars.len();
        let mut list: Vec<&Thread> = threads
            .into_iter()
            .filter(|t| t.is_open() && t.suggestion.is_some() && t.anchor.placement != Placement::Unanchored && t.anchor.end <= len)
            .collect();
        list.sort_by_key(|t| (t.anchor.start, t.anchor.end, t.id));
        let mut view = View { source_len: len, ..Self::default() };
        let mut pos = 0;
        let push = |view: &mut View, text: &str, n: usize, kind: Kind| {
            if n > 0 {
                let at = view.len();
                view.text.push_str(text);
                view.spans.push(Span { start: at, end: at + n, kind });
            }
        };
        for t in list {
            let Some(s) = &t.suggestion else { continue };
            let (start, end) = (t.anchor.start, t.anchor.end);
            if start < pos || (start == end && s.replacement.is_empty()) {
                continue;
            }
            let text: String = chars[pos..start].iter().collect();
            push(&mut view, &text, start - pos, Kind::Text { source: pos });
            let first = view.len();
            let deleted: String = chars[start..end].iter().collect();
            push(&mut view, &deleted, end - start, Kind::Deleted { id: t.id, source: start });
            let inserted_at = view.len();
            push(&mut view, &s.replacement, s.replacement.chars().count(), Kind::Inserted { id: t.id, at: end });
            view.shown.push(Shown { id: t.id, author: s.author.clone(), source: (start, end), view: (first, view.len()), inserted_at });
            pos = end;
        }
        let text: String = chars[pos..].iter().collect();
        push(&mut view, &text, len - pos, Kind::Text { source: pos });
        view
    }

    /// Whether the shown text is the file's: no suggestion is shown.
    pub fn is_plain(&self) -> bool {
        self.shown.is_empty()
    }

    /// The length of the shown text, in characters.
    pub fn len(&self) -> usize {
        self.spans.last().map_or(0, |s| s.end)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The stretch character `v` of the shown text is in.
    fn span_at(&self, v: usize) -> Option<&Span> {
        let i = self.spans.partition_point(|s| s.end <= v);
        self.spans.get(i)
    }

    /// The place in the file of position `v` of the shown text. A replacement's characters are
    /// all where it goes.
    pub fn to_source(&self, v: usize) -> usize {
        match self.span_at(v) {
            Some(Span { start, kind: Kind::Text { source } | Kind::Deleted { source, .. }, .. }) => source + (v - start),
            Some(Span { kind: Kind::Inserted { at, .. }, .. }) => *at,
            None => self.source_len,
        }
    }

    /// The position in the shown text of character `s` of the file: after a replacement that
    /// goes just before it.
    pub fn to_view(&self, s: usize) -> usize {
        for span in &self.spans {
            match span.kind {
                Kind::Text { source } | Kind::Deleted { source, .. } if source <= s && s < source + (span.end - span.start) => {
                    return span.start + (s - source);
                }
                _ => {}
            }
        }
        self.len()
    }

    /// The position in the shown text just after character `s - 1` of the file: before a
    /// replacement that goes at `s`. For the ends of passages.
    pub fn to_view_end(&self, s: usize) -> usize {
        if s == 0 { 0 } else { self.to_view(s - 1) + 1 }
    }

    /// The suggestion shown at position `v` (in its passage or its replacement, or at its ends).
    pub fn suggestion_at(&self, v: usize) -> Option<&Shown> {
        self.shown.iter().find(|s| s.view.0 <= v && v <= s.view.1)
    }

    /// What replacing `start..end` of the shown text with `text` does, typed by `me` in Suggest
    /// mode: the file does not change, one's suggestions do. Text typed next to or inside one's
    /// own suggestion extends it; deleting text already deleted only moves the caret. An edit
    /// inside another person's suggestion is refused: returns that person.
    pub fn interpret(&self, source: &str, start: usize, end: usize, text: &str, me: &Author) -> Result<Plan, Author> {
        let len = self.len();
        let (vs, ve) = (start.min(len), end.min(len).max(start.min(len)));
        if text.is_empty() && vs < ve {
            let only_deleted = self.spans.iter().filter(|s| s.start < ve && s.end > vs).all(|s| matches!(s.kind, Kind::Deleted { .. }));
            if only_deleted {
                return Ok(Plan { ops: Vec::new(), caret: Caret::Source(self.to_source(vs)) });
            }
        }
        for s in self.shown.iter().filter(|s| !s.author.is(me)) {
            let (a, b) = s.view;
            let inside = if vs == ve { a < vs && vs < b } else { a < ve && b > vs };
            if inside {
                return Err(s.author.clone());
            }
        }
        let touched: Vec<&Shown> = self.shown.iter().filter(|s| s.author.is(me) && s.view.0 <= ve && s.view.1 >= vs).collect();
        let w0 = touched.iter().map(|s| s.view.0).fold(vs, usize::min);
        let w1 = touched.iter().map(|s| s.view.1).fold(ve, usize::max);
        let (s0, s1) = (self.to_source(w0), self.to_source(w1));
        // The window as it would read with the edit made and its suggestions applied.
        let shown: Vec<char> = self.text.chars().collect();
        let mut proposed = String::new();
        let mut k = 0;
        for i in w0..=w1 {
            if i == vs {
                proposed.push_str(text);
                k = proposed.chars().count();
            }
            if i == w1 {
                break;
            }
            if (vs..ve).contains(&i) {
                continue;
            }
            match self.span_at(i).map(|s| s.kind) {
                Some(Kind::Deleted { .. }) | None => {}
                Some(_) => {
                    if let Some(c) = shown.get(i) {
                        proposed.push(*c);
                    }
                }
            }
        }
        let passage: String = source.chars().skip(s0).take(s1.saturating_sub(s0)).collect();
        let unchanged = passage == proposed;
        let mut ops = Vec::new();
        let mut kept = false;
        match touched.split_first() {
            None if !unchanged => {
                ops.push(Op::New { start: s0, end: s1, text: proposed });
                kept = true;
            }
            None => {}
            Some((first, rest)) => {
                if unchanged {
                    ops.push(Op::Remove { id: first.id });
                } else {
                    ops.push(Op::Revise { id: first.id, start: s0, end: s1, text: proposed });
                    kept = true;
                }
                ops.extend(rest.iter().map(|s| Op::Remove { id: s.id }));
            }
        }
        let deleted_source = text.is_empty() && matches!(self.span_at(vs).map(|s| s.kind), Some(Kind::Text { .. } | Kind::Deleted { .. }));
        let caret = if deleted_source {
            Caret::Source(self.to_source(vs))
        } else if kept {
            Caret::Inserted(k)
        } else {
            Caret::Source(s0 + k)
        };
        Ok(Plan { ops, caret })
    }
}
