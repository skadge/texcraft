//! The message of a version a save makes: what it changed, so the history can be read without
//! opening each version (ROADMAP C9).
//!
//! The subject names the files added, edited, deleted and renamed (at most three names, then how
//! many others) and, for an edited LaTeX file, the sections whose text changed, as the outline
//! names them: "Edited Introduction and Results (main.tex); added figures/plot.png". It is kept to
//! [`SUBJECT_MAX`] characters, saying less when it would be longer (the sections left out, then the
//! folders, then only how many files); the body then lists each file. Like git's own messages they
//! are written in English: they are the project's, read by anyone and any tool.

use std::collections::BTreeMap;

use texcraft_history::store::blob_id;

/// At most this many characters in a subject, as git's conventions have it.
pub const SUBJECT_MAX: usize = 72;

/// At most this many names in a list before "and N others".
const NAMES_MAX: usize = 3;

/// How a file changed since the version before.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    Added(String),
    Deleted(String),
    /// Edited, with the sections whose text changed (none for a file without an outline).
    Edited(String, Vec<String>),
    Renamed(String, String),
}

/// What a save changed: the latest version's files as paths and blob ids (`before`), the files
/// now; `old_text` reads a file of the latest version, for an edited LaTeX file's sections.
/// Hidden files (the review's threads) are not named: a save that changed only those updated the
/// review comments.
pub fn message(before: &[(String, String)], now: &[(String, Vec<u8>)], old_text: &dyn Fn(&str) -> Option<Vec<u8>>) -> String {
    let before: BTreeMap<&str, &str> = before.iter().filter(|(p, _)| !hidden(p)).map(|(p, id)| (p.as_str(), id.as_str())).collect();
    let now: BTreeMap<&str, &[u8]> = now.iter().filter(|(p, _)| !hidden(p)).map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let id = |bytes: &[u8]| blob_id(bytes).unwrap_or_default();
    let mut added: Vec<(&str, String)> = now.iter().filter(|(p, _)| !before.contains_key(*p)).map(|(p, b)| (*p, id(b))).collect();
    let mut deleted: Vec<(&str, &str)> = before.iter().filter(|(p, _)| !now.contains_key(*p)).map(|(p, i)| (*p, *i)).collect();
    let mut changes = Vec::new();
    for (path, bytes) in &now {
        let Some(old) = before.get(path) else { continue };
        if *old != id(bytes) {
            let sections = if is_latex(path) {
                old_text(path).map(|old| sections(&String::from_utf8_lossy(&old), &String::from_utf8_lossy(bytes))).unwrap_or_default()
            } else {
                Vec::new()
            };
            changes.push(Change::Edited((*path).to_owned(), sections));
        }
    }
    // A file deleted and one added with the same bytes: moved or renamed.
    let mut renamed = Vec::new();
    deleted.retain(|(from, old)| match added.iter().position(|(_, new)| new == old) {
        Some(i) => {
            renamed.push(Change::Renamed((*from).to_owned(), added.remove(i).0.to_owned()));
            false
        }
        None => true,
    });
    changes.extend(added.into_iter().map(|(p, _)| Change::Added(p.to_owned())));
    changes.extend(deleted.into_iter().map(|(p, _)| Change::Deleted(p.to_owned())));
    changes.extend(renamed);
    if changes.is_empty() {
        // Only hidden files changed, and those a save writes are the review's.
        return "Updated the review comments".to_owned();
    }
    compose(&changes)
}

fn hidden(path: &str) -> bool {
    texcraft_history::is_hidden(path)
}

fn is_latex(path: &str) -> bool {
    [".tex", ".ltx", ".sty", ".cls"].iter().any(|e| path.ends_with(e))
}

/// The headings of `new` (and those only in `old`) whose text changed: a section's text runs from
/// its heading to the next heading of any level. Text before the first heading is not a section.
fn sections(old: &str, new: &str) -> Vec<String> {
    let parts = |text: &str| -> Vec<(String, String)> {
        let lines: Vec<&str> = text.lines().collect();
        let headings = texcraft_latex::outline(text);
        headings
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let start = (h.line as usize).min(lines.len());
                let end = headings.get(i + 1).map_or(lines.len(), |n| (n.line as usize).clamp(start, lines.len()));
                (h.title.clone(), lines[start..end].join("\n"))
            })
            .collect()
    };
    let (old, new) = (parts(old), parts(new));
    // Headings told apart by their title and which of that title they are.
    let keyed = |parts: &[(String, String)]| -> Vec<((String, usize), String)> {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        parts
            .iter()
            .map(|(title, text)| {
                let n = seen.entry(title.clone()).or_default();
                *n += 1;
                ((title.clone(), *n), text.clone())
            })
            .collect()
    };
    let (old, new) = (keyed(&old), keyed(&new));
    let old_map: BTreeMap<&(String, usize), &String> = old.iter().map(|(k, t)| (k, t)).collect();
    let new_map: BTreeMap<&(String, usize), &String> = new.iter().map(|(k, t)| (k, t)).collect();
    let mut out: Vec<String> = Vec::new();
    for (key, text) in &new {
        if old_map.get(key) != Some(&text) && !out.contains(&key.0) {
            out.push(key.0.clone());
        }
    }
    for (key, _) in &old {
        if !new_map.contains_key(key) && !out.contains(&key.0) {
            out.push(key.0.clone());
        }
    }
    out
}

/// "a", "a and b", "a, b and c", "a, b, c and 2 others".
fn list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        _ if names.len() > NAMES_MAX => {
            let rest = names.len() - NAMES_MAX;
            format!("{} and {rest} other{}", names[..NAMES_MAX].join(", "), if rest == 1 { "" } else { "s" })
        }
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// "1 file", "3 files".
fn files(n: usize) -> String {
    format!("{n} file{}", if n == 1 { "" } else { "s" })
}

/// How much a subject says.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Detail {
    /// Paths, and the sections of an edited file.
    Sections,
    /// Paths only.
    Paths,
    /// File names, without their folders.
    Names,
    /// How many files.
    Counts,
}

fn name(path: &str, detail: Detail) -> String {
    if detail >= Detail::Names { path.rsplit('/').next().unwrap_or(path).to_owned() } else { path.to_owned() }
}

/// The clauses of a subject, at a level of detail: edited, added, deleted, renamed.
fn clauses(changes: &[Change], detail: Detail) -> Vec<String> {
    let edited: Vec<(&String, &Vec<String>)> =
        changes.iter().filter_map(|c| if let Change::Edited(p, s) = c { Some((p, s)) } else { None }).collect();
    let pick = |f: fn(&Change) -> Option<&String>| -> Vec<String> { changes.iter().filter_map(f).map(|p| name(p, detail)).collect() };
    let added = pick(|c| if let Change::Added(p) = c { Some(p) } else { None });
    let deleted = pick(|c| if let Change::Deleted(p) = c { Some(p) } else { None });
    let renamed: Vec<(&String, &String)> = changes.iter().filter_map(|c| if let Change::Renamed(a, b) = c { Some((a, b)) } else { None }).collect();
    let mut out = Vec::new();
    if !edited.is_empty() {
        out.push(match (detail, edited.as_slice()) {
            (Detail::Counts, _) => format!("edited {}", files(edited.len())),
            (Detail::Sections, [(path, sections)]) if !sections.is_empty() => format!("edited {} ({path})", list(sections)),
            (Detail::Sections, _) if edited.len() <= NAMES_MAX => {
                let each: Vec<String> = edited.iter().map(|(p, s)| if s.is_empty() { (*p).clone() } else { format!("{} ({p})", list(s)) }).collect();
                format!("edited {}", each.join(", "))
            }
            _ => format!("edited {}", list(&edited.iter().map(|(p, _)| name(p, detail)).collect::<Vec<_>>())),
        });
    }
    for (verb, names) in [("added", &added), ("deleted", &deleted)] {
        if names.is_empty() {
            continue;
        }
        out.push(if detail == Detail::Counts { format!("{verb} {}", files(names.len())) } else { format!("{verb} {}", list(names)) });
    }
    match renamed.as_slice() {
        [] => {}
        [(from, to)] if detail < Detail::Counts => {
            let to =
                if from.rsplit_once('/').map(|(d, _)| d) == to.rsplit_once('/').map(|(d, _)| d) { name(to, Detail::Names) } else { (*to).clone() };
            out.push(format!("renamed {} to {}", name(from, detail), name(&to, detail)));
        }
        _ => out.push(format!("renamed {}", files(renamed.len()))),
    }
    out
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The subject at the most detail that fits, then the body when the subject left something out
/// or more than one file changed.
fn compose(changes: &[Change]) -> String {
    let (detail, mut subject) = [Detail::Sections, Detail::Paths, Detail::Names, Detail::Counts]
        .into_iter()
        .map(|d| (d, capitalised(&clauses(changes, d).join("; "))))
        .find(|(d, s)| s.chars().count() <= SUBJECT_MAX || *d == Detail::Counts)
        .unwrap_or((Detail::Counts, String::new()));
    if subject.chars().count() > SUBJECT_MAX {
        subject = subject.chars().take(SUBJECT_MAX - 1).collect::<String>() + "…";
    }
    let with_sections = |c: &Change| matches!(c, Change::Edited(_, s) if !s.is_empty());
    let said_all = detail == Detail::Sections || (detail == Detail::Paths && !changes.iter().any(with_sections));
    if said_all && changes.len() == 1 {
        return subject;
    }
    let body: Vec<String> = changes
        .iter()
        .map(|c| match c {
            Change::Added(p) => format!("Added {p}"),
            Change::Deleted(p) => format!("Deleted {p}"),
            Change::Edited(p, s) if s.is_empty() => format!("Edited {p}"),
            Change::Edited(p, s) => format!("Edited {p}: {}", s.join(", ")),
            Change::Renamed(a, b) => format!("Renamed {a} to {b}"),
        })
        .collect();
    format!("{subject}\n\n{}", body.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(files: &[(&str, &str)]) -> Vec<(String, String)> {
        files.iter().map(|(p, t)| ((*p).to_owned(), blob_id(t.as_bytes()).unwrap_or_default())).collect()
    }

    fn now(files: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        files.iter().map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec())).collect()
    }

    /// The message of going from `old` to `new`.
    fn described(old: &[(&str, &str)], new: &[(&str, &str)]) -> String {
        let text = |path: &str| old.iter().find(|(p, _)| *p == path).map(|(_, t)| t.as_bytes().to_vec());
        message(&ids(old), &now(new), &text)
    }

    fn subject(message: &str) -> &str {
        message.lines().next().unwrap_or_default()
    }

    const PAPER: &str = "\\documentclass{article}\n\\begin{document}\n\\section{Introduction}\nWe begin.\n\\section{Method}\nWe measure.\n\\section{Results}\nIt works.\n\\end{document}\n";

    #[test]
    fn one_section_edited_is_named_with_its_file_and_needs_no_body() {
        let m = described(&[("main.tex", PAPER)], &[("main.tex", &PAPER.replace("We begin.", "We start."))]);
        assert_eq!(m, "Edited Introduction (main.tex)");
    }

    #[test]
    fn sections_edited_and_a_file_added_share_the_subject() {
        let edited = PAPER.replace("We begin.", "We start.").replace("It works.", "It works well.");
        let m = described(&[("main.tex", PAPER)], &[("main.tex", &edited), ("figures/plot.png", "png")]);
        assert_eq!(subject(&m), "Edited Introduction and Results (main.tex); added figures/plot.png");
        assert!(m.contains("\n\nEdited main.tex: Introduction, Results\nAdded figures/plot.png"), "the body lists each file: {m}");
    }

    #[test]
    fn a_heading_renamed_or_removed_counts_as_touched() {
        let m = described(&[("main.tex", PAPER)], &[("main.tex", &PAPER.replace("{Method}", "{Methods}"))]);
        assert_eq!(subject(&m), "Edited Methods and Method (main.tex)");
        let preamble = described(&[("main.tex", PAPER)], &[("main.tex", &PAPER.replace("article", "report"))]);
        assert_eq!(subject(&preamble), "Edited main.tex", "the text before the first heading is no section");
    }

    #[test]
    fn many_files_are_listed_up_to_three_and_a_long_subject_says_less() {
        let old: Vec<(String, String)> = (1..=6).map(|i| (format!("chapters/chapter-{i}.tex"), format!("old {i}\n"))).collect();
        let old: Vec<(&str, &str)> = old.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let new: Vec<(String, String)> = (1..=6).map(|i| (format!("chapters/chapter-{i}.tex"), format!("new {i}\n"))).collect();
        let new: Vec<(&str, &str)> = new.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let m = described(&old, &new);
        assert!(subject(&m).chars().count() <= SUBJECT_MAX, "{m}");
        assert!(subject(&m).ends_with("and 3 others"), "{m}");
        assert_eq!(m.lines().filter(|l| l.starts_with("Edited chapters/")).count(), 6, "the body names every file: {m}");

        let lots: Vec<(String, String)> = (1..=4).map(|i| (format!("figures/a-rather-long-figure-name-{i}.png"), "x".repeat(i))).collect();
        let lots: Vec<(&str, &str)> = lots.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let m = described(&[], &lots);
        assert!(subject(&m).chars().count() <= SUBJECT_MAX, "{m}");
        assert!(m.lines().count() > 4, "what the subject leaves out is in the body: {m}");
    }

    #[test]
    fn a_file_moved_unchanged_is_a_rename_and_binary_files_are_named() {
        let m = described(&[("main.tex", PAPER), ("plot.png", "\u{0}png")], &[("main.tex", PAPER), ("figures/plot.png", "\u{0}png")]);
        assert_eq!(m, "Renamed plot.png to figures/plot.png");
        let m = described(&[("main.tex", PAPER), ("plot.png", "\u{0}png")], &[("main.tex", PAPER), ("plot.png", "\u{0}PNG")]);
        assert_eq!(m, "Edited plot.png");
        let m = described(&[("main.tex", PAPER), ("old.bib", "@a")], &[("main.tex", PAPER)]);
        assert_eq!(m, "Deleted old.bib");
    }

    #[test]
    fn only_the_review_changed_is_said() {
        let m = message(&ids(&[("main.tex", PAPER)]), &now(&[("main.tex", PAPER), (".texcraft/review/t.json", "{}")]), &|_| None);
        assert_eq!(m, "Updated the review comments");
    }
}
