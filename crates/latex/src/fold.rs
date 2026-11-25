//! The regions of a LaTeX text the editor can fold (docs/ROADMAP.md, E9): environments that span
//! lines, sections up to the next heading of the same or a higher level, and the preamble.
//!
//! A region is a range of whole lines. Folded, its first line stays shown and the others hide.
//! Only one region starts on a line: the widest, so a line's fold marker folds what it opens.

use crate::tree::{self, Kind as NodeKind, Node};

/// What a region is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldKind {
    /// `\begin{name}` to `\end{name}`.
    Environment(String),
    /// A sectioning command (`level` as in [`crate::Heading`]) with its title.
    Section { level: u8, title: String },
    /// From `\documentclass` to the line before `\begin{document}`.
    Preamble,
}

/// A foldable region: lines `start..=end` (from 0), `end > start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRegion {
    pub start: usize,
    pub end: usize,
    pub kind: FoldKind,
}

/// The foldable regions of `text`, by their first line, one per line.
pub fn regions(text: &str) -> Vec<FoldRegion> {
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line_of = |byte: usize| starts.partition_point(|s| *s <= byte).saturating_sub(1);
    let mut out = Vec::new();
    let nodes = tree::parse(text);
    let mut document = None;
    environments(text, &nodes, &line_of, &mut out, &mut document);
    let blank = |line: usize| {
        let (s, e) = (starts.get(line).copied().unwrap_or(text.len()), starts.get(line + 1).copied().unwrap_or(text.len()));
        text.get(s..e).is_none_or(|l| l.trim().is_empty())
    };
    let last_line = starts.len().saturating_sub(1);
    // The preamble: `\documentclass` to the line before `\begin{document}`.
    if let Some((begin, _)) = document
        && let Some(class) = text.find("\\documentclass").map(&line_of)
        && class + 1 < begin
    {
        let mut end = begin - 1;
        while end > class && blank(end) {
            end -= 1;
        }
        if end > class {
            out.push(FoldRegion { start: class, end, kind: FoldKind::Preamble });
        }
    }
    // Sections: to the next heading of the same or a higher level, or `\end{document}`.
    let headings = crate::outline(text);
    let stop = document.map_or(last_line + 1, |(_, end)| end);
    for (i, h) in headings.iter().enumerate() {
        let start = h.line as usize;
        let next = headings[i + 1..].iter().find(|n| n.level <= h.level).map_or(stop, |n| n.line as usize);
        let next = if start < stop { next.min(stop) } else { next };
        let mut end = next.saturating_sub(1).min(last_line);
        while end > start && blank(end) {
            end -= 1;
        }
        if end > start {
            out.push(FoldRegion { start, end, kind: FoldKind::Section { level: h.level, title: h.title.clone() } });
        }
    }
    // One region per first line: the widest.
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    out.dedup_by(|later, kept| later.start == kept.start);
    out
}

/// The environments spanning lines among `nodes`, and `document`'s first and last lines.
fn environments(text: &str, nodes: &[Node], line_of: &dyn Fn(usize) -> usize, out: &mut Vec<FoldRegion>, document: &mut Option<(usize, usize)>) {
    for node in nodes {
        let (start, end) = (line_of(node.range.start), line_of(node.range.end.saturating_sub(1).max(node.range.start)));
        match &node.kind {
            NodeKind::Environment { name, body, end: Some(_), args, .. } => {
                if name == "document" {
                    document.get_or_insert((start, end));
                } else if end > start {
                    out.push(FoldRegion { start, end, kind: FoldKind::Environment(name.clone()) });
                }
                for arg in args {
                    environments(text, &arg.body, line_of, out, document);
                }
                environments(text, body, line_of, out, document);
            }
            NodeKind::Environment { body, args, end: None, .. } => {
                for arg in args {
                    environments(text, &arg.body, line_of, out, document);
                }
                environments(text, body, line_of, out, document);
            }
            NodeKind::Math { env: Some(name), closed: true, .. } if end > start => {
                out.push(FoldRegion { start, end, kind: FoldKind::Environment(name.clone()) });
            }
            NodeKind::Verbatim { closed: true, .. } if end > start => {
                if let Some(name) = text.get(node.range.clone()).and_then(|t| t.strip_prefix("\\begin{")).and_then(|t| t.split('}').next()) {
                    out.push(FoldRegion { start, end, kind: FoldKind::Environment(name.to_owned()) });
                }
            }
            NodeKind::Group { body, .. } => environments(text, body, line_of, out, document),
            NodeKind::Command { args, .. } => {
                for arg in args {
                    environments(text, &arg.body, line_of, out, document);
                }
            }
            _ => {}
        }
    }
}
