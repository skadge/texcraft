//! Integration tests for texcraft-review.

mod anchors;
mod model;

use chrono::{DateTime, TimeZone, Utc};
use texcraft_review::{Anchor, Author, Id, Message, Review};

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_790_000_000 + seconds, 0).single().unwrap_or_default()
}

fn ada() -> Author {
    Author::new("Ada", "3f2a9c0177de4b10aa")
}

/// A review with one thread on `start..end` of `text`, in `main.tex`.
fn one_thread(text: &str, start: usize, end: usize) -> Result<(Review, Id), texcraft_review::Error> {
    let mut r = Review::default();
    let id = Id::new(1_000, 42);
    let first = Message::new(Id::new(1_000, 43), ada(), at(0), "Is this right?");
    r.comment(id, Anchor::new("main.tex", text, start, end), first)?;
    Ok((r, id))
}
