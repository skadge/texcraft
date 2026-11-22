//! The session's edit log (docs/collaboration-roadmap.md, F1): every edit of a file's text, with
//! its origin, for those who follow the text (the editor, review anchors, later the shared
//! document). It keeps the latest [`KEPT`] edits.

use std::collections::VecDeque;

use texcraft_buffer::{Origin, Transaction};
use texcraft_project::ProjectPath;

/// How many edits the log keeps.
const KEPT: usize = 4096;

/// One edit of a file, as the session made it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileEdit {
    /// The session's revision just after it.
    pub revision: u64,
    pub path: ProjectPath,
    /// Character offsets in the text before it.
    pub changes: Transaction,
    pub origin: Origin,
}

#[derive(Debug, Default)]
pub(crate) struct Log {
    edits: VecDeque<FileEdit>,
    /// The revision of the newest edit dropped.
    dropped: u64,
}

impl Log {
    pub(crate) fn push(&mut self, edit: FileEdit) {
        if self.edits.len() >= KEPT
            && let Some(old) = self.edits.pop_front()
        {
            self.dropped = old.revision;
        }
        self.edits.push_back(edit);
    }

    pub(crate) fn since(&self, path: &ProjectPath, revision: u64) -> Option<Vec<&FileEdit>> {
        if revision < self.dropped {
            return None;
        }
        Some(self.edits.iter().filter(|e| e.revision > revision && &e.path == path).collect())
    }
}

/// `text` with `tx` made, or `None` when a change ends past the text.
pub(crate) fn apply(text: &str, tx: &Transaction) -> Option<String> {
    // Byte offsets of the changes' ends, found in one pass.
    let mut wanted: Vec<usize> = tx.changes().iter().flat_map(|c| [c.start, c.end]).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut bytes = std::collections::HashMap::with_capacity(wanted.len());
    let mut next = wanted.iter().peekable();
    for (chars, (byte, _)) in text.char_indices().chain(std::iter::once((text.len(), ' '))).enumerate() {
        while next.peek().is_some_and(|w| **w == chars) {
            bytes.insert(chars, byte);
            next.next();
        }
        if next.peek().is_none() {
            break;
        }
    }
    if next.peek().is_some() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for c in tx.changes() {
        let (s, e) = (*bytes.get(&c.start)?, *bytes.get(&c.end)?);
        out.push_str(text.get(at..s)?);
        out.push_str(&c.text);
        at = e;
    }
    out.push_str(text.get(at..)?);
    Some(out)
}
