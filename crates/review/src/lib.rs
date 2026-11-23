//! Review of a project's sources (docs/collaboration.md §6, docs/collaboration-roadmap.md R1):
//! comment threads on passages of the text, who wrote what, and the `.texcraft/review/` file
//! format ([`format`], documented in docs/review-format.md).
//!
//! The crate is pure: no I/O, no clock and no random source. The engine passes the time and the
//! random bits of new ids, reads and writes the files, and feeds the text's edits to
//! [`Review::follow`] so the anchors stay on their passages.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod anchor;
pub mod format;
mod threads;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use anchor::{Anchor, CONTEXT, Placement, Quote};
pub use threads::{Filter, Review};

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

/// Whether a thread is still being discussed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum State {
    Open,
    Resolved { by: Author, at: DateTime<Utc> },
}

/// A discussion of a passage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub id: Id,
    pub anchor: Anchor,
    pub state: State,
    pub messages: Vec<Message>,
}

impl Thread {
    pub fn is_open(&self) -> bool {
        self.state == State::Open
    }

    /// Who started it.
    pub fn author(&self) -> Option<&Author> {
        self.messages.first().map(|m| &m.author)
    }

    /// When it last changed: its last message, edit or resolution.
    pub fn updated(&self) -> Option<DateTime<Utc>> {
        let messages = self.messages.iter().map(|m| m.edited.unwrap_or(m.time).max(m.time));
        let resolved = match &self.state {
            State::Resolved { at, .. } => Some(*at),
            State::Open => None,
        };
        messages.chain(resolved).max()
    }
}
