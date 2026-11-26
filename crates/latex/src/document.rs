//! The whole document's outline (docs/ROADMAP.md, E14): the headings of the main file and of the
//! files it `\input`s and `\include`s, each file's in the place it is included, in the order the
//! document reads, with the numbers the PDF gives them ([`crate::numbers`]). An include whose file
//! is not in the project, or that would read a file already being read (a loop), is an entry of
//! its own, so the outline can say so.
//!
//! Includes are followed as the numbering follows them: on a line of their own, in the body.

use std::sync::Arc;

use crate::numbers::{self, Numbers};
use crate::view::{self, Block, BlockKind};

/// One entry of the document's outline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Entry {
    /// The project file the entry is in.
    pub file: String,
    /// Its line in that file, from 0.
    pub line: u32,
    /// How deep the file is included: 0 for the main file.
    pub depth: usize,
    #[serde(flatten)]
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryKind {
    /// A sectioning command.
    Heading {
        /// 0 for `\part` down to 6 for `\subparagraph`.
        level: u8,
        title: String,
        /// The number the PDF shows ("2.1"), when it numbers the heading.
        #[serde(skip_serializing_if = "Option::is_none")]
        number: Option<String>,
    },
    /// An `\input` or `\include` that is not followed: `name` as written.
    Include { command: String, name: String, problem: Problem },
}

/// Why an include is not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// No project file has the name.
    Missing,
    /// The file is being read already: it includes itself, or one that includes it.
    Loop,
    /// Included more deeply than is followed.
    TooDeep,
}

/// How deep includes are followed.
const DEEPEST: usize = 16;

/// The outline of the document whose main file is `main`: `read` gives a project file's text,
/// `exists` whether it is one, `numbers` the document's numbers when they are known.
pub fn outline(main: &str, read: &dyn Fn(&str) -> Option<Arc<str>>, exists: &dyn Fn(&str) -> bool, numbers: Option<&Numbers>) -> Vec<Entry> {
    let theorems = read(main).map(|t| view::theorems(&t)).unwrap_or_default();
    let mut walk = Walk { main, read, exists, numbers, theorems, open: Vec::new(), out: Vec::new() };
    walk.file(main);
    walk.out
}

/// The files the document reads, in the order the outline has them: the main file first.
pub fn files(entries: &[Entry]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for e in entries {
        if !out.contains(&e.file.as_str()) {
            out.push(&e.file);
        }
    }
    out
}

struct Walk<'a> {
    main: &'a str,
    read: &'a dyn Fn(&str) -> Option<Arc<str>>,
    exists: &'a dyn Fn(&str) -> bool,
    numbers: Option<&'a Numbers>,
    /// The theorem-like environments the main file declares, which the numbering's view has.
    theorems: Vec<view::TheoremKind>,
    /// The files being read, from the main file down.
    open: Vec<String>,
    out: Vec<Entry>,
}

/// The line of byte `at` of `text`, from 0.
fn line_of(text: &str, at: usize) -> u32 {
    u32::try_from(text.get(..at).map_or(0, |t| t.matches('\n').count())).unwrap_or(u32::MAX)
}

impl Walk<'_> {
    fn file(&mut self, path: &str) {
        let Some(text) = (self.read)(path) else { return };
        self.open.push(path.to_owned());
        // The headings' titles as the outline of one file has them, by line.
        let titled = crate::outline(&text);
        let blocks = view::view_with(&text, &self.theorems);
        let mut places = Vec::new();
        view::walk(&blocks, &mut |b: &Block| match &b.kind {
            BlockKind::Heading { .. } | BlockKind::Include { .. } => places.push(b),
            _ => {}
        });
        let depth = self.open.len() - 1;
        for b in places {
            let line = line_of(&text, b.range.start);
            match &b.kind {
                BlockKind::Heading { level, .. } => {
                    let title = titled
                        .iter()
                        .find(|h| h.line == line && h.level == *level)
                        .map(|h| h.title.clone())
                        .unwrap_or_else(|| text.get(b.range.clone()).unwrap_or_default().to_owned());
                    let number = self.numbers.and_then(|n| n.one(path, b.range.start)).map(str::to_owned);
                    self.out.push(Entry { file: path.to_owned(), line, depth, kind: EntryKind::Heading { level: *level, title, number } });
                }
                BlockKind::Include { command, name } => {
                    let problem = match numbers::resolve(self.main, name, self.exists) {
                        None => Some(Problem::Missing),
                        Some(found) if self.open.contains(&found) => Some(Problem::Loop),
                        Some(_) if self.open.len() >= DEEPEST => Some(Problem::TooDeep),
                        Some(found) => {
                            self.file(&found);
                            None
                        }
                    };
                    if let Some(problem) = problem {
                        let kind = EntryKind::Include { command: command.clone(), name: name.clone(), problem };
                        self.out.push(Entry { file: path.to_owned(), line, depth, kind });
                    }
                }
                _ => {}
            }
        }
        self.open.pop();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn document(files: &[(&str, &str)]) -> Vec<Entry> {
        let map: HashMap<String, Arc<str>> = files.iter().map(|(p, t)| ((*p).to_owned(), Arc::from(*t))).collect();
        let read = |p: &str| map.get(p).cloned();
        let exists = |p: &str| map.contains_key(p);
        let numbers = numbers::number("main.tex", &read, &exists, None);
        outline("main.tex", &read, &exists, Some(&numbers))
    }

    fn headings(entries: &[Entry]) -> Vec<(String, &str)> {
        entries
            .iter()
            .filter_map(|e| match &e.kind {
                EntryKind::Heading { title, .. } => Some((title.clone(), e.file.as_str())),
                EntryKind::Include { .. } => None,
            })
            .collect()
    }

    const MAIN: &str = "\\documentclass{article}\n\\begin{document}\n\\section{Start}\n\\input{chapters/one}\n\\section{End}\n\\end{document}\n";

    #[test]
    fn an_included_file_s_headings_come_in_its_place_with_the_document_s_numbers() {
        let one = "\\section{One}\n\\subsection{Deeper}\n\\input{chapters/two}\n";
        let two = "\\subsection{Two}\n";
        let entries = document(&[("main.tex", MAIN), ("chapters/one.tex", one), ("chapters/two.tex", two)]);
        let found = headings(&entries);
        let order: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(order, ["Start", "One", "Deeper", "Two", "End"]);
        assert_eq!(found[3].1, "chapters/two.tex", "a nested include's headings are its file's");
        let depth = |title: &str| entries.iter().find(|e| matches!(&e.kind, EntryKind::Heading { title: t, .. } if t == title)).map(|e| e.depth);
        assert!(depth("Two") > depth("One") && depth("One") > depth("Start"), "each include is one deeper");
        let number = |title: &str| {
            entries.iter().find_map(|e| match &e.kind {
                EntryKind::Heading { title: t, number, .. } if t == title => number.clone(),
                _ => None,
            })
        };
        // Numbered through the files, as the PDF numbers them.
        assert_eq!(number("End").as_deref(), Some("3"));
        assert_eq!(number("Two").as_deref(), Some("2.2"));
        let line = entries.iter().find(|e| matches!(&e.kind, EntryKind::Heading { title, .. } if title == "Deeper")).map(|e| e.line);
        assert_eq!(line, Some(1), "an entry has its line in its own file");
    }

    #[test]
    fn a_missing_file_and_a_loop_are_entries_not_gaps() {
        let one = "\\section{One}\n\\input{main}\n\\input{chapters/gone}\n";
        let entries = document(&[("main.tex", MAIN), ("chapters/one.tex", one)]);
        let problems: Vec<(&str, Problem)> = entries
            .iter()
            .filter_map(|e| match &e.kind {
                EntryKind::Include { name, problem, .. } => Some((name.as_str(), *problem)),
                EntryKind::Heading { .. } => None,
            })
            .collect();
        assert_eq!(problems, [("main", Problem::Loop), ("chapters/gone", Problem::Missing)]);
        let order: Vec<String> = headings(&entries).into_iter().map(|(t, _)| t).collect();
        assert_eq!(order, ["Start", "One", "End"], "a loop is not read twice");
    }

    #[test]
    fn a_file_that_includes_itself_ends() {
        let entries = document(&[("main.tex", "\\section{A}\n\\input{main}\n")]);
        assert_eq!(headings(&entries).len(), 1);
        assert_eq!(files(&entries), ["main.tex"]);
    }
}
