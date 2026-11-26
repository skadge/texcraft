//! Whether a rewritten passage keeps the LaTeX of the original (docs/ROADMAP.md, AI3): the same
//! labels, the same references and citations (by key), the same formulas (up to spacing) and every
//! command it used. A model asked to make a passage clearer or shorter, or to translate it, must
//! change only its words; [`compare`] says what else it changed, so the rewrite can be refused
//! with the reason.
//!
//! A passage changed as the user asked (fleshed out, extended) may gain text and LaTeX, and change
//! its markup, but not lose or alter its labels, references, citations or formulas: [`dropped`]
//! says what it lost.
//!
//! Comments are ignored. Formulas are compared whole, delimiters included: `$x$` and `\(x\)` are
//! different spellings, and a formula is one item however many commands are in it.

use std::collections::{BTreeMap, BTreeSet};

use crate::{MATH_ENVIRONMENTS, control_sequence_end, environment_name};

/// What a change is to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Label,
    Reference,
    Citation,
    Math,
    Command,
}

/// Something the rewrite dropped (or changed), or added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub part: Part,
    /// As written in the text it is from: `\ref{fig:a}`, `$x^2$`, `\emph`.
    pub item: String,
    /// Gone from the rewrite; else new in it.
    pub dropped: bool,
}

/// What `rewritten` changed of `original`'s LaTeX, besides its words; empty when it kept it all.
/// Commands only count when dropped (a rewrite may add emphasis); labels, references, citations
/// and formulas count both ways.
pub fn compare(original: &str, rewritten: &str) -> Vec<Change> {
    let (a, b) = (scan(original), scan(rewritten));
    let mut out = Vec::new();
    for (part, x, y) in [(Part::Label, &a.labels, &b.labels), (Part::Reference, &a.refs, &b.refs), (Part::Citation, &a.cites, &b.cites)] {
        out.extend(x.iter().filter(|(k, _)| !y.contains_key(*k)).map(|(_, shown)| Change { part, item: shown.clone(), dropped: true }));
        out.extend(y.iter().filter(|(k, _)| !x.contains_key(*k)).map(|(_, shown)| Change { part, item: shown.clone(), dropped: false }));
    }
    let mut left: Vec<&(String, String)> = b.math.iter().collect();
    for m in &a.math {
        match left.iter().position(|n| n.0 == m.0) {
            Some(i) => {
                left.remove(i);
            }
            None => out.push(Change { part: Part::Math, item: shorten(&m.1), dropped: true }),
        }
    }
    out.extend(left.into_iter().map(|m| Change { part: Part::Math, item: shorten(&m.1), dropped: false }));
    out.extend(a.commands.difference(&b.commands).map(|c| Change { part: Part::Command, item: c.clone(), dropped: true }));
    out
}

/// What `changed` dropped or altered of `original`'s labels, references, citations and formulas,
/// when the passage was changed as the user asked (bullet points fleshed out into paragraphs):
/// new text and LaTeX may be added, and its markup may change (a list may become prose), but every
/// label, reference and citation key and every formula, as written up to spacing, must still be
/// there. An altered formula counts as dropped; additions never count.
pub fn dropped(original: &str, changed: &str) -> Vec<Change> {
    compare(original, changed).into_iter().filter(|c| c.dropped && c.part != Part::Command).collect()
}

/// A formula as shown in a message: at most 60 characters.
fn shorten(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 60 { one_line } else { one_line.chars().take(59).collect::<String>() + "…" }
}

/// The LaTeX of a passage: keys by kind (each with how it was first written), formulas (spacing
/// made even, and as written) and command names.
#[derive(Default)]
struct Found {
    labels: BTreeMap<String, String>,
    refs: BTreeMap<String, String>,
    cites: BTreeMap<String, String>,
    math: Vec<(String, String)>,
    commands: BTreeSet<String>,
}

fn is_reference(name: &str) -> bool {
    matches!(name, "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref" | "crefrange" | "nameref" | "vref" | "labelcref" | "hyperref")
}

fn is_citation(name: &str) -> bool {
    name.to_ascii_lowercase().contains("cite")
}

fn scan(text: &str) -> Found {
    let mut found = Found::default();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => i = text[i..].find('\n').map_or(text.len(), |n| i + n),
            b'$' => {
                let close = if bytes.get(i + 1) == Some(&b'$') { "$$" } else { "$" };
                let end = closing(text, i + close.len(), close);
                found.math.push(math(&text[i..end]));
                i = end;
            }
            b'\\' => {
                let end = control_sequence_end(text, i);
                let name = &text[i + 1..end];
                match name {
                    "(" | "[" => {
                        let end = closing(text, end, if name == "(" { "\\)" } else { "\\]" });
                        found.math.push(math(&text[i..end]));
                        i = end;
                    }
                    "begin" => match environment_name(text, end) {
                        Some((env, after)) if MATH_ENVIRONMENTS.contains(&&text[env.clone()]) => {
                            let end = closing(text, after, &format!("\\end{{{}}}", &text[env]));
                            found.math.push(math(&text[i..end]));
                            i = end;
                        }
                        Some((env, after)) => {
                            found.commands.insert(format!("\\begin{{{}}}", &text[env]));
                            i = after;
                        }
                        None => i = end,
                    },
                    // The end of an environment goes with its beginning.
                    "end" => i = environment_name(text, end).map_or(end, |(_, after)| after),
                    _ if name.starts_with(|c: char| c.is_ascii_alphabetic()) => {
                        let bare = name.trim_end_matches('*');
                        let kind = if bare == "label" {
                            Some(&mut found.labels)
                        } else if is_reference(bare) {
                            Some(&mut found.refs)
                        } else if is_citation(bare) {
                            Some(&mut found.cites)
                        } else {
                            None
                        };
                        match kind {
                            Some(keys) => {
                                let (list, after) = argument(text, end);
                                for key in list.split(',').map(str::trim).filter(|k| !k.is_empty()) {
                                    keys.entry(key.to_owned()).or_insert_with(|| format!("\\{bare}{{{key}}}"));
                                }
                                i = after;
                            }
                            None => {
                                found.commands.insert(format!("\\{bare}"));
                                i = end;
                            }
                        }
                    }
                    // A control symbol (`\%`, `\\`, `\,`): punctuation, not markup to keep.
                    _ => i = end,
                }
            }
            _ => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    found
}

/// A formula: with its spacing made even (to compare), and as written.
fn math(span: &str) -> (String, String) {
    (span.split_whitespace().collect::<Vec<_>>().join(" "), span.to_owned())
}

/// Where the formula opened before `from` ends: after `close` (a `$` escaped as `\$` does not
/// close it), or at the end of the text.
fn closing(text: &str, from: usize, close: &str) -> usize {
    let mut at = from;
    while let Some(n) = text.get(at..).and_then(|rest| rest.find(close)) {
        let found = at + n;
        if close.starts_with('$') && found > 0 && text.as_bytes().get(found - 1) == Some(&b'\\') {
            at = found + 1;
            continue;
        }
        return found + close.len();
    }
    text.len()
}

/// The text of the mandatory argument after a command ending at `from` (optional `[…]` arguments
/// and spaces before it skipped), and where it ends; empty when there is none.
fn argument(text: &str, from: usize) -> (&str, usize) {
    let bytes = text.as_bytes();
    let mut i = from;
    loop {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'[') {
            break;
        }
        match text[i..].find(']') {
            Some(n) => i += n + 1,
            None => return ("", from),
        }
    }
    if bytes.get(i) != Some(&b'{') {
        return ("", from);
    }
    let mut depth = 0usize;
    for (n, b) in bytes.iter().enumerate().skip(i) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (&text[i + 1..n], n + 1);
                }
            }
            _ => {}
        }
    }
    ("", from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSAGE: &str =
        "As \\citet{knuth84, lamport94} show in Section~\\ref{sec:intro}, the sum $\\sum_i x_i$ is \\emph{small}\\label{eq:s}. % a note\n";

    #[test]
    fn rewording_that_keeps_the_latex_is_accepted() {
        let reworded =
            "In Section~\\ref{sec:intro}, \\citet{lamport94,knuth84} show that the sum $\\sum_i  x_i$ remains \\emph{tiny}\\label{eq:s}.\n";
        assert_eq!(compare(PASSAGE, reworded), Vec::new());
    }

    #[test]
    fn a_dropped_reference_is_named() {
        let shorter = "\\citet{knuth84, lamport94} show the sum $\\sum_i x_i$ is \\emph{small}\\label{eq:s}.";
        let changes = compare(PASSAGE, shorter);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert!(changes[0].dropped && changes[0].part == Part::Reference);
        assert!(changes[0].item.contains("\\ref{sec:intro}"), "{changes:?}");
    }

    #[test]
    fn altered_formulas_labels_citations_and_dropped_commands_are_each_refused() {
        let altered = "As \\citet{knuth84} show in Section~\\ref{sec:intro}, the sum $\\sum_j x_j$ is small\\label{eq:t}.";
        let parts: BTreeSet<(Part, bool)> = compare(PASSAGE, altered).iter().map(|c| (c.part, c.dropped)).collect();
        for expected in
            [(Part::Citation, true), (Part::Math, true), (Part::Math, false), (Part::Label, true), (Part::Label, false), (Part::Command, true)]
        {
            assert!(parts.contains(&expected), "{expected:?} not reported: {parts:?}");
        }
    }

    /// A model's answer is untrusted: half-written LaTeX never panics, and keeps what it holds.
    #[test]
    fn hostile_answers_are_compared_without_panicking() {
        let answers = ["$", "$$x", "\\", "\\(", "\\begin{equation", "\\begin{align}x", "\\ref{", "\\cite[p.~3]{", "é\\é$é", "}}}{{{", "\\label{a}}"];
        for a in answers {
            let _ = compare(PASSAGE, a);
            let _ = compare(a, PASSAGE);
        }
        assert!(compare("\\label{a}", "\\label{a}}").is_empty());
    }

    #[test]
    fn a_passage_may_grow_but_not_lose_or_alter_its_latex() {
        let bullets = "\\begin{itemize}\n\\item fast, see \\ref{sec:speed}\n\\item $O(n)$ \\cite{knuth84}\n\\end{itemize}\n";
        let fleshed = "The method is fast, as Section~\\ref{sec:speed} shows: it runs in $O(n)$ \\cite{knuth84}, \\emph{far} below the $O(n^2)$ of \\cite{lamport94}.\\label{p:fast}\n";
        // The list became prose that keeps its reference, formula and citation, and adds others.
        assert_eq!(dropped(bullets, fleshed), Vec::new());
        assert!(!compare(bullets, fleshed).is_empty(), "a rewrite would refuse it");
        // Kept as a list and grown.
        let grown = format!("{}More: $O(n^2)$ in \\cite{{lamport94}}\\label{{p:more}}, \\emph{{slower}}.\n", bullets);
        assert_eq!(dropped(bullets, &grown), Vec::new());
        // An altered formula or a dropped citation is not.
        let altered = bullets.replace("$O(n)$", "$O(n \\log n)$").replace(" \\cite{knuth84}", "");
        let parts: BTreeSet<Part> = dropped(bullets, &altered).iter().map(|c| c.part).collect();
        assert_eq!(parts, BTreeSet::from([Part::Math, Part::Citation]));
    }

    #[test]
    fn what_is_in_a_comment_or_after_an_escaped_dollar_does_not_count() {
        assert_eq!(compare("Costs \\$5 and \\$6. % see \\ref{x}", "It costs \\$5 or \\$6."), Vec::new());
    }
}
