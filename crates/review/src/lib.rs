//! Review of a project's sources (docs/collaboration.md §6, docs/collaboration-roadmap.md R1 and
//! R2): comment threads on passages of the text, suggested changes (a thread that also proposes a
//! replacement of its passage), who wrote what, and the `.texcraft/review/` file format
//! ([`format`], documented in docs/review-format.md).
//!
//! The crate is pure: no I/O, no clock and no random source. The engine passes the time and the
//! random bits of new ids, reads and writes the files, and feeds the text's edits to
//! [`Review::follow`] so the anchors stay on their passages.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod anchor;
pub mod format;
mod suggestions;
mod threads;
pub mod view;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use anchor::{Anchor, CONTEXT, Placement, Quote};
pub use suggestions::Accepted;
pub use threads::{Filter, Kind, Review};

/// What a review operation returns.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("there is no thread `{0}`")]
    NoThread(Id),
    #[error("there is no message `{0}` in this thread")]
    NoMessage(Id),
    #[error("only its author can change or delete a message")]
    NotAuthor,
    #[error("a comment needs some text")]
    Empty,
    #[error("the suggestion would change nothing: its text is the passage's")]
    NoChange,
    #[error("`{0}` is a comment, not a suggestion")]
    NotSuggestion(Id),
    #[error("`{0}` is a suggestion: accept or reject it instead")]
    IsSuggestion(Id),
    #[error("the suggestion is no longer open")]
    NotOpen,
    #[error("the passage changed since the suggestion was made, so it cannot be applied; reject it, or suggest again")]
    Outdated,
    #[error("an accepted suggestion is part of the text; undo the accept to reopen it")]
    AlreadyAccepted,
    #[error("the passage {start}..{end} is not in the text ({len} characters)")]
    OutOfText { start: usize, end: usize, len: usize },
    #[error("this review file was written by a newer TeXCraft (format version {0}); update TeXCraft to read it")]
    Newer(u64),
    #[error("this is not a review file: {0}")]
    Invalid(String),
}

/// A thread's or a message's id: a ULID (48 bits of milliseconds, 80 random bits), so ids sort
/// by creation time and two people never make the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(ulid::Ulid);

impl Id {
    /// An id made at `millis` (since 1970) from the random bits `random` (80 of them are used).
    pub fn new(millis: u64, random: u128) -> Self {
        Self(ulid::Ulid::from_parts(millis, random))
    }

    pub fn parse(text: &str) -> Option<Self> {
        ulid::Ulid::from_string(text).ok().map(Self)
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = [0; ulid::ULID_LEN];
        f.write_str(self.0.array_to_str(&mut buffer))
    }
}

impl Serialize for Id {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Id::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not an id")))
    }
}

/// Who wrote a message: a display name, which may repeat, and the person's public key (hex), which
/// does not. No accounts: the key is made on first use and kept with the preferences.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    /// An ed25519 public key in hex; empty for an author without one (an agent, for now).
    #[serde(default)]
    pub key: String,
}

impl Author {
    pub fn new(name: impl Into<String>, key: impl Into<String>) -> Self {
        Self { name: name.into(), key: key.into() }
    }

    /// The same person: the same key, or with no keys, the same name.
    pub fn is(&self, other: &Author) -> bool {
        if self.key.is_empty() && other.key.is_empty() { self.name == other.name } else { self.key == other.key }
    }

    /// A short form of the key for people to compare ("3f2a 9c01 77de 4b10"); empty without one.
    pub fn fingerprint(&self) -> String {
        let hex: Vec<char> = self.key.chars().filter(char::is_ascii_hexdigit).take(16).collect();
        hex.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
    }

    /// What colours and badges are derived from: the key, or the name without one.
    pub fn colour_id(&self) -> &str {
        if self.key.is_empty() { &self.name } else { &self.key }
    }

    /// The letter on the author's badge.
    pub fn initial(&self) -> String {
        self.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_owned())
    }
}

/// One message of a thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: Id,
    pub author: Author,
    pub time: DateTime<Utc>,
    pub text: String,
    /// When the text was last changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<DateTime<Utc>>,
    /// Deleted by its author: the text is gone, the place in the thread stays.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

/// Whether a thread is still being discussed. A comment is resolved; a suggestion is accepted
/// (its replacement is now in the text) or rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum State {
    Open,
    Resolved { by: Author, at: DateTime<Utc> },
    Accepted { by: Author, at: DateTime<Utc> },
    Rejected { by: Author, at: DateTime<Utc> },
}

impl State {
    /// Who closed it and when; `None` while open.
    pub fn closed(&self) -> Option<(&Author, DateTime<Utc>)> {
        match self {
            State::Open => None,
            State::Resolved { by, at } | State::Accepted { by, at } | State::Rejected { by, at } => Some((by, *at)),
        }
    }
}

/// A proposed change of a thread's passage: replace it (the anchor's range) with `replacement`.
/// An empty passage is an insertion, an empty replacement a deletion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub author: Author,
    pub time: DateTime<Utc>,
    pub replacement: String,
    /// When its text last changed (consecutive typing extends a suggestion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<DateTime<Utc>>,
    /// The passage was changed by another edit (another suggestion accepted over it, someone
    /// typing inside it): it can no longer be applied, only rejected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outdated: bool,
}

/// A discussion of a passage, and with a [`Suggestion`], a proposed change of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub id: Id,
    pub anchor: Anchor,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Suggestion>,
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl Thread {
    pub fn is_open(&self) -> bool {
        self.state == State::Open
    }

    pub fn is_suggestion(&self) -> bool {
        self.suggestion.is_some()
    }

    /// An open suggestion that can be applied to the text now.
    pub fn can_accept(&self) -> bool {
        self.is_open() && self.anchor.placement != Placement::Unanchored && self.suggestion.as_ref().is_some_and(|s| !s.outdated)
    }

    /// Who started it: the suggestion's author, or the first message's.
    pub fn author(&self) -> Option<&Author> {
        self.suggestion.as_ref().map(|s| &s.author).or_else(|| self.messages.first().map(|m| &m.author))
    }

    /// When it last changed: its suggestion, last message, edit or closing.
    pub fn updated(&self) -> Option<DateTime<Utc>> {
        let messages = self.messages.iter().map(|m| m.edited.unwrap_or(m.time).max(m.time));
        let suggested = self.suggestion.as_ref().map(|s| s.edited.unwrap_or(s.time).max(s.time));
        let closed = self.state.closed().map(|(_, at)| at);
        messages.chain(suggested).chain(closed).max()
    }
}
