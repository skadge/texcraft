//! What cross-references and citations point at, for the formatted view's pickers and chips: the
//! labels of a file with what each labels, and the entries of a `.bib` file (through retex's
//! reader) with a short author-year form.

use crate::tree::{self, Kind, Node};

/// A `\label` of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub key: String,
    /// The line, from 0.
    pub line: u32,
    /// What it labels, as it reads: the heading, caption or environment around it (empty when it
    /// cannot tell).
    pub what: String,
}

pub use retex_bib::Entry;

/// The labels of `text`, in order.
pub fn labels(text: &str) -> Vec<Label> {
    let places = retex_lsp::scan::places(text);
    let tree = tree::parse(text);
    let mut out = Vec::new();
    let mut heading = String::new();
    let line_start: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    for p in places {
        match p.kind {
            retex_lsp::scan::Kind::Section => heading = p.name.clone(),
            retex_lsp::scan::Kind::Label => {
                let at = line_start.get(p.line as usize).copied().unwrap_or(0);
                let byte = at + text.get(at..).map_or(0, |l| l.chars().take(p.start).map(char::len_utf8).sum::<usize>());
                let what = around(text, &tree, byte).unwrap_or_else(|| heading.clone());
                out.push(Label { key: p.name, line: p.line, what });
            }
            _ => {}
        }
    }
    out
}

/// What the `\label` at `byte` labels when it is in a float or a numbered environment: its
/// caption, or the environment's name.
fn around(text: &str, nodes: &[Node], byte: usize) -> Option<String> {
    for n in nodes {
        if !(n.range.start < byte && byte < n.range.end) {
            continue;
        }
        return match &n.kind {
            Kind::Environment { name, body, .. } => {
                if let Some(inner) = around(text, body, byte) {
                    return Some(inner);
                }
                if name.as_str() == "document" {
                    return None;
                }
                let caption = body.iter().find_map(|c| match &c.kind {
                    Kind::Command { name, args, .. } if name == "caption" => {
                        args.iter().rev().find(|a| !a.optional).and_then(|a| text.get(a.inner.clone()))
                    }
                    _ => None,
                });
                Some(caption.map_or_else(|| name.trim_end_matches('*').to_owned(), |c| c.split_whitespace().collect::<Vec<_>>().join(" ")))
            }
            Kind::Math { env: Some(env), .. } => Some(env.trim_end_matches('*').to_owned()),
            Kind::Group { body, .. } => around(text, body, byte),
            _ => None,
        };
    }
    None
}

/// The entries of a `.bib` file.
pub fn entries(bib: &str) -> Vec<Entry> {
    retex_bib::entries(bib)
}

/// A citation as an author-year reader sees it: "Lemaignan and Warnier (2017)", "Lemaignan et
/// al. (2017)", the family names of the authors (or the title without them).
pub fn author_year(e: &Entry) -> String {
    let names: Vec<String> = e.author.as_deref().map(|a| a.split(" and ").map(family).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
    let who = match names.as_slice() {
        [] => e.title.clone().unwrap_or_else(|| e.key.clone()),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, ..] => format!("{a} et al."),
    };
    match &e.year {
        Some(y) => format!("{who} ({y})"),
        None => who,
    }
}

/// The family name of a BibTeX name ("Lemaignan, Séverin" or "Séverin Lemaignan"), braces gone.
fn family(name: &str) -> String {
    let name = name.trim();
    if let Some(whole) = name.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
        return whole.trim().to_owned();
    }
    let family = match name.split_once(',') {
        Some((f, _)) => f,
        None => name.rsplit(' ').next().unwrap_or(name),
    };
    family.chars().filter(|c| !matches!(c, '{' | '}')).collect::<String>().trim().to_owned()
}

/// Whether the preamble of `main` loads `package` (`\usepackage{a,package}`).
pub fn loads(main: &str, package: &str) -> bool {
    tree::parse(main).iter().any(|n| match &n.kind {
        Kind::Command { name, args, .. } if name == "usepackage" || name == "RequirePackage" => {
            args.iter().filter(|a| !a.optional).any(|a| main.get(a.inner.clone()).is_some_and(|list| list.split(',').any(|p| p.trim() == package)))
        }
        _ => false,
    })
}
