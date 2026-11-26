//! The numbers the PDF gives headings, display formulas, theorems, figures and tables, for the
//! formatted view: counted through the whole document from its main file (the files it
//! `\input`s and `\include`s in their place), as LaTeX counts them, and taken from the last
//! build's `.aux` for whatever has a `\label` there. A number from the build also sets its
//! counter, so what follows a labelled part is numbered from it.
//!
//! What it counts: the sectioning commands (to `secnumdepth`, letters after `\appendix`), the
//! numbered math environments row by row (`\nonumber`, `\notag` and `\tag` heeded), the theorem-like
//! environments as `\newtheorem` declares them, and the captions of figures and tables. Counters
//! reset by the chapter in a class with chapters, or as `\numberwithin` says.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::auxfile::Aux;
use crate::tree::{self, Kind, Node};
use crate::view::{self, Block, BlockKind, InlineKind, TheoremKind};

/// The numbers of the parts of each file, by the first byte of their block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Numbers {
    by_block: HashMap<(String, usize), Number>,
}

/// What a block is numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Number {
    /// A heading, a theorem, a figure or a table: "2.1".
    One(String),
    /// A display formula, row by row ("(3)", or `None` for an unnumbered row).
    Rows(Vec<Option<String>>),
}

impl Numbers {
    /// The number of the block of `file` starting at `start`.
    pub fn of(&self, file: &str, start: usize) -> Option<&Number> {
        self.by_block.get(&(file.to_owned(), start))
    }

    /// The single number of a block ("2.1"), when it has one.
    pub fn one(&self, file: &str, start: usize) -> Option<&str> {
        match self.of(file, start)? {
            Number::One(n) => Some(n),
            Number::Rows(_) => None,
        }
    }
}

/// The project file `\input{name}` or `\include{name}` reads: as written and with `.tex`, from
/// the main file's folder (where TeX runs) or the project's root. `exists` says whether a project
/// path is a file.
pub fn resolve(main: &str, name: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let name = name.trim().trim_start_matches("./");
    let folder = main.rsplit_once('/').map(|(f, _)| f);
    let bases = [folder.map(|f| format!("{f}/{name}")), Some(name.to_owned())];
    for base in bases.into_iter().flatten() {
        for candidate in [format!("{base}.tex"), base.clone()] {
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// How the document numbers: from its main file's class and preamble.
struct Scheme {
    chapters: bool,
    secnumdepth: u8,
    /// Counter → the counter that resets it and prefixes its number.
    within: HashMap<String, String>,
    theorems: Vec<TheoremKind>,
}

impl Scheme {
    fn of(main: &str) -> Scheme {
        let class = crate::tree::parse(main)
            .iter()
            .find_map(|n| match &n.kind {
                Kind::Command { name, args, .. } if name == "documentclass" => {
                    args.iter().rev().find(|a| !a.optional).and_then(|a| main.get(a.inner.clone())).map(str::trim)
                }
                _ => None,
            })
            .unwrap_or("article")
            .to_owned();
        let chapters = matches!(class.as_str(), "book" | "report" | "memoir" | "scrbook" | "scrreprt" | "thesis");
        let mut within: HashMap<String, String> = HashMap::new();
        let levels = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];
        for pair in levels.windows(2).skip(1) {
            if let [parent, child] = pair
                && (chapters || *parent != "chapter")
            {
                within.insert((*child).to_owned(), (*parent).to_owned());
            }
        }
        if chapters {
            for c in ["equation", "figure", "table"] {
                within.insert(c.to_owned(), "chapter".to_owned());
            }
        }
        let mut secnumdepth = if chapters { 2 } else { 3 };
        for (command, rest) in commands(main) {
            let args: Vec<String> = braced(rest, 2);
            match (command, args.as_slice()) {
                ("numberwithin", [counter, parent]) => {
                    within.insert(counter.clone(), parent.clone());
                }
                ("setcounter", [counter, value]) if counter == "secnumdepth" => {
                    secnumdepth = value.trim().parse::<i32>().map_or(secnumdepth, |v| v.clamp(-1, 6) as u8);
                }
                _ => {}
            }
        }
        let theorems = view::theorems(main);
        for t in &theorems {
            if let Some(w) = &t.within {
                within.insert(t.counter.clone(), w.clone());
            }
        }
        Scheme { chapters, secnumdepth, within, theorems }
    }
}

/// The control words of `text` outside comments, each with the text after it.
fn commands(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|l| l.split('%').next()).flat_map(|line| {
        line.match_indices('\\').filter_map(move |(at, _)| {
            let rest = &line[at + 1..];
            let n = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
            (n > 0).then(|| (&rest[..n], &rest[n..]))
        })
    })
}

/// The first `n` `{…}` arguments at the start of `s`.
fn braced(mut s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    while out.len() < n {
        let t = s.trim_start();
        let Some(inner) = t.strip_prefix('{') else { break };
        let Some(end) = inner.find('}') else { break };
        out.push(inner[..end].trim().to_owned());
        s = &inner[end + 1..];
    }
    out
}

/// Environments numbered row by row, and those with one number.
const ROWS: &[&str] = &["align", "gather", "flalign", "alignat", "eqnarray", "xalignat", "xxalignat"];
const ONE: &[&str] = &["equation", "multline"];

/// The rows of display math (split at `\\` outside groups and inner environments).
fn rows(math: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut depth, mut envs, mut start) = (0i32, 0i32, 0usize);
    let bytes = math.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'%' => {
                i = math[i..].find('\n').map_or(bytes.len(), |n| i + n);
                continue;
            }
            b'\\' => {
                let rest = &math[i + 1..];
                if rest.starts_with("begin") {
                    envs += 1;
                } else if rest.starts_with("end") {
                    envs -= 1;
                } else if rest.starts_with('\\') && depth == 0 && envs == 0 {
                    out.push(start..i);
                    start = i + 2;
                    i += 2;
                    continue;
                }
                // Skip the escaped character (`\{`, `\\` inside a group).
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(start..math.len());
    out
}

/// The argument of `\command{…}` in `s`, if `s` has one.
fn argument_of<'s>(s: &'s str, command: &str) -> Option<&'s str> {
    let at = s.find(command)?;
    let rest = s[at + command.len()..].trim_start().strip_prefix('{')?;
    Some(&rest[..rest.find('}')?])
}

/// The counting, through the document.
struct Counting<'a> {
    scheme: Scheme,
    aux: Option<&'a Aux>,
    read: &'a dyn Fn(&str) -> Option<Arc<str>>,
    exists: &'a dyn Fn(&str) -> bool,
    main: String,
    counters: HashMap<String, u32>,
    appendix: bool,
    out: Numbers,
    /// The files being read, to stop a file that includes itself.
    open: Vec<String>,
}

/// Letters for an appendix: 1 → A, 27 → AA.
fn letters(mut n: u32) -> String {
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, char::from(b'A' + (n % 26) as u8));
        n /= 26;
    }
    s
}

fn roman(mut n: u32) -> String {
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// The counter's value a number shows ("2.3" → 3, "B" → 2), to go on from.
fn last_value(number: &str) -> Option<u32> {
    let last = number.rsplit(['.', '-']).next()?.trim();
    if let Ok(n) = last.parse() {
        return Some(n);
    }
    if !last.is_empty() && last.chars().all(|c| c.is_ascii_uppercase()) {
        return Some(last.bytes().fold(0u32, |v, b| v * 26 + u32::from(b - b'A' + 1)));
    }
    None
}

impl Counting<'_> {
    /// The counter's number as shown, with the numbers of the counters it is within.
    fn shown(&self, counter: &str) -> String {
        let value = self.counters.get(counter).copied().unwrap_or(0);
        let top = if self.scheme.chapters { "chapter" } else { "section" };
        let own = match counter {
            "part" => roman(value),
            c if c == top && self.appendix => letters(value),
            _ => value.to_string(),
        };
        match self.scheme.within.get(counter) {
            Some(parent) if parent != counter && parent != "part" => format!("{}.{own}", self.shown(parent)),
            _ => own,
        }
    }

    /// Steps `counter` and resets those within it.
    fn step(&mut self, counter: &str) {
        *self.counters.entry(counter.to_owned()).or_insert(0) += 1;
        let reset: Vec<String> = self.scheme.within.iter().filter(|(_, p)| p.as_str() == counter).map(|(c, _)| c.clone()).collect();
        for c in reset {
            self.counters.insert(c.clone(), 0);
            self.reset_within(&c, 0);
        }
    }

    fn reset_within(&mut self, counter: &str, depth: usize) {
        if depth > 8 {
            return;
        }
        let reset: Vec<String> = self.scheme.within.iter().filter(|(_, p)| p.as_str() == counter).map(|(c, _)| c.clone()).collect();
        for c in reset {
            self.counters.insert(c.clone(), 0);
            self.reset_within(&c, depth + 1);
        }
    }

    /// Steps `counter` and gives its number: the build's when `label` is in the `.aux` (which
    /// then sets the counter), the count's otherwise.
    fn number(&mut self, counter: &str, label: Option<&str>) -> String {
        self.step(counter);
        if let Some(t) = label.and_then(|k| self.aux?.label(k)) {
            if let Some(v) = last_value(&t.number) {
                self.counters.insert(counter.to_owned(), v);
            }
            return t.number.clone();
        }
        self.shown(counter)
    }

    fn file(&mut self, path: &str) {
        if self.open.len() > 16 || self.open.iter().any(|p| p == path) {
            return;
        }
        let Some(text) = (self.read)(path) else { return };
        self.open.push(path.to_owned());
        let blocks = view::view_with(&text, &self.scheme.theorems);
        self.blocks(path, &text, &blocks);
        self.open.pop();
    }

    fn blocks(&mut self, path: &str, text: &str, blocks: &[Block]) {
        for b in blocks {
            self.block(path, text, b);
        }
    }

    fn put(&mut self, path: &str, start: usize, n: Number) {
        self.out.by_block.insert((path.to_owned(), start), n);
    }

    fn block(&mut self, path: &str, text: &str, b: &Block) {
        let key = |l: &Option<view::Inline>| match l {
            Some(view::Inline { kind: InlineKind::Chip { key, .. }, .. }) => Some(key.clone()),
            _ => None,
        };
        match &b.kind {
            BlockKind::Heading { level, star, label, .. } => {
                let name = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"][usize::from((*level).min(6))];
                if *star {
                    return;
                }
                // secnumdepth's levels: part -1, chapter 0, section 1…
                let depth = i32::from(*level) - 1;
                let label = key(label);
                if depth > i32::from(self.scheme.secnumdepth) {
                    self.step(name);
                    return;
                }
                let n = self.number(name, label.as_deref());
                self.put(path, b.range.start, Number::One(n));
            }
            BlockKind::Formula { source, env: Some(env), .. } => {
                let math = text.get(source.clone()).unwrap_or_default();
                let numbered_rows = ROWS.contains(&env.as_str());
                if !numbered_rows && !ONE.contains(&env.as_str()) {
                    // Starred: only a `\tag` shows.
                    if let Some(tag) = argument_of(math, "\\tag") {
                        self.put(path, b.range.start, Number::Rows(vec![Some(format!("({tag})"))]));
                    }
                    return;
                }
                let parts = if numbered_rows { rows(math) } else { std::iter::once(0..math.len()).collect() };
                let mut shown = Vec::new();
                let count = parts.len();
                for (i, r) in parts.into_iter().enumerate() {
                    let row = math.get(r).unwrap_or_default();
                    // A last row with nothing in it (a trailing `\\`) is not set.
                    if i + 1 == count && count > 1 && row.trim().is_empty() {
                        continue;
                    }
                    if let Some(tag) = argument_of(row, "\\tag") {
                        shown.push(Some(format!("({tag})")));
                    } else if row.contains("\\nonumber") || row.contains("\\notag") {
                        shown.push(None);
                    } else {
                        let label = argument_of(row, "\\label").map(str::trim);
                        shown.push(Some(format!("({})", self.number("equation", label))));
                    }
                }
                self.put(path, b.range.start, Number::Rows(shown));
            }
            BlockKind::Theorem { kind, label, blocks, .. } => {
                if kind.numbered {
                    let n = self.number(&kind.counter.clone(), label.as_deref());
                    self.put(path, b.range.start, Number::One(n));
                }
                self.blocks(path, text, blocks);
            }
            BlockKind::Figure { caption, label, .. } => {
                if !caption.is_empty() {
                    let n = self.number("figure", key(label).as_deref());
                    self.put(path, b.range.start, Number::One(n));
                }
            }
            BlockKind::Table(t) => {
                if let Some(f) = t.float.as_ref().filter(|f| !f.caption.is_empty()) {
                    let n = self.number("table", key(&f.label).as_deref());
                    self.put(path, b.range.start, Number::One(n));
                }
            }
            BlockKind::Source | BlockKind::UnsupportedTable(_) => self.source(text, &b.range),
            BlockKind::List { items, .. } => {
                for item in items {
                    self.blocks(path, text, &item.blocks);
                }
            }
            BlockKind::Container { blocks, .. } => self.blocks(path, text, blocks),
            BlockKind::FrontMatter(fm) => self.blocks(path, text, &fm.blocks),
            BlockKind::Paragraph(inlines) => {
                if inlines.iter().any(|i| matches!(i.kind, InlineKind::Source) && text.get(i.range.clone()).is_some_and(|s| s.trim() == "\\appendix"))
                {
                    self.appendix = true;
                    let top = if self.scheme.chapters { "chapter" } else { "section" };
                    self.counters.insert(top.to_owned(), 0);
                    self.reset_within(top, 0);
                }
            }
            BlockKind::Include { name, .. } => {
                if let Some(found) = resolve(&self.main, name, self.exists) {
                    self.file(&found);
                }
            }
            BlockKind::Preamble | BlockKind::Title { .. } | BlockKind::Formula { env: None, .. } => {}
        }
    }

    /// The captions of the floats in a part shown as its source, which number as the others do.
    fn source(&mut self, text: &str, range: &Range<usize>) {
        let src = text.get(range.clone()).unwrap_or_default();
        let nodes = tree::parse(src);
        self.captions(&nodes, None);
    }

    fn captions(&mut self, nodes: &[Node], float: Option<&str>) {
        for n in nodes {
            match &n.kind {
                Kind::Environment { name, body, .. } => {
                    let float = match name.trim_end_matches('*') {
                        "figure" => Some("figure"),
                        "table" => Some("table"),
                        _ => float,
                    };
                    self.captions(body, float);
                }
                Kind::Command { name, .. } if name == "caption" => {
                    if let Some(counter) = float {
                        self.step(counter);
                    }
                }
                Kind::Group { body, .. } => self.captions(body, float),
                _ => {}
            }
        }
    }
}

/// The numbers of the document whose main file is `main`: `read` gives a project file's text,
/// `exists` whether it is one.
pub fn number(main: &str, read: &dyn Fn(&str) -> Option<Arc<str>>, exists: &dyn Fn(&str) -> bool, aux: Option<&Aux>) -> Numbers {
    let Some(text) = read(main) else { return Numbers::default() };
    let scheme = Scheme::of(&text);
    let mut c = Counting {
        scheme,
        aux,
        read,
        exists,
        main: main.to_owned(),
        counters: HashMap::new(),
        appendix: false,
        out: Numbers::default(),
        open: Vec::new(),
    };
    c.file(main);
    c.out
}

/// The theorem-like environments the main file declares, for the view of a file it includes.
pub fn declared(main: &str) -> Vec<TheoremKind> {
    view::theorems(main)
}
