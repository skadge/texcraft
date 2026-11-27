//! The view model of the formatted view (docs/ROADMAP.md): the [`tree`](crate::tree)
//! mapped to blocks and inline runs, each holding the byte range of the source it stands for.
//!
//! What the view cannot show faithfully stays as its source ([`BlockKind::Source`],
//! [`InlineKind::Source`]): nothing is hidden except comments and the commands whose whole effect
//! is shown another way (a style, a heading level, a list marker).

use std::ops::Range;

use crate::table::{self, Table, Unsupported};
use crate::tree::{self, Arg, Kind, Node};

/// How a run of text is set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Typewriter.
    pub mono: bool,
    pub sans: bool,
    pub small_caps: bool,
    /// Raised and smaller: a mark the view adds (an affiliation's number, a note's sign).
    pub sup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    pub kind: InlineKind,
    /// The source it stands for.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineKind {
    /// Text as it reads: spaces collapsed, quotes, dashes and control symbols resolved.
    Text {
        text: String,
        style: Style,
    },
    /// `\\`.
    LineBreak,
    /// A formula; `source` is its math without delimiters.
    Formula {
        source: Range<usize>,
        display: bool,
        closed: bool,
    },
    /// `\label`, `\ref`, `\cite`, `\input` and kin: `key` is the argument as written.
    Chip {
        kind: ChipKind,
        command: String,
        key: String,
    },
    Footnote(Vec<Inline>),
    Link {
        url: String,
        text: Vec<Inline>,
    },
    /// Shown as the source of its range, in the code font.
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChipKind {
    Label,
    Reference,
    Citation,
    /// `\input`, `\include`.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// The source it stands for.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// Everything before `\begin{document}`.
    Preamble,
    /// `\maketitle`, with the arguments of `\title`, `\author` and `\date` (empty when missing).
    Title {
        title: Vec<Inline>,
        author: Vec<Inline>,
        date: Vec<Inline>,
        /// Inside the braces of the last `\title`, `\author` and `\date`: where the text of one
        /// with none yet goes.
        slots: [Option<Range<usize>>; 3],
    },
    /// `\part` (level 0) to `\subparagraph` (level 6), with the `\label` that follows it.
    Heading {
        level: u8,
        star: bool,
        content: Vec<Inline>,
        label: Option<Inline>,
    },
    Paragraph(Vec<Inline>),
    List {
        kind: ListKind,
        items: Vec<Item>,
    },
    /// A title block of elsarticle's `frontmatter`: its title, authors, affiliations and notes, and
    /// the blocks in it (the abstract, the keywords).
    FrontMatter(Box<FrontMatter>),
    /// A text environment shown as its formatted body under its name: `abstract`, `quote`, `center`…
    Container {
        name: String,
        blocks: Vec<Block>,
    },
    /// A theorem-like environment (`theorem`, `lemma`, `definition`, `proof`, those of
    /// `\newtheorem`): its name as the document prints it, the optional argument's text, the key
    /// of the first `\label` in it, and its body.
    Theorem {
        kind: TheoremKind,
        title: Vec<Inline>,
        label: Option<String>,
        blocks: Vec<Block>,
    },
    /// A paragraph of nothing but `\input{name}` or `\include{name}`: the file is shown in its
    /// place. `name` is as written.
    Include {
        command: String,
        name: String,
    },
    /// Display math; `source` is the math without its delimiters.
    Formula {
        source: Range<usize>,
        env: Option<String>,
        closed: bool,
    },
    /// A `figure` holding only an image, a caption, a label and `\centering`.
    Figure {
        image: Option<Image>,
        caption: Vec<Inline>,
        label: Option<Inline>,
    },
    /// A `tabular` shown as a grid, alone or in a `table` float with its caption and label.
    Table(Box<TableBlock>),
    /// A `tabular` (or a `table` float holding one) the grid cannot show faithfully: shown as its
    /// source, with why.
    UnsupportedTable(Unsupported),
    /// Shown as the source of its range, in the code font.
    Source,
}

/// elsarticle's `frontmatter`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    pub title: Vec<Inline>,
    /// The keys of the notes on the title (`\tnoteref`).
    pub title_notes: Vec<String>,
    pub authors: Vec<Author>,
    /// `\affiliation` and `\address`, in order: the authors' marks number them so.
    pub affiliations: Vec<Affiliation>,
    /// `\tnotetext`, `\cortext`, `\fntext`, in order.
    pub notes: Vec<Note>,
    /// What else it holds: the abstract, the keywords, and what the view shows as its source.
    pub blocks: Vec<Block>,
}

/// An `\author` of a front matter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Author {
    pub name: Vec<Inline>,
    /// The keys of its affiliations (`\author[a,b]`).
    pub affiliations: Vec<String>,
    /// The keys of its notes (`\corref`, `\fnref`).
    pub notes: Vec<String>,
    /// Its `\ead`s, each as its text.
    pub emails: Vec<Vec<Inline>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Affiliation {
    /// As written in its optional argument; empty without one.
    pub key: String,
    /// Its text; for elsarticle's `organization={…}, city={…}` form, the values, `, ` between
    /// them (text standing for no source).
    pub text: Vec<Inline>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub key: String,
    pub kind: NoteKind,
    pub text: Vec<Inline>,
}

/// What a front matter's note is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteKind {
    /// `\tnotetext`: the title.
    Title,
    /// `\cortext`: the corresponding author.
    Corresponding,
    /// `\fntext`: an author.
    Author,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub table: Table,
    /// The inlines of each row's cells.
    pub cells: Vec<Vec<Vec<Inline>>>,
    /// In a `table` float: its caption (empty when it has none), whether the caption comes before
    /// the table, and its label.
    pub float: Option<Float>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Float {
    pub caption: Vec<Inline>,
    pub caption_above: bool,
    pub label: Option<Inline>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// As written in `\includegraphics`, often without its extension.
    pub path: String,
    /// The optional argument (`width=…`), as written.
    pub options: String,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
    Bullets,
    Numbers,
    Descriptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `\item[…]`'s argument.
    pub label: Option<Vec<Inline>>,
    pub blocks: Vec<Block>,
    /// From `\item` to the next one or the list's end.
    pub range: Range<usize>,
}

/// Text environments shown formatted, under their name.
pub(crate) const CONTAINERS: &[&str] = &["abstract", "keyword", "quote", "quotation", "verse", "center", "flushleft", "flushright"];

/// A theorem-like environment: how the document prints its name and numbers it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TheoremKind {
    pub env: String,
    /// As printed: "Theorem", "Lemma", "Proof".
    pub name: String,
    /// `\newtheorem*` and `proof` are not numbered.
    pub numbered: bool,
    /// The counter it numbers with (its own, or the one named in `\newtheorem{lemma}[theorem]…`).
    pub counter: String,
    /// The counter that resets it and prefixes its number (`\newtheorem{theorem}{Theorem}[section]`).
    pub within: Option<String>,
}

/// The theorem-like environments a document has without declaring them: amsthm's `proof`, and the
/// usual names (declared in a package or a file the view does not read), each numbered by itself.
const USUAL_THEOREMS: &[&str] = &[
    "theorem",
    "lemma",
    "proposition",
    "corollary",
    "definition",
    "example",
    "remark",
    "conjecture",
    "claim",
    "exercise",
    "problem",
    "assumption",
    "observation",
    "notation",
    "hypothesis",
    "fact",
    "question",
];

/// The theorem-like environments `\newtheorem` declares in `text`, in order.
pub fn theorems(text: &str) -> Vec<TheoremKind> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("\\newtheorem") {
        rest = &rest[at + "\\newtheorem".len()..];
        // The rest of the line holds the declaration; a comment before it does not count.
        let line_start = text.len() - rest.len() - "\\newtheorem".len();
        let line = text[..line_start].rsplit('\n').next().unwrap_or_default();
        if line.contains('%') {
            continue;
        }
        if rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let (star, mut s) = match rest.strip_prefix('*') {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let mut take = |open: char, close: char| -> Option<String> {
            let t = s.trim_start();
            let inner = t.strip_prefix(open)?;
            let end = inner.find(close)?;
            s = &inner[end + 1..];
            Some(inner[..end].trim().to_owned())
        };
        let Some(env) = take('{', '}') else { continue };
        let shared = take('[', ']');
        let Some(name) = take('{', '}') else { continue };
        let within = if shared.is_none() { take('[', ']') } else { None };
        let name: String = name.chars().filter(|c| !matches!(c, '{' | '}')).collect();
        let counter = shared.clone().unwrap_or_else(|| env.clone());
        // A shared counter is reset as the theorem that owns it is.
        let within = within.or_else(|| out.iter().find(|k: &&TheoremKind| Some(&k.env) == shared.as_ref()).and_then(|k| k.within.clone()));
        out.push(TheoremKind { env, name, numbered: !star, counter, within });
    }
    out
}

/// How `env` is shown when it is theorem-like: declared in `declared`, or one of the usual names.
pub fn theorem_kind(env: &str, declared: &[TheoremKind]) -> Option<TheoremKind> {
    if let Some(k) = declared.iter().find(|k| k.env == env) {
        return Some(k.clone());
    }
    let capital = |s: &str| {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    };
    match env {
        "proof" => Some(TheoremKind { env: env.to_owned(), name: "Proof".to_owned(), numbered: false, counter: String::new(), within: None }),
        _ if USUAL_THEOREMS.contains(&env) => {
            Some(TheoremKind { env: env.to_owned(), name: capital(env), numbered: true, counter: env.to_owned(), within: None })
        }
        _ => None,
    }
}

/// Whether the body of `env` is shown as formatted text (a container or a theorem) in `text`.
pub(crate) fn shown_as_text(text: &str, env: &str) -> bool {
    CONTAINERS.contains(&env) || theorem_kind(env, &theorems(text)).is_some()
}

/// The view of `text`.
pub fn view(text: &str) -> Vec<Block> {
    build(text, &tree::parse(text))
}

/// The view of `text`, with the theorem-like environments another file declares (the main
/// file's preamble, for a file it includes).
pub fn view_with(text: &str, declared: &[TheoremKind]) -> Vec<Block> {
    build_with(text, &tree::parse(text), declared)
}

/// The view of `text` from its tree.
pub fn build(text: &str, tree: &[Node]) -> Vec<Block> {
    build_with(text, tree, &[])
}

fn build_with(text: &str, tree: &[Node], declared: &[TheoremKind]) -> Vec<Block> {
    let mut known = theorems(text);
    known.extend(declared.iter().filter(|d| !known.iter().any(|k| k.env == d.env)).cloned().collect::<Vec<_>>());
    let document = tree.iter().position(|n| matches!(&n.kind, Kind::Environment { name, .. } if name == "document"));
    let Some((at, Some(Node { kind: Kind::Environment { body, .. }, range }))) = document.map(|i| (i, tree.get(i))) else {
        return View::new(text, tree, &[], known).flow(tree);
    };
    let view = View::new(text, tree, body, known);
    let mut blocks = Vec::new();
    if !text[..range.start].trim().is_empty() {
        blocks.push(Block { kind: BlockKind::Preamble, range: 0..range.start });
    }
    blocks.extend(view.flow(body));
    let after = tree.get(at + 1..).unwrap_or_default();
    if let (Some(first), Some(last)) = (after.first(), after.last())
        && !text[first.range.start..last.range.end].trim().is_empty()
    {
        blocks.push(Block { kind: BlockKind::Source, range: first.range.start..last.range.end });
    }
    blocks
}

struct View<'a> {
    text: &'a str,
    /// The arguments of `\title`, `\author` and `\date`: the last one of each, and of `\author`
    /// all of them (classes such as revtex, amsart and acmart take an `\author` per author).
    titles: [Vec<&'a Arg>; 3],
    /// The theorem-like environments declared.
    theorems: Vec<TheoremKind>,
}

/// The last required argument.
fn required(args: &[Arg]) -> Option<&Arg> {
    args.iter().rev().find(|a| !a.optional)
}

/// The level of a sectioning command.
pub fn heading_level(name: &str) -> Option<u8> {
    let level = match name {
        "part" => 0,
        "chapter" => 1,
        "section" => 2,
        "subsection" => 3,
        "subsubsection" => 4,
        "paragraph" => 5,
        "subparagraph" => 6,
        _ => return None,
    };
    Some(level)
}

/// The text of the heading whose source is `text[range]`: inside the braces of its argument.
pub fn heading_text(text: &str, range: &Range<usize>) -> Option<Range<usize>> {
    let nodes = tree::parse(text.get(range.clone())?);
    match &nodes.first()?.kind {
        Kind::Command { args, .. } => {
            let inner = &required(args)?.inner;
            Some(range.start + inner.start..range.start + inner.end)
        }
        _ => None,
    }
}

/// Where the text of a heading goes (where the caret stands in an empty one).
pub fn heading_slot(text: &str, range: &Range<usize>) -> Option<usize> {
    heading_text(text, range).map(|r| r.start)
}

/// Where the body of the environment whose source is `text[range]` starts: after its `\begin`
/// and its arguments (where the caret stands in an empty theorem or quote).
pub fn body_start(text: &str, range: &Range<usize>) -> Option<usize> {
    let nodes = tree::parse(text.get(range.clone())?);
    match &nodes.first()?.kind {
        Kind::Environment { begin, .. } => Some(range.start + begin.end),
        _ => None,
    }
}

/// Calls `f` with each block of `blocks` and of the blocks inside them, in reading order.
pub fn walk<'b>(blocks: &'b [Block], f: &mut dyn FnMut(&'b Block)) {
    for b in blocks {
        f(b);
        match &b.kind {
            BlockKind::List { items, .. } => items.iter().for_each(|i| walk(&i.blocks, f)),
            BlockKind::Container { blocks, .. } | BlockKind::Theorem { blocks, .. } => walk(blocks, f),
            BlockKind::FrontMatter(fm) => walk(&fm.blocks, f),
            _ => {}
        }
    }
}

/// The inline runs a block shows itself (not those of the blocks inside it), footnotes' and
/// links' included.
pub fn inlines_of(b: &Block) -> Vec<&Inline> {
    fn add<'i>(inlines: &'i [Inline], out: &mut Vec<&'i Inline>) {
        for i in inlines {
            out.push(i);
            if let InlineKind::Footnote(inner) | InlineKind::Link { text: inner, .. } = &i.kind {
                add(inner, out);
            }
        }
    }
    let mut out = Vec::new();
    match &b.kind {
        BlockKind::Title { title, author, date, .. } => [title, author, date].into_iter().for_each(|i| add(i, &mut out)),
        BlockKind::Heading { content, label, .. } => {
            add(content, &mut out);
            out.extend(label.iter());
        }
        BlockKind::Paragraph(inlines) => add(inlines, &mut out),
        BlockKind::FrontMatter(fm) => {
            add(&fm.title, &mut out);
            for a in &fm.authors {
                add(&a.name, &mut out);
                a.emails.iter().for_each(|e| add(e, &mut out));
            }
            fm.affiliations.iter().for_each(|a| add(&a.text, &mut out));
            fm.notes.iter().for_each(|n| add(&n.text, &mut out));
        }
        BlockKind::List { items, .. } => items.iter().filter_map(|i| i.label.as_ref()).for_each(|l| add(l, &mut out)),
        BlockKind::Theorem { title, .. } => add(title, &mut out),
        BlockKind::Figure { caption, label, .. } => {
            add(caption, &mut out);
            out.extend(label.iter());
        }
        BlockKind::Table(t) => {
            t.cells.iter().flatten().for_each(|c| add(c, &mut out));
            if let Some(f) = &t.float {
                add(&f.caption, &mut out);
                out.extend(f.label.iter());
            }
        }
        BlockKind::Preamble
        | BlockKind::Container { .. }
        | BlockKind::Formula { .. }
        | BlockKind::UnsupportedTable(_)
        | BlockKind::Source
        | BlockKind::Include { .. } => {}
    }
    out
}

/// Whether `\name` cites (rather than refers to a label).
pub fn is_citation(name: &str) -> bool {
    chip_kind(name) == Some(ChipKind::Citation)
}

fn chip_kind(name: &str) -> Option<ChipKind> {
    let kind = match name {
        "label" => ChipKind::Label,
        "ref" | "eqref" | "pageref" | "autoref" | "nameref" | "cref" | "Cref" => ChipKind::Reference,
        "cite" | "citep" | "citet" | "parencite" | "textcite" | "autocite" | "footcite" | "nocite" | "citeauthor" | "citeyear" => ChipKind::Citation,
        "input" | "include" => ChipKind::File,
        _ => return None,
    };
    Some(kind)
}

/// The style of a `\textbf{…}`-like command's argument, from the style around it.
pub(crate) fn styled(name: &str, s: Style) -> Option<Style> {
    let style = match name {
        "textbf" => Style { bold: true, ..s },
        "textit" | "textsl" => Style { italic: true, ..s },
        "emph" => Style { italic: !s.italic, ..s },
        "underline" => Style { underline: true, ..s },
        "texttt" => Style { mono: true, sans: false, ..s },
        "textsf" => Style { sans: true, mono: false, ..s },
        "textrm" => Style { sans: false, mono: false, ..s },
        "textsc" => Style { small_caps: true, ..s },
        "textup" => Style { italic: false, ..s },
        "textmd" => Style { bold: false, ..s },
        "textnormal" => Style::default(),
        "mbox" => s,
        _ => return None,
    };
    Some(style)
}

/// The style after a declaration such as `\bfseries`, which lasts to the end of its group.
pub(crate) fn declared(name: &str, s: Style) -> Option<Style> {
    let style = match name {
        "bfseries" | "bf" => Style { bold: true, ..s },
        "itshape" | "it" | "slshape" | "sl" => Style { italic: true, ..s },
        "em" => Style { italic: !s.italic, ..s },
        "ttfamily" | "tt" => Style { mono: true, sans: false, ..s },
        "sffamily" | "sf" => Style { sans: true, mono: false, ..s },
        "rmfamily" | "rm" => Style { sans: false, mono: false, ..s },
        "scshape" | "sc" => Style { small_caps: true, ..s },
        "upshape" => Style { italic: false, ..s },
        "mdseries" => Style { bold: false, ..s },
        "normalfont" => Style::default(),
        _ => return None,
    };
    Some(style)
}

/// Commands that stand for a fixed text.
pub(crate) fn logo(name: &str) -> Option<&'static str> {
    Some(match name {
        "LaTeX" => "LaTeX",
        "TeX" => "TeX",
        "LaTeXe" => "LaTeX2ε",
        "ldots" | "dots" | "textellipsis" => "…",
        // elsarticle's keyword separator.
        "sep" => "·",
        "textbackslash" => "\\",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "textendash" => "–",
        "textemdash" => "—",
        _ => return None,
    })
}

/// The letters an accent command makes of the letters it takes: `(accent, bases, accented)`,
/// the two strings in step.
const ACCENTS: &[(&str, &str, &str)] = &[
    ("`", "AEIOUaeiou\u{131}", "ÀÈÌÒÙàèìòùì"),
    ("'", "AEIOUYaeiouyCcNnSsZzLlRrGg\u{131}", "ÁÉÍÓÚÝáéíóúýĆćŃńŚśŹźĹĺŔŕǴǵí"),
    ("^", "AEIOUaeiouCcGgHhJjSsWwYy\u{131}", "ÂÊÎÔÛâêîôûĈĉĜĝĤĥĴĵŜŝŴŵŶŷî"),
    ("\"", "AEIOUaeiouyY\u{131}", "ÄËÏÖÜäëïöüÿŸï"),
    ("~", "ANOanoIiUu", "ÃÑÕãñõĨĩŨũ"),
    ("=", "AEIOUaeiou", "ĀĒĪŌŪāēīōū"),
    (".", "CEGIZcegz", "ĊĖĠİŻċėġż"),
    ("c", "CcSsTtGgKkLlNnRr", "ÇçŞşŢţĢģĶķĻļŅņŖŗ"),
    ("v", "CcDdEeNnRrSsTtZz", "ČčĎďĚěŇňŘřŠšŤťŽž"),
    ("u", "AaGgUuEeIiOo", "ĂăĞğŬŭĔĕĬĭŎŏ"),
    ("H", "OoUu", "ŐőŰű"),
    ("k", "AaEeIiUu", "ĄąĘęĮįŲų"),
    ("r", "AaUu", "ÅåŮů"),
];

/// The accented letter that `src` starts with as an accent command (`\'e`, `\'{e}`, `\"{\i}`,
/// `\c{c}`, `\c c`), and how many bytes of `src` it takes; `None` when `src` starts with none
/// the view knows.
pub(crate) fn accented(src: &str) -> Option<(char, usize)> {
    let after = src.strip_prefix('\\')?;
    let name_len = match after.chars().next()? {
        c if c.is_ascii_alphabetic() => after.len() - after.trim_start_matches(|c: char| c.is_ascii_alphabetic()).len(),
        c => c.len_utf8(),
    };
    let name = after.get(..name_len)?;
    let (_, bases, accented) = ACCENTS.iter().find(|(a, _, _)| *a == name)?;
    let rest = after.get(name_len..)?;
    let letter_named = name.chars().all(|c| c.is_ascii_alphabetic());
    // The letter it takes: `{e}` or `{\i}`; after a symbol, `e` itself; after a letter's name,
    // one space then `e`.
    let (base, taken) = if let Some(group) = rest.strip_prefix('{') {
        let close = group.find('}')?;
        let inner = group.get(..close)?.trim();
        let base = if inner == "\\i" { '\u{131}' } else { single(inner)? };
        (base, close + 2)
    } else if letter_named {
        let spaced = rest.strip_prefix(' ')?;
        (spaced.chars().next().filter(char::is_ascii_alphabetic)?, 2)
    } else {
        (rest.chars().next().filter(char::is_ascii_alphabetic)?, 1)
    };
    let at = bases.chars().position(|b| b == base)?;
    Some((accented.chars().nth(at)?, 1 + name_len + taken))
}

/// The one character of `s`, if it has exactly one.
fn single(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// What a control symbol shows; `None` for those shown as their source.
pub(crate) fn control_symbol(symbol: &str) -> Option<&'static str> {
    Some(match symbol {
        "\\%" => "%",
        "\\&" => "&",
        "\\$" => "$",
        "\\#" => "#",
        "\\_" => "_",
        "\\{" => "{",
        "\\}" => "}",
        "\\ " | "\\\n" | "\\\t" => " ",
        "\\," => "\u{2009}",
        // Invisible in print: a discretionary hyphen, an italic correction, a spacing hint.
        "\\-" | "\\/" | "\\@" => "",
        _ => return None,
    })
}

/// Text as it reads: runs of white space as one space, ``` `` ''``` and `` ` ' `` as curly quotes,
/// `--` and `---` as en and em dashes.
fn typography(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut space = false;
    while let Some(c) = chars.next() {
        // A no-break space is a character of its own, not a space TeX collapses.
        if c.is_ascii_whitespace() {
            space = true;
            continue;
        }
        if std::mem::take(&mut space) {
            out.push(' ');
        }
        let mut next_is = |want: char| chars.next_if_eq(&want).is_some();
        match c {
            '-' if next_is('-') => out.push(if next_is('-') { '—' } else { '–' }),
            '`' => out.push(if next_is('`') { '“' } else { '‘' }),
            '\'' => out.push(if next_is('\'') { '”' } else { '’' }),
            _ => out.push(c),
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// Appends text, merging it with the text before when they touch and share a style, and keeping
/// one space where two runs meet.
fn push_text(out: &mut Vec<Inline>, text: &str, style: Style, range: Range<usize>) {
    let mut text = text;
    if let Some(Inline { kind: InlineKind::Text { text: last, style: last_style }, range: last_range }) = out.last_mut() {
        if last.ends_with(' ') {
            text = text.strip_prefix(' ').unwrap_or(text);
        }
        if *last_style == style && last_range.end == range.start {
            last.push_str(text);
            last_range.end = range.end;
            return;
        }
    }
    if !text.is_empty() {
        out.push(Inline { kind: InlineKind::Text { text: text.to_owned(), style }, range });
    }
}

/// Whether the inlines show nothing but white space.
fn blank(inlines: &[Inline]) -> bool {
    inlines.iter().all(|i| matches!(&i.kind, InlineKind::Text { text, .. } if text.trim().is_empty()))
}

/// Whether a node shows nothing: white space, a comment, a paragraph break.
fn invisible(text: &str, n: &Node) -> bool {
    match n.kind {
        Kind::Text => text[n.range.clone()].trim().is_empty(),
        Kind::Comment | Kind::ParBreak => true,
        _ => false,
    }
}

/// The blocks and the paragraph being gathered.
#[derive(Default)]
struct Flow {
    blocks: Vec<Block>,
    para: Vec<Inline>,
    style: Style,
    /// The commands and groups the paragraph's inlines come from: its range covers them whole, so
    /// that what wraps the paragraph (a heading, a list) wraps them, not their insides.
    wraps: Option<Range<usize>>,
}

impl Flow {
    fn flush(&mut self, text: &str) {
        let para = std::mem::take(&mut self.para);
        let wraps = self.wraps.take();
        // An `\input` or `\include` on a line of its own: the file in its place, the paragraph's
        // text before and after it apart.
        let alone = |r: &Range<usize>| {
            let before = text.get(..r.start).unwrap_or_default();
            let after = text.get(r.end..).unwrap_or_default();
            let line_before = before.rsplit('\n').next().unwrap_or_default();
            let line_after = after.split('\n').next().unwrap_or_default();
            line_before.trim().is_empty() && (line_after.trim().is_empty() || line_after.trim_start().starts_with('%'))
        };
        let split = para.iter().any(|i| matches!(&i.kind, InlineKind::Chip { kind: ChipKind::File, .. }) && alone(&i.range));
        let mut part = Vec::new();
        for i in para {
            if matches!(&i.kind, InlineKind::Chip { kind: ChipKind::File, .. }) && alone(&i.range) {
                self.paragraph(std::mem::take(&mut part), None);
                if let InlineKind::Chip { command, key, .. } = i.kind {
                    self.blocks.push(Block { kind: BlockKind::Include { command, name: key }, range: i.range });
                }
            } else {
                part.push(i);
            }
        }
        self.paragraph(part, wraps.filter(|_| !split));
    }

    fn paragraph(&mut self, mut para: Vec<Inline>, wraps: Option<Range<usize>>) {
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = para.first_mut() {
            *text = text.trim_ascii_start().to_owned();
        }
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = para.last_mut() {
            *text = text.trim_ascii_end().to_owned();
        }
        para.retain(|i| !matches!(&i.kind, InlineKind::Text { text, .. } if text.is_empty()));
        if let (Some(first), Some(last)) = (para.first(), para.last()) {
            let mut range = first.range.start..last.range.end;
            if let Some(w) = wraps {
                range = range.start.min(w.start)..range.end.max(w.end);
            }
            self.blocks.push(Block { kind: BlockKind::Paragraph(para), range });
        }
    }

    /// Gives `label` to the block just before, if nothing shows between them and it has none.
    fn attach_label(&mut self, label: &Inline) -> bool {
        if !blank(&self.para) {
            return false;
        }
        let slot = match self.blocks.last_mut().map(|b| &mut b.kind) {
            Some(BlockKind::Heading { label: slot @ None, .. } | BlockKind::Figure { label: slot @ None, .. }) => slot,
            _ => return false,
        };
        *slot = Some(label.clone());
        self.para.clear();
        true
    }
}

impl<'a> View<'a> {
    fn new(text: &'a str, top: &'a [Node], body: &'a [Node], theorems: Vec<TheoremKind>) -> Self {
        let mut titles: [Vec<&'a Arg>; 3] = Default::default();
        for n in top.iter().chain(body) {
            if let Kind::Command { name, args, .. } = &n.kind
                && let Some(slot) = ["title", "author", "date"].iter().position(|t| t == name)
                && let Some(arg) = required(args)
                && let Some(kept) = titles.get_mut(slot)
            {
                if slot != 1 {
                    kept.clear();
                }
                kept.push(arg);
            }
        }
        View { text, titles, theorems }
    }

    fn src(&self, range: &Range<usize>) -> &'a str {
        self.text.get(range.clone()).unwrap_or_default()
    }

    /// The blocks of a sequence of nodes.
    fn flow(&self, nodes: &[Node]) -> Vec<Block> {
        let mut f = Flow::default();
        let mut i = 0;
        while let Some(n) = nodes.get(i) {
            if n.kind == Kind::ParBreak {
                f.flush(self.text);
                i += 1;
                continue;
            }
            if let Some(block) = self.block(n, f.style) {
                f.flush(self.text);
                f.blocks.push(block);
                i += 1;
                continue;
            }
            let mut inlines = Vec::new();
            i = self.inline_at(nodes, i, &mut f.style, &mut inlines);
            if let [label @ Inline { kind: InlineKind::Chip { kind: ChipKind::Label, .. }, .. }] = inlines.as_slice()
                && f.attach_label(label)
            {
                continue;
            }
            if !inlines.is_empty() && matches!(n.kind, Kind::Command { .. } | Kind::Group { .. }) {
                f.wraps = Some(f.wraps.take().map_or(n.range.clone(), |w| w.start.min(n.range.start)..w.end.max(n.range.end)));
            }
            for inline in inlines {
                match inline.kind {
                    InlineKind::Text { text, style } => push_text(&mut f.para, &text, style, inline.range),
                    _ => f.para.push(inline),
                }
            }
        }
        f.flush(self.text);
        f.blocks
    }

    /// The block a node makes, if it is one.
    fn block(&self, n: &Node, style: Style) -> Option<Block> {
        let range = n.range.clone();
        let kind = match &n.kind {
            Kind::Command { name, star, args } => {
                if name == "maketitle" {
                    self.title()
                } else {
                    let level = heading_level(name)?;
                    let arg = required(args)?;
                    BlockKind::Heading { level, star: *star, content: self.inlines(&arg.body, style), label: None }
                }
            }
            Kind::Math { display: true, env, body, closed } => BlockKind::Formula { source: body.clone(), env: env.clone(), closed: *closed },
            Kind::Verbatim { .. } if self.src(&range).starts_with("\\begin") => BlockKind::Source,
            Kind::Environment { name, .. } if table::GRID_ENVIRONMENTS.contains(&name.as_str()) => self.grid(n, None),
            Kind::Environment { name, args, body, end, .. } => match theorem_kind(name, &self.theorems) {
                Some(kind) if end.is_some() => self.theorem(kind, args, body),
                _ => self.environment(name, body, end.is_some()),
            },
            _ => return None,
        };
        Some(Block { kind, range })
    }

    /// A theorem-like environment: its optional argument (parsed, or the `[…]` its body starts
    /// with when the parser does not know the environment), its first label and its body.
    fn theorem(&self, kind: TheoremKind, args: &[Arg], body: &[Node]) -> BlockKind {
        let mut body = body;
        let mut title = args.iter().find(|a| a.optional).map(|a| self.inlines(&a.body, Style::default()));
        let mut rest: Vec<Node> = Vec::new();
        if title.is_none()
            && let Some(Node { kind: Kind::Text, range }) = body.first()
        {
            let src = self.src(range);
            let lead = src.len() - src.trim_start().len();
            if src.trim_start().starts_with('[')
                && let Some(close) = src.find(']')
            {
                let inner = range.start + lead + 1..range.start + close;
                title = Some(vec![text_inline(typography(self.src(&inner)).trim(), Style::default(), inner)]);
                rest.push(Node { kind: Kind::Text, range: range.start + close + 1..range.end });
                rest.extend(body.iter().skip(1).cloned());
            }
        }
        if !rest.is_empty() {
            body = &rest;
        }
        let label = first_label(self.text, body);
        BlockKind::Theorem { kind, title: title.unwrap_or_default(), label, blocks: self.flow(body) }
    }

    /// A `tabular` node as a grid, or why it stays as source.
    fn grid(&self, n: &Node, float: Option<Float>) -> BlockKind {
        let Kind::Environment { body, .. } = &n.kind else { return BlockKind::Source };
        match table::parse(self.text, n) {
            Ok(table) => {
                let cells = table
                    .rows
                    .iter()
                    .map(|row| row.cells.iter().map(|c| self.inlines(&clipped(body, &c.content), Style::default())).collect())
                    .collect();
                BlockKind::Table(Box::new(TableBlock { table, cells, float }))
            }
            Err(why) => BlockKind::UnsupportedTable(why),
        }
    }

    /// A `table` float holding one `tabular`, with its caption and label and `\centering`; `None`
    /// when it holds more than that.
    fn table_float(&self, body: &[Node]) -> Option<BlockKind> {
        let (mut grid, mut caption, mut label, mut caption_above) = (None, None, None, false);
        for n in body {
            if invisible(self.text, n) {
                continue;
            }
            match &n.kind {
                Kind::Environment { name, .. } if table::GRID_ENVIRONMENTS.contains(&name.as_str()) && grid.is_none() => grid = Some(n),
                Kind::Command { name, args, .. } => match (name.as_str(), required(args)) {
                    ("centering", _) => {}
                    ("caption", Some(arg)) if caption.is_none() => {
                        caption_above = grid.is_none();
                        caption = Some(self.inlines(&arg.body, Style::default()));
                    }
                    ("label", Some(_)) if label.is_none() => label = self.chip(n, name, args),
                    _ => return None,
                },
                _ => return None,
            }
        }
        let float = Float { caption: caption.unwrap_or_default(), caption_above, label };
        Some(self.grid(grid?, Some(float)))
    }

    fn title(&self) -> BlockKind {
        // Each `\author` on a line of its own, as `\and` puts them: the break stands for the source
        // from one's closing brace to the next one's opening brace, so deleting it joins the two.
        let [title, author, date] = self.titles.each_ref().map(|args| {
            let mut out = Vec::new();
            for (k, arg) in args.iter().enumerate() {
                if k > 0
                    && let Some(before) = args.get(k - 1)
                {
                    out.push(Inline { kind: InlineKind::LineBreak, range: before.inner.end..arg.inner.start });
                }
                out.extend(self.inlines(&arg.body, Style::default()));
            }
            out
        });
        let slots = self.titles.each_ref().map(|args| args.last().map(|a| a.inner.clone()));
        BlockKind::Title { title, author, date, slots }
    }

    fn environment(&self, name: &str, body: &[Node], closed: bool) -> BlockKind {
        let shown = match name {
            "itemize" => self.list(ListKind::Bullets, body),
            "enumerate" => self.list(ListKind::Numbers, body),
            "description" => self.list(ListKind::Descriptions, body),
            "figure" | "figure*" if closed => self.figure(body),
            "table" | "table*" if closed => self.table_float(body),
            "frontmatter" if closed => Some(BlockKind::FrontMatter(Box::new(self.front_matter(body)))),
            _ if CONTAINERS.contains(&name) => Some(BlockKind::Container { name: name.to_owned(), blocks: self.flow(body) }),
            _ => None,
        };
        shown.unwrap_or(BlockKind::Source)
    }

    /// elsarticle's front matter: its commands gathered, the rest (the abstract, the keywords) as
    /// blocks.
    fn front_matter(&self, body: &[Node]) -> FrontMatter {
        let mut fm = FrontMatter::default();
        let mut rest = Vec::new();
        let key = |args: &[Arg]| args.iter().find(|a| a.optional).map(|a| self.src(&a.inner).trim().to_owned()).unwrap_or_default();
        for n in body {
            let Kind::Command { name, args, .. } = &n.kind else {
                rest.push(n.clone());
                continue;
            };
            let Some(arg) = required(args) else {
                rest.push(n.clone());
                continue;
            };
            match name.as_str() {
                "title" => (fm.title, fm.title_notes) = self.marked(&arg.body),
                "author" => {
                    let (name, notes) = self.marked(&arg.body);
                    let affiliations = key(args).split(',').map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()).collect();
                    fm.authors.push(Author { name, affiliations, notes, emails: Vec::new() });
                }
                "ead" => match fm.authors.last_mut() {
                    Some(a) => a.emails.push(self.inlines(&arg.body, Style::default())),
                    None => rest.push(n.clone()),
                },
                "affiliation" | "address" => fm.affiliations.push(Affiliation { key: key(args), text: self.affiliation(arg) }),
                "tnotetext" | "cortext" | "fntext" => {
                    let kind = match name.as_str() {
                        "tnotetext" => NoteKind::Title,
                        "cortext" => NoteKind::Corresponding,
                        _ => NoteKind::Author,
                    };
                    fm.notes.push(Note { key: key(args), kind, text: self.inlines(&arg.body, Style::default()) });
                }
                _ => rest.push(n.clone()),
            }
        }
        fm.blocks = self.flow(&rest);
        fm
    }

    /// The inlines of an argument without the note marks in it (`\tnoteref`, `\corref`,
    /// `\fnref`), and those marks' keys.
    fn marked(&self, nodes: &[Node]) -> (Vec<Inline>, Vec<String>) {
        let mut keys = Vec::new();
        let mut kept = Vec::new();
        for n in nodes {
            match &n.kind {
                Kind::Command { name, args, .. } if matches!(name.as_str(), "tnoteref" | "corref" | "fnref") => {
                    if let Some(a) = required(args) {
                        keys.extend(self.src(&a.inner).split(',').map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()));
                    }
                }
                _ => kept.push(n.clone()),
            }
        }
        (self.inlines(&kept, Style::default()), keys)
    }

    /// An affiliation's text: elsarticle's `organization={…}, city={…}` as its values, `, `
    /// between them; otherwise its argument.
    fn affiliation(&self, arg: &Arg) -> Vec<Inline> {
        let keyed = arg.body.iter().any(|n| n.kind == Kind::Text && self.src(&n.range).contains('='));
        if !keyed {
            return self.inlines(&arg.body, Style::default());
        }
        let mut out = Vec::new();
        for n in &arg.body {
            let Kind::Group { body, .. } = &n.kind else { continue };
            let value = self.inlines(body, Style::default());
            if value.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(text_inline(", ", Style::default(), n.range.start..n.range.start));
            }
            out.extend(value);
        }
        out
    }

    /// A list, or `None` when something shows before its first `\item`.
    fn list(&self, kind: ListKind, body: &[Node]) -> Option<BlockKind> {
        let is_item = |n: &Node| matches!(&n.kind, Kind::Command { name, .. } if name == "item");
        let starts: Vec<usize> = body.iter().enumerate().filter(|(_, n)| is_item(n)).map(|(i, _)| i).collect();
        let first = *starts.first()?;
        if !body[..first].iter().all(|n| invisible(self.text, n)) {
            return None;
        }
        let mut items = Vec::new();
        for (k, &s) in starts.iter().enumerate() {
            let next = starts.get(k + 1).copied().unwrap_or(body.len());
            let (Some(head), Some(last)) = (body.get(s), body.get(next - 1)) else { continue };
            let label = match &head.kind {
                Kind::Command { args, .. } => args.first().map(|a| self.inlines(&a.body, Style::default())),
                _ => None,
            };
            let blocks = self.flow(body.get(s + 1..next).unwrap_or_default());
            items.push(Item { label, blocks, range: head.range.start..last.range.end });
        }
        Some(BlockKind::List { kind, items })
    }

    /// A figure card, or `None` when the figure holds more than the card shows.
    fn figure(&self, body: &[Node]) -> Option<BlockKind> {
        let (mut image, mut caption, mut label) = (None, Vec::new(), None);
        for n in body {
            if invisible(self.text, n) {
                continue;
            }
            let Kind::Command { name, args, .. } = &n.kind else { return None };
            match (name.as_str(), required(args)) {
                ("centering", _) => {}
                ("includegraphics", Some(arg)) if image.is_none() => {
                    let options = args.iter().find(|a| a.optional).map(|a| self.src(&a.inner).to_owned()).unwrap_or_default();
                    image = Some(Image { path: self.src(&arg.inner).trim().to_owned(), options, range: n.range.clone() });
                }
                ("caption", Some(arg)) if caption.is_empty() => caption = self.inlines(&arg.body, Style::default()),
                ("label", Some(_)) if label.is_none() => label = self.chip(n, name, args),
                _ => return None,
            }
        }
        Some(BlockKind::Figure { image, caption, label })
    }

    /// The inline runs of a sequence (an argument's body), block-level nodes shown inline.
    fn inlines(&self, nodes: &[Node], style: Style) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut style = style;
        let mut i = 0;
        while i < nodes.len() {
            let mut one = Vec::new();
            i = self.inline_at(nodes, i, &mut style, &mut one);
            for inline in one {
                match inline.kind {
                    InlineKind::Text { text, style } => push_text(&mut out, &text, style, inline.range),
                    _ => out.push(inline),
                }
            }
        }
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = out.first_mut() {
            *text = text.trim_ascii_start().to_owned();
        }
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = out.last_mut() {
            *text = text.trim_ascii_end().to_owned();
        }
        out.retain(|i| !matches!(&i.kind, InlineKind::Text { text, .. } if text.is_empty()));
        out
    }

    /// The inlines of `nodes[i]` (and of the nodes it takes with it); returns the next index.
    fn inline_at(&self, nodes: &[Node], i: usize, style: &mut Style, out: &mut Vec<Inline>) -> usize {
        let Some(n) = nodes.get(i) else { return i + 1 };
        let range = n.range.clone();
        let source = Inline { kind: InlineKind::Source, range: range.clone() };
        if matches!(n.kind, Kind::ControlSymbol | Kind::Command { .. })
            && let Some((letter, len)) = accented(self.text.get(range.start..).unwrap_or_default())
        {
            return self.accent(nodes, i, (letter, range.start + len), *style, out);
        }
        match &n.kind {
            Kind::Text => out.push(text_inline(&typography(self.src(&range)), *style, range)),
            Kind::ParBreak => out.push(Inline { kind: InlineKind::LineBreak, range }),
            Kind::Comment => {
                // The comment takes its line end, and TeX skips the spaces that start the next line.
                if let Some(next @ Node { kind: Kind::Text, .. }) = nodes.get(i + 1) {
                    let src = self.src(&next.range);
                    let rest = src.strip_prefix('\n').map_or(src, str::trim_start);
                    let skipped = src.len() - rest.len();
                    out.push(text_inline(&typography(rest), *style, next.range.start + skipped..next.range.end));
                    return i + 2;
                }
            }
            Kind::Special if self.src(&range) == "~" => out.push(text_inline("\u{a0}", *style, range)),
            Kind::ControlSymbol => match self.src(&range) {
                "\\\\" => out.push(Inline { kind: InlineKind::LineBreak, range }),
                symbol => match control_symbol(symbol) {
                    Some(shown) => out.push(text_inline(shown, *style, range)),
                    None => out.push(source),
                },
            },
            Kind::Command { name, args, .. } => return self.command(nodes, i, (name, args), style, out),
            Kind::Group { body, .. } => out.extend(self.inlines_keep_space(body, *style)),
            Kind::Math { body, display, closed, .. } => {
                out.push(Inline { kind: InlineKind::Formula { source: body.clone(), display: *display, closed: *closed }, range });
            }
            Kind::Special | Kind::Environment { .. } | Kind::Verbatim { .. } | Kind::Error => out.push(source),
        }
        i + 1
    }

    /// An accent command at `nodes[i]` shown as its letter, the source up to `end`: the nodes it
    /// takes (its argument) skipped, and the rest of a text it takes a letter from shown after it.
    /// Returns the next index.
    fn accent(&self, nodes: &[Node], i: usize, (letter, end): (char, usize), style: Style, out: &mut Vec<Inline>) -> usize {
        let start = nodes.get(i).map_or(end, |n| n.range.start);
        out.push(text_inline(&letter.to_string(), style, start..end));
        let mut next = i + 1;
        while let Some(n) = nodes.get(next)
            && n.range.start < end
        {
            if n.range.end > end
                && let Kind::Text = n.kind
            {
                let rest = end..n.range.end;
                out.push(text_inline(&typography(self.src(&rest)), style, rest));
            }
            next += 1;
        }
        next
    }

    /// Like [`Self::inlines`] without trimming: a group's spaces are the paragraph's.
    fn inlines_keep_space(&self, nodes: &[Node], style: Style) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut style = style;
        let mut i = 0;
        while i < nodes.len() {
            i = self.inline_at(nodes, i, &mut style, &mut out);
        }
        out
    }

    fn chip(&self, n: &Node, name: &str, args: &[Arg]) -> Option<Inline> {
        let kind = chip_kind(name)?;
        let key = self.src(&required(args)?.inner).trim().to_owned();
        Some(Inline { kind: InlineKind::Chip { kind, command: name.to_owned(), key }, range: n.range.clone() })
    }

    /// A command's inlines; returns the next index.
    fn command(&self, nodes: &[Node], i: usize, (name, args): (&str, &[Arg]), style: &mut Style, out: &mut Vec<Inline>) -> usize {
        let Some(n) = nodes.get(i) else { return i + 1 };
        let range = n.range.clone();
        if let Some(inner) = styled(name, *style)
            && let Some(arg) = required(args)
        {
            out.extend(self.inlines_keep_space(&arg.body, inner));
            return i + 1;
        }
        if let Some(s) = declared(name, *style) {
            *style = s;
            return i + 1;
        }
        if let Some(text) = logo(name) {
            // The empty group that ends its name (`\textasciitilde{}`, typed as `~`) goes with it.
            if let Some(Node { kind: Kind::Group { body, closed: true }, range: group }) = nodes.get(i + 1)
                && body.is_empty()
                && group.start == range.end
            {
                out.push(text_inline(text, *style, range.start..group.end));
                return i + 2;
            }
            out.push(text_inline(text, *style, range));
            return i + 1;
        }
        if let Some(chip) = self.chip(n, name, args) {
            out.push(chip);
            return i + 1;
        }
        let kind = match (name, required(args)) {
            ("footnote" | "thanks", Some(arg)) => InlineKind::Footnote(self.inlines(&arg.body, Style::default())),
            ("url", Some(arg)) => {
                let url = self.src(&arg.inner).trim().to_owned();
                let text = vec![text_inline(&url, Style { mono: true, ..*style }, arg.inner.clone())];
                InlineKind::Link { url, text }
            }
            ("href", Some(arg)) if args.len() == 2 => {
                let url = args.first().map(|a| self.src(&a.inner).trim().to_owned()).unwrap_or_default();
                InlineKind::Link { url, text: self.inlines(&arg.body, *style) }
            }
            ("and", _) => InlineKind::LineBreak,
            _ => return self.unknown(nodes, i, out),
        };
        out.push(Inline { kind, range });
        i + 1
    }

    /// A command the view does not know, shown as its source with the groups right after it (its
    /// arguments, as far as the view can tell).
    fn unknown(&self, nodes: &[Node], i: usize, out: &mut Vec<Inline>) -> usize {
        let Some(n) = nodes.get(i) else { return i + 1 };
        let mut end = n.range.end;
        let mut next = i + 1;
        while let Some(Node { kind: Kind::Group { .. }, range }) = nodes.get(next)
            && range.start == end
        {
            end = range.end;
            next += 1;
        }
        out.push(Inline { kind: InlineKind::Source, range: n.range.start..end });
        next
    }
}

/// The key of the first `\label` among `nodes` (and in their groups).
pub(crate) fn first_label(text: &str, nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|n| match &n.kind {
        Kind::Command { name, args, .. } if name == "label" => required(args).and_then(|a| text.get(a.inner.clone())).map(|k| k.trim().to_owned()),
        Kind::Group { body, .. } => first_label(text, body),
        _ => None,
    })
}

/// The nodes of `nodes` that show in `r` (a cell's text): text clipped to it, and the body of a
/// group that holds it (a `\multicolumn`'s argument).
fn clipped(nodes: &[Node], r: &Range<usize>) -> Vec<Node> {
    let mut out = Vec::new();
    for n in nodes {
        if n.range.end <= r.start || n.range.start >= r.end {
            continue;
        }
        let inside = r.start <= n.range.start && n.range.end <= r.end;
        match &n.kind {
            _ if inside => out.push(n.clone()),
            Kind::Text => out.push(Node { kind: Kind::Text, range: n.range.start.max(r.start)..n.range.end.min(r.end) }),
            Kind::Group { body, .. } if n.range.start < r.start && r.end <= n.range.end => return clipped(body, r),
            _ => {}
        }
    }
    out
}

fn text_inline(text: &str, style: Style, range: Range<usize>) -> Inline {
    Inline { kind: InlineKind::Text { text: text.to_owned(), style }, range }
}
