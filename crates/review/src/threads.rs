//! A project's threads and what can be done to them.

use chrono::{DateTime, Utc};

use crate::{Anchor, Author, Error, Id, Message, Result, State, Thread};

/// Comments or suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Comment,
    Suggestion,
}

/// Which threads a list shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    /// Only comments, or only suggestions.
    pub kind: Option<Kind>,
    /// Only this file's.
    pub path: Option<String>,
    /// Only open (`Some(true)`) or only resolved (`Some(false)`) ones.
    pub open: Option<bool>,
    /// Only those this author took part in: a name, or a key.
    pub author: Option<String>,
}

impl Filter {
    pub fn matches(&self, t: &Thread) -> bool {
        let by = |author: &Author, a: &str| author.name == a || author.key == a;
        self.path.as_ref().is_none_or(|p| *p == t.anchor.path)
            && self.open.is_none_or(|open| open == t.is_open())
            && self.kind.is_none_or(|k| (k == Kind::Suggestion) == t.is_suggestion())
            && self
                .author
                .as_ref()
                .is_none_or(|a| t.suggestion.as_ref().is_some_and(|s| by(&s.author, a)) || t.messages.iter().any(|m| !m.deleted && by(&m.author, a)))
    }
}

/// Every thread of a project, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Review {
    threads: Vec<Thread>,
}

impl Review {
    pub fn threads(&self) -> &[Thread] {
        &self.threads
    }

    pub fn get(&self, id: Id) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: Id) -> Result<&mut Thread> {
        self.threads.iter_mut().find(|t| t.id == id).ok_or(Error::NoThread(id))
    }

    /// Adds a thread (read from a file, or new), replacing one with its id.
    pub fn insert(&mut self, thread: Thread) {
        self.threads.retain(|t| t.id != thread.id);
        let at = self.threads.partition_point(|t| t.id < thread.id);
        self.threads.insert(at, thread);
    }

    /// Starts a thread on `anchor` with its first message.
    pub fn comment(&mut self, id: Id, anchor: Anchor, first: Message) -> Result<&Thread> {
        if first.text.trim().is_empty() {
            return Err(Error::Empty);
        }
        self.insert(Thread { id, anchor, state: State::Open, suggestion: None, messages: vec![first] });
        self.get(id).ok_or(Error::NoThread(id))
    }

    pub fn reply(&mut self, thread: Id, message: Message) -> Result<()> {
        if message.text.trim().is_empty() {
            return Err(Error::Empty);
        }
        self.get_mut(thread)?.messages.push(message);
        Ok(())
    }

    fn own_message<'a>(&'a mut self, thread: Id, message: Id, by: &Author) -> Result<&'a mut Message> {
        let m = self.get_mut(thread)?.messages.iter_mut().find(|m| m.id == message).ok_or(Error::NoMessage(message))?;
        if !m.author.is(by) {
            return Err(Error::NotAuthor);
        }
        Ok(m)
    }

    /// Changes the text of one's own message.
    pub fn edit(&mut self, thread: Id, message: Id, by: &Author, text: &str, at: DateTime<Utc>) -> Result<()> {
        if text.trim().is_empty() {
            return Err(Error::Empty);
        }
        let m = self.own_message(thread, message, by)?;
        if m.deleted {
            return Err(Error::NoMessage(message));
        }
        m.text = text.to_owned();
        m.edited = Some(at);
        Ok(())
    }

    /// Deletes one's own message: its text goes, its place in the thread stays.
    pub fn delete_message(&mut self, thread: Id, message: Id, by: &Author) -> Result<()> {
        let m = self.own_message(thread, message, by)?;
        m.text.clear();
        m.deleted = true;
        Ok(())
    }

    /// Deletes a thread its author started, with all its messages (a suggestion: withdraws it).
    pub fn remove(&mut self, thread: Id, by: &Author) -> Result<Thread> {
        let t = self.get_mut(thread)?;
        if !t.author().is_some_and(|a| a.is(by)) {
            return Err(Error::NotAuthor);
        }
        let at = self.threads.iter().position(|t| t.id == thread).ok_or(Error::NoThread(thread))?;
        Ok(self.threads.remove(at))
    }

    /// Resolves a comment (a suggestion is accepted or rejected instead).
    pub fn resolve(&mut self, thread: Id, by: &Author, at: DateTime<Utc>) -> Result<()> {
        let t = self.get_mut(thread)?;
        if t.is_suggestion() {
            return Err(Error::IsSuggestion(thread));
        }
        t.state = State::Resolved { by: by.clone(), at };
        Ok(())
    }

    /// Reopens a resolved comment or a rejected suggestion.
    pub fn reopen(&mut self, thread: Id) -> Result<()> {
        let t = self.get_mut(thread)?;
        if matches!(t.state, State::Accepted { .. }) {
            return Err(Error::AlreadyAccepted);
        }
        t.state = State::Open;
        Ok(())
    }

    /// The threads `filter` keeps, oldest first.
    pub fn list<'a>(&'a self, filter: &'a Filter) -> impl Iterator<Item = &'a Thread> + 'a {
        self.threads.iter().filter(move |t| filter.matches(t))
    }

    /// Moves the anchors in `path` with an edit of it: `(start, end, inserted characters)`,
    /// sorted, not overlapping, in the text before the edit. An open suggestion whose passage the
    /// edit changes becomes outdated; returns those that did.
    pub fn follow(&mut self, path: &str, changes: &[(usize, usize, usize)]) -> Vec<Id> {
        let mut outdated = Vec::new();
        for t in self.threads.iter_mut().filter(|t| t.anchor.path == path) {
            if t.state == State::Open
                && let Some(s) = t.suggestion.as_mut()
                && !s.outdated
                && crate::suggestions::changed_inside(&t.anchor, changes)
            {
                s.outdated = true;
                outdated.push(t.id);
            }
            t.anchor.follow(changes);
        }
        outdated
    }

    /// A file was renamed: its threads go with it. Returns the ids that changed.
    pub fn rename(&mut self, from: &str, to: &str) -> Vec<Id> {
        let mut changed = Vec::new();
        for t in self.threads.iter_mut().filter(|t| t.anchor.path == from) {
            t.anchor.path = to.to_owned();
            changed.push(t.id);
        }
        changed
    }

    /// Every anchor of `path`, for the text as it is now (when it is saved). An accepted or
    /// rejected suggestion keeps the passage it replaced.
    pub fn requote(&mut self, path: &str, text: &str) {
        for t in self.threads.iter_mut().filter(|t| t.anchor.path == path && (t.is_open() || !t.is_suggestion())) {
            t.anchor.requote(text);
        }
    }

    /// Puts the anchors of `path` on their passages in `text`, the file as read.
    pub fn place(&mut self, path: &str, text: &str) {
        for t in self.threads.iter_mut().filter(|t| t.anchor.path == path) {
            t.anchor.place(text);
        }
    }
}

impl Message {
    pub fn new(id: Id, author: Author, time: DateTime<Utc>, text: impl Into<String>) -> Self {
        Self { id, author, time, text: text.into(), edited: None, deleted: false }
    }
}
