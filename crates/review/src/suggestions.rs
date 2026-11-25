//! Suggested changes (docs/collaboration-roadmap.md, R2): a thread whose [`Suggestion`] proposes
//! to replace its passage. Accepting one gives the engine the edit to make; once made, the
//! suggestion is marked accepted on the text it put there, so undoing that edit reopens it.

use chrono::{DateTime, Utc};

use crate::{Anchor, Author, Error, Filter, Id, Placement, Result, Review, State, Suggestion, Thread};

/// Whether `changes` (`(start, end, inserted characters)` in the text before them) change the
/// text strictly inside `anchor`'s passage: text typed at its edges does not count, text typed
/// inside it, or deleted from it, does. For an empty passage (an insertion point), a change that
/// spans the point counts.
pub(crate) fn changed_inside(anchor: &Anchor, changes: &[(usize, usize, usize)]) -> bool {
    if anchor.placement == Placement::Unanchored {
        return false;
    }
    let (s, e) = (anchor.start, anchor.end);
    changes.iter().any(|&(cs, ce, _)| {
        if s == e {
            cs < s && s < ce
        } else if cs == ce {
            s < cs && cs < e
        } else {
            cs < e && ce > s
        }
    })
}

/// The characters `start..end` of `text`.
fn chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end.saturating_sub(start)).collect()
}

/// A suggestion to apply: replace `start..end` (characters, in the text as it is) of its file
/// with `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub id: Id,
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

impl Review {
    /// Starts a suggestion: replace `anchor`'s passage of `text` (its file's text) with
    /// `replacement`.
    pub fn suggest(&mut self, id: Id, anchor: Anchor, text: &str, replacement: &str, author: Author, time: DateTime<Utc>) -> Result<&Thread> {
        let len = text.chars().count();
        if anchor.start > anchor.end || anchor.end > len {
            return Err(Error::OutOfText { start: anchor.start, end: anchor.end, len });
        }
        if chars(text, anchor.start, anchor.end) == replacement {
            return Err(Error::NoChange);
        }
        let suggestion = Suggestion { author, time, replacement: replacement.to_owned(), edited: None, outdated: false };
        self.insert(Thread { id, anchor, state: State::Open, suggestion: Some(suggestion), messages: Vec::new() });
        self.get(id).ok_or(Error::NoThread(id))
    }

    /// Changes one's own open suggestion: its passage (`anchor`, made on the text as it is) and
    /// replacement. Consecutive typing in Suggest mode extends a suggestion this way.
    pub fn revise(&mut self, id: Id, by: &Author, anchor: Anchor, replacement: &str, at: DateTime<Utc>) -> Result<()> {
        if anchor.quote.exact == replacement {
            return Err(Error::NoChange);
        }
        let t = self.get_mut(id)?;
        let Some(s) = t.suggestion.as_mut() else { return Err(Error::NotSuggestion(id)) };
        if !s.author.is(by) {
            return Err(Error::NotAuthor);
        }
        if t.state != State::Open {
            return Err(Error::NotOpen);
        }
        s.replacement = replacement.to_owned();
        s.edited = Some(at);
        s.outdated = false;
        t.anchor = Anchor { path: t.anchor.path.clone(), ..anchor };
        Ok(())
    }

    /// The edit that accepts suggestion `id`, if it can be applied now.
    pub fn to_accept(&self, id: Id) -> Result<Accepted> {
        let t = self.get(id).ok_or(Error::NoThread(id))?;
        let s = t.suggestion.as_ref().ok_or(Error::NotSuggestion(id))?;
        if !t.is_open() {
            return Err(Error::NotOpen);
        }
        if s.outdated || t.anchor.placement == Placement::Unanchored {
            return Err(Error::Outdated);
        }
        Ok(Accepted { id, path: t.anchor.path.clone(), start: t.anchor.start, end: t.anchor.end, replacement: s.replacement.clone() })
    }

    /// The edits that accept every open suggestion `filter` keeps, by file and in the order of
    /// the text. Of suggestions that overlap, the first is taken; the others become outdated when
    /// it is made ([`Review::follow`]).
    pub fn to_accept_all(&self, filter: &Filter) -> Vec<Accepted> {
        let mut all: Vec<Accepted> = self.list(filter).filter(|t| t.can_accept()).filter_map(|t| self.to_accept(t.id).ok()).collect();
        all.sort_by(|a, b| (&a.path, a.start, a.end, a.id).cmp(&(&b.path, b.start, b.end, b.id)));
        let mut out: Vec<Accepted> = Vec::new();
        for a in all {
            let overlaps = out.last().is_some_and(|last| last.path == a.path && a.start < last.end);
            if !overlaps {
                out.push(a);
            }
        }
        out
    }

    /// `done`, edits of one file made together, were made: each suggestion is accepted, its
    /// passage now the text it put there.
    pub fn accepted(&mut self, done: &[Accepted], by: &Author, at: DateTime<Utc>) {
        let mut delta: isize = 0;
        for a in done {
            let start = a.start.saturating_add_signed(delta);
            let len = a.replacement.chars().count();
            delta += len as isize - (a.end - a.start) as isize;
            if let Ok(t) = self.get_mut(a.id) {
                t.anchor.start = start;
                t.anchor.end = start + len;
                t.anchor.placement = Placement::Anchored;
                t.state = State::Accepted { by: by.clone(), at };
                if let Some(s) = t.suggestion.as_mut() {
                    s.outdated = false;
                }
            }
        }
    }

    /// Rejects suggestion `id`: the text stays as it is.
    pub fn reject(&mut self, id: Id, by: &Author, at: DateTime<Utc>) -> Result<()> {
        let t = self.get_mut(id)?;
        if !t.is_suggestion() {
            return Err(Error::NotSuggestion(id));
        }
        if !t.is_open() {
            return Err(Error::NotOpen);
        }
        t.state = State::Rejected { by: by.clone(), at };
        Ok(())
    }

    /// Follows an undo or a redo of `path` (`(start, end, text)`, sorted, in the text before it)
    /// like [`Review::follow`], and recognises accepts in it: a change that puts back the passage
    /// an accepted suggestion replaced reopens it, and one that makes an open suggestion's change
    /// accepts it again (`by`, or its author without one). Returns the suggestions that changed.
    pub fn follow_undo(&mut self, path: &str, changes: &[(usize, usize, String)], by: Option<&Author>, at: DateTime<Utc>) -> Vec<Id> {
        let mut reopen = Vec::new();
        let mut reaccept = Vec::new();
        let mut delta: isize = 0;
        for (cs, ce, text) in changes {
            let len = text.chars().count();
            let start = cs.saturating_add_signed(delta);
            for t in self.threads().iter().filter(|t| t.anchor.path == path && (t.anchor.start, t.anchor.end) == (*cs, *ce)) {
                let Some(s) = &t.suggestion else { continue };
                if matches!(t.state, State::Accepted { .. }) && t.anchor.quote.exact == *text {
                    reopen.push((t.id, start, start + len));
                } else if t.can_accept() && s.replacement == *text {
                    reaccept.push((t.id, start, start + len, by.unwrap_or(&s.author).clone()));
                }
            }
            delta += len as isize - (ce - cs) as isize;
        }
        let sizes: Vec<(usize, usize, usize)> = changes.iter().map(|(s, e, t)| (*s, *e, t.chars().count())).collect();
        let mut changed = self.follow(path, &sizes);
        for (id, start, end) in reopen {
            if let Ok(t) = self.get_mut(id) {
                (t.anchor.start, t.anchor.end, t.anchor.placement) = (start, end, Placement::Anchored);
                t.state = State::Open;
                if let Some(s) = t.suggestion.as_mut() {
                    s.outdated = false;
                }
                changed.push(id);
            }
        }
        for (id, start, end, by) in reaccept {
            if let Ok(t) = self.get_mut(id) {
                (t.anchor.start, t.anchor.end, t.anchor.placement) = (start, end, Placement::Anchored);
                t.state = State::Accepted { by, at };
                if let Some(s) = t.suggestion.as_mut() {
                    s.outdated = false;
                }
                changed.push(id);
            }
        }
        changed.sort();
        changed.dedup();
        changed
    }
}
