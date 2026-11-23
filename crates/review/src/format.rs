//! The `.texcraft/review/` files (docs/review-format.md): one JSON file per thread, named by its
//! id, with a format `version`. A file of a newer version is refused with a message instead of
//! being read wrongly.

use serde::Serialize;
use serde_json::Value;

use crate::{Error, Id, Result, Thread};

/// The format this crate writes, and the newest it reads.
pub const VERSION: u64 = 1;

/// Where a project keeps its review, relative to the project's folder.
pub const DIR: &str = ".texcraft/review";

/// The file a thread is kept in, in [`DIR`].
pub fn file_name(id: Id) -> String {
    format!("{id}.json")
}

#[derive(Serialize)]
struct File<'a> {
    version: u64,
    kind: &'static str,
    #[serde(flatten)]
    thread: &'a Thread,
}

/// The file's text for `thread`: pretty JSON, ending with a newline, so it diffs well in git.
pub fn write(thread: &Thread) -> Result<String> {
    let file = File { version: VERSION, kind: "thread", thread };
    let mut text = serde_json::to_string_pretty(&file).map_err(|e| Error::Invalid(e.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// Reads a thread's file.
pub fn read(text: &str) -> Result<Thread> {
    let value: Value = serde_json::from_str(text).map_err(|e| Error::Invalid(e.to_string()))?;
    let version = value.get("version").and_then(Value::as_u64).ok_or_else(|| Error::Invalid("it has no `version`".to_owned()))?;
    if version > VERSION {
        return Err(Error::Newer(version));
    }
    match value.get("kind").and_then(Value::as_str) {
        Some("thread") => {}
        Some(other) => return Err(Error::Invalid(format!("it holds a `{other}`, not a thread"))),
        None => return Err(Error::Invalid("it has no `kind`".to_owned())),
    }
    serde_json::from_value(value).map_err(|e| Error::Invalid(e.to_string()))
}
