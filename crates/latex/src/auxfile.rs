//! What the last build numbered, read from its `.aux` files: the number, page and title of each
//! `\label` (`\newlabel`), the kind of thing it labels (cleveref's `@cref` record or hyperref's
//! anchor), and the label each bibliography entry is cited by (`\bibcite`, plain or natbib's).
//!
//! From these, what a cross-reference or a citation shows in the PDF ([`Aux::reference`],
//! [`Aux::citation`]): the names are LaTeX's English defaults (hyperref's for `\autoref`,
//! cleveref's for `\cref` and `\Cref`, natbib's punctuation), as an English document shows them.
//!
//! (The module is not called `aux`: that is a reserved file name on Windows.)

use std::collections::HashMap;

/// What a `\newlabel` says of a label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    /// As typeset: "3", "2.1", "A".
    pub number: String,
    pub page: String,
    /// The heading or caption it is in (hyperref's third field), when there is one.
    pub title: String,
    /// What it numbers: `equation`, `section`, `figure`, `theorem`… (cleveref's type, or
    /// hyperref's anchor's first part).
    pub kind: Option<String>,
}

/// What a `\bibcite` says of an entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cited {
    /// The entry's number in the list.
    pub number: String,
    /// natbib's year and short author list (empty with plain `\bibcite{key}{3}`).
    pub year: String,
    pub authors: String,
}

/// The labels and citations of a build's `.aux` files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Aux {
    labels: HashMap<String, Target>,
    cites: HashMap<String, Cited>,
    /// The document uses biblatex, whose labels the `.aux` does not hold.
    pub biblatex: bool,
}

/// Why a reference or citation has no text from the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// No build has written an `.aux` yet.
    NotBuilt,
    /// The build has no label (or bibliography entry) with this key.
    Unknown(String),
    /// biblatex's labels are not in the `.aux`.
    Biblatex,
    /// `\nocite`: the entry is listed, nothing shows in the text.
    Silent,
}

/// How citations are set: natbib's author-year (its default) or numbers, or LaTeX's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiteStyle {
    Plain,
    NatbibAuthorYear,
    NatbibNumbers,
}

impl CiteStyle {
    /// The style of a document from its main file: natbib loaded, with `numbers` or not.
    pub fn of(main: &str) -> CiteStyle {
        let tree = crate::tree::parse(main);
        let mut style = CiteStyle::Plain;
        for n in &tree {
            if let crate::tree::Kind::Command { name, args, .. } = &n.kind
                && (name == "usepackage" || name == "RequirePackage")
            {
                let list = |optional: bool| -> Vec<&str> {
                    args.iter()
                        .filter(|a| a.optional == optional)
                        .flat_map(|a| main.get(a.inner.clone()).unwrap_or_default().split(','))
                        .map(str::trim)
                        .collect()
                };
                if list(false).contains(&"natbib") {
                    style = if list(true).contains(&"numbers") { CiteStyle::NatbibNumbers } else { CiteStyle::NatbibAuthorYear };
                }
            }
        }
        style
    }
}

/// The `{…}` group at the start of `s` (after spaces): its inside and what follows it.
fn group(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let rest = s.strip_prefix('{')?;
    let mut depth = 1usize;
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&rest[..i], &rest[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// The groups `{a}{b}{c}…` at the start of `s`.
fn groups(mut s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    while let Some((inner, rest)) = group(s) {
        out.push(inner);
        s = rest;
    }
    out
}

/// A field as it reads: `\relax`, `\ignorespaces` and braces gone, `~` as a no-break space.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '~' => out.push('\u{a0}'),
            '\\' => {
                let mut name = String::new();
                while let Some(&n) = chars.peek().filter(|n| n.is_ascii_alphabetic()) {
                    name.push(n);
                    chars.next();
                }
                if name.is_empty() {
                    // A control symbol: `\.`, `\ `, `\%` stand for their character.
                    if let Some(sym) = chars.next() {
                        out.push(sym);
                    }
                } else if chars.peek() == Some(&' ') {
                    chars.next();
                }
                // `\relax`, `\ignorespaces`, `\textup`…: their arguments stay, they go.
            }
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reads the `.aux` text of a build (several files' texts may follow one another).
pub fn parse(text: &str) -> Aux {
    let mut aux = Aux::default();
    let mut crefs: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("\\newlabel") {
            let Some((key, rest)) = group(rest) else { continue };
            let Some((fields, _)) = group(rest) else { continue };
            let fields = groups(fields);
            if let Some(key) = key.strip_suffix("@cref") {
                // `{[type][n][within]shown}`: the type in the first brackets.
                if let Some(kind) = fields.first().and_then(|f| f.strip_prefix('[')).and_then(|f| f.split_once(']')).map(|(k, _)| k) {
                    crefs.insert(key.to_owned(), kind.to_owned());
                }
                continue;
            }
            let field = |i: usize| fields.get(i).map(|f| clean(f)).unwrap_or_default();
            let kind = fields.get(3).and_then(|a| a.split('.').next()).filter(|k| !k.is_empty() && *k != "Doc-Start").map(str::to_owned);
            aux.labels.insert(key.to_owned(), Target { number: field(0), page: field(1), title: field(2), kind });
        } else if let Some(rest) = line.strip_prefix("\\bibcite") {
            let Some((key, rest)) = group(rest) else { continue };
            let Some((value, _)) = group(rest) else { continue };
            let parts = groups(value);
            let cited = if parts.len() >= 3 {
                let at = |i: usize| parts.get(i).map(|p| clean(p)).unwrap_or_default();
                Cited { number: at(0), year: at(1), authors: at(2) }
            } else {
                Cited { number: clean(value), ..Cited::default() }
            };
            aux.cites.insert(key.to_owned(), cited);
        } else if line.starts_with("\\abx@aux") {
            aux.biblatex = true;
        }
    }
    for (key, kind) in crefs {
        if let Some(t) = aux.labels.get_mut(&key) {
            t.kind = Some(kind);
        }
    }
    aux
}

/// hyperref's `\autoref` name for a kind.
fn autoref_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "figure" => "Figure",
        "table" => "Table",
        "equation" | "AMS" => "Equation",
        "part" => "Part",
        "appendix" => "Appendix",
        "chapter" => "chapter",
        "section" => "section",
        "subsection" => "subsection",
        "subsubsection" => "subsubsection",
        "paragraph" => "paragraph",
        "subparagraph" => "subparagraph",
        "Item" | "enumi" | "enumii" | "enumiii" | "enumiv" => "item",
        "footnote" | "Hfootnote" => "footnote",
        "theorem" => "Theorem",
        "page" => "page",
        _ => return None,
    })
}

/// cleveref's names for a kind: `\cref`'s singular and plural, and `\Cref`'s.
fn cref_names(kind: &str) -> Option<[&'static str; 4]> {
    Some(match kind {
        "equation" | "AMS" => ["eq.", "eqs.", "Equation", "Equations"],
        "figure" => ["fig.", "figs.", "Figure", "Figures"],
        "table" => ["table", "tables", "Table", "Tables"],
        "section" | "subsection" | "subsubsection" => ["section", "sections", "Section", "Sections"],
        "chapter" => ["chapter", "chapters", "Chapter", "Chapters"],
        "appendix" | "subappendix" => ["appendix", "appendices", "Appendix", "Appendices"],
        "part" => ["part", "parts", "Part", "Parts"],
        "paragraph" | "subparagraph" => ["paragraph", "paragraphs", "Paragraph", "Paragraphs"],
        "enumi" | "enumii" | "enumiii" | "enumiv" | "Item" => ["item", "items", "Item", "Items"],
        "footnote" => ["footnote", "footnotes", "Footnote", "Footnotes"],
        "theorem" => ["theorem", "theorems", "Theorem", "Theorems"],
        "lemma" => ["lemma", "lemmas", "Lemma", "Lemmas"],
        "corollary" => ["corollary", "corollaries", "Corollary", "Corollaries"],
        "proposition" => ["proposition", "propositions", "Proposition", "Propositions"],
        "definition" => ["definition", "definitions", "Definition", "Definitions"],
        "example" => ["example", "examples", "Example", "Examples"],
        "remark" => ["remark", "remarks", "Remark", "Remarks"],
        "note" => ["note", "notes", "Note", "Notes"],
        "algorithm" => ["algorithm", "algorithms", "Algorithm", "Algorithms"],
        "listing" => ["listing", "listings", "Listing", "Listings"],
        "line" => ["line", "lines", "Line", "Lines"],
        "page" => ["page", "pages", "Page", "Pages"],
        _ => return None,
    })
}

/// "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

impl Aux {
    pub fn label(&self, key: &str) -> Option<&Target> {
        self.labels.get(key)
    }

    pub fn cited(&self, key: &str) -> Option<&Cited> {
        self.cites.get(key)
    }

    /// What `\command{keys}` shows in the PDF, as the build numbered it.
    pub fn reference(aux: Option<&Aux>, command: &str, keys: &str) -> Result<String, Missing> {
        let aux = aux.ok_or(Missing::NotBuilt)?;
        let keys: Vec<&str> = keys.split(',').map(str::trim).filter(|k| !k.is_empty()).collect();
        let mut targets = Vec::new();
        for k in &keys {
            targets.push(aux.labels.get(*k).ok_or_else(|| Missing::Unknown((*k).to_owned()))?);
        }
        let equation = |t: &Target| matches!(t.kind.as_deref(), Some("equation" | "AMS"));
        let one = |t: &Target| -> String {
            match command {
                "eqref" => format!("({})", t.number),
                "pageref" => t.page.clone(),
                "nameref" if !t.title.is_empty() => t.title.clone(),
                "autoref" => match t.kind.as_deref().and_then(autoref_name) {
                    Some(name) => format!("{name}\u{a0}{}", t.number),
                    None => t.number.clone(),
                },
                _ => t.number.clone(),
            }
        };
        if !matches!(command, "cref" | "Cref") {
            return Ok(targets.iter().map(|t| one(t)).collect::<Vec<_>>().join(", "));
        }
        let capital = command == "Cref";
        // Kinds in order, each with its numbers: "eqs. (1) and (2) and fig. 3".
        let mut runs: Vec<(Option<&str>, Vec<String>)> = Vec::new();
        for t in &targets {
            let shown = if equation(t) { format!("({})", t.number) } else { t.number.clone() };
            match runs.last_mut() {
                Some((kind, numbers)) if *kind == t.kind.as_deref() => numbers.push(shown),
                _ => runs.push((t.kind.as_deref(), vec![shown])),
            }
        }
        let parts: Vec<String> = runs
            .iter()
            .map(|(kind, numbers)| {
                let names = kind.and_then(cref_names);
                let name = names.map(|n| n[usize::from(capital) * 2 + usize::from(numbers.len() > 1)]);
                match name {
                    Some(name) => format!("{name}\u{a0}{}", and_list(numbers)),
                    None => and_list(numbers),
                }
            })
            .collect();
        Ok(and_list(&parts))
    }

    /// What `\command{keys}` shows in the PDF: the bibliography's labels for the keys, set as
    /// `style` sets them.
    pub fn citation(aux: Option<&Aux>, style: CiteStyle, command: &str, keys: &str) -> Result<String, Missing> {
        if command == "nocite" {
            return Err(Missing::Silent);
        }
        let aux = aux.ok_or(Missing::NotBuilt)?;
        if aux.biblatex {
            return Err(Missing::Biblatex);
        }
        let mut cited = Vec::new();
        for k in keys.split(',').map(str::trim).filter(|k| !k.is_empty()) {
            cited.push(aux.cites.get(k).ok_or_else(|| Missing::Unknown(k.to_owned()))?);
        }
        let numbers = || format!("[{}]", cited.iter().map(|c| c.number.as_str()).collect::<Vec<_>>().join(", "));
        let author = |c: &Cited| if c.authors.is_empty() { c.number.clone() } else { c.authors.clone() };
        let shown = match (style, command) {
            (_, "citeauthor") => cited.iter().map(|c| author(c)).collect::<Vec<_>>().join(", "),
            (_, "citeyear") => cited.iter().map(|c| c.year.clone()).collect::<Vec<_>>().join(", "),
            (CiteStyle::Plain, _) | (CiteStyle::NatbibNumbers, "cite" | "citep") => numbers(),
            (CiteStyle::NatbibNumbers, _) => cited.iter().map(|c| format!("{} [{}]", author(c), c.number)).collect::<Vec<_>>().join(", "),
            (CiteStyle::NatbibAuthorYear, "citep") => {
                format!("[{}]", cited.iter().map(|c| format!("{}, {}", author(c), c.year)).collect::<Vec<_>>().join(", "))
            }
            (CiteStyle::NatbibAuthorYear, _) => cited.iter().map(|c| format!("{} [{}]", author(c), c.year)).collect::<Vec<_>>().join(", "),
        };
        Ok(shown)
    }
}
