//! The view model of the formatted view (docs/ROADMAP.md): the [`tree`](crate::tree)
//! mapped to blocks and inline runs, each holding the byte range of the source it stands for.
//!
//! What the view cannot show faithfully stays as its source ([`BlockKind::Source`],
//! [`InlineKind::Source`]): nothing is hidden except comments and the commands whose whole effect
//! is shown another way (a style, a heading level, a list marker).

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use crate::names::Names;
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
    /// The size declared (`\large`, `\Huge`): steps from the text's size, `\tiny` -4 to `\Huge` 5.
    pub size: i8,
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
    /// A command that prints something the view names rather than shows: `\tableofcontents`,
    /// `\listoffigures`, `\today` (the date), `\newpage`, `\appendix`, `\maketitle` with nothing set.
    Placeholder,
    /// Beamer's overlays: `\pause`, and an overlay specification (`\item<2->`, `\only<3>`); `key`
    /// is the specification without its angle brackets.
    Overlay,
    /// A call of a command the document defines ([`Macro`]): its name; its text arguments follow.
    Macro,
    /// A `\bibitem` of a `thebibliography`: its key.
    Entry,
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
    /// A text environment shown as its formatted body under its name: `abstract`, `quote`, `center`,
    /// a letter, beamer's frames, blocks and columns, a title page…
    Container {
        name: String,
        /// Its text arguments, shown as lines of their own: a frame's title (its argument or
        /// `\frametitle`) and subtitle, a block's title, a letter's recipient.
        titles: Vec<Field>,
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

/// A text argument shown as a line of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub text: Vec<Inline>,
    /// Inside its braces (where the caret stands in an empty one).
    pub slot: Range<usize>,
}

/// elsarticle's `frontmatter`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    pub title: Vec<Inline>,
    /// Inside the braces of its `\title` (where the caret stands in an empty one).
    pub title_slot: Option<Range<usize>>,
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
    /// Inside the braces of its name.
    pub slot: Range<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Affiliation {
    /// As written in its optional argument; empty without one.
    pub key: String,
    /// Its text; for elsarticle's `organization={…}, city={…}` form, the values, `, ` between
    /// them (text standing for no source).
    pub text: Vec<Inline>,
    /// Inside its braces.
    pub slot: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub key: String,
    pub kind: NoteKind,
    pub text: Vec<Inline>,
    /// Inside its braces.
    pub slot: Range<usize>,
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
    /// A `thebibliography`: its entries, each `\bibitem`'s key a chip at its start.
    Bibliography,
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
pub(crate) const CONTAINERS: &[&str] = &[
    "abstract",
    "keyword",
    "quote",
    "quotation",
    "verse",
    "center",
    "flushleft",
    "flushright",
    "letter",
    "titlepage",
    "thebibliography",
    // Beamer.
    "frame",
    "block",
    "alertblock",
    "exampleblock",
    "columns",
    "column",
];

/// Commands that print what the view names ([`ChipKind::Placeholder`]).
const PLACEHOLDERS: &[&str] = &[
    "tableofcontents",
    "listoffigures",
    "listoftables",
    "printbibliography",
    "bibliography",
    "printindex",
    "today",
    "newpage",
    "clearpage",
    "cleardoublepage",
    "pagebreak",
    "appendix",
    "titlepage",
];

/// Beamer's commands that show their argument on some slides only (`\only<2>{…}`): the view shows
/// the specification as a chip and the argument as text.
pub(crate) const OVERLAY_WRAPPERS: &[&str] = &["only", "uncover", "visible", "invisible", "onslide", "alert", "structure"];

/// A part of an overlay specification (`3-`, `alert@3`; `3-| alert@3` has two): an action, when
/// it names one, and the slides it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPart {
    /// `alert`, `uncover`, `only`…; `None` for the command's own.
    pub action: Option<String>,
    pub slides: Vec<Slides>,
}

/// Slides of an overlay specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slides {
    /// `3`.
    One(u32),
    /// `3-`.
    From(u32),
    /// `-3`.
    UpTo(u32),
    /// `2-4`.
    Between(u32, u32),
    /// `+-`: from the next step on.
    FromNext,
    /// `+`: the next step only.
    Next,
}

/// An overlay specification without its angle brackets (`3-| alert@3`) read part by part; `None`
/// for one the view does not read (a mode, `beamer:2`; an offset, `+(1)-`).
pub fn overlay_parts(spec: &str) -> Option<Vec<OverlayPart>> {
    let number = |s: &str| s.trim().parse::<u32>().ok();
    spec.split('|')
        .map(|part| {
            let (action, ranges) = match part.split_once('@') {
                Some((a, r)) => (Some(a.trim().to_owned()), r),
                None => (None, part),
            };
            if action.as_ref().is_some_and(|a| a.is_empty() || !a.chars().all(|c| c.is_ascii_alphabetic())) {
                return None;
            }
            let slides = ranges
                .split(',')
                .map(|r| match r.trim() {
                    "+-" => Some(Slides::FromNext),
                    "+" => Some(Slides::Next),
                    r => match r.split_once('-') {
                        None => number(r).map(Slides::One),
                        Some(("", b)) => number(b).map(Slides::UpTo),
                        Some((a, "")) => number(a).map(Slides::From),
                        Some((a, b)) => Some(Slides::Between(number(a)?, number(b)?)),
                    },
                })
                .collect::<Option<Vec<Slides>>>()?;
            Some(OverlayPart { action, slides })
        })
        .collect()
}

/// The parts of a letter (the letter class) shown as paragraphs of their text.
const LETTER_PARTS: &[&str] = &["opening", "closing", "ps", "encl", "cc"];

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

/// What the main file declares that the view of a file it includes uses: its theorem-like
/// environments, its macros (`\newcommand` in its preamble), and the names its class and language
/// print.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    pub theorems: Vec<TheoremKind>,
    pub macros: Vec<Macro>,
    pub names: Names,
}

impl Declared {
    /// What the main file `main` declares.
    pub fn of(main: &str) -> Self {
        Declared { theorems: theorems(main), macros: macros(main), names: crate::names::names(main) }
    }
}

thread_local! {
    /// The declarations of the file being shown and edited ([`within`]).
    static AROUND: RefCell<Option<Arc<Declared>>> = const { RefCell::new(None) };
}

/// Runs `f` with `declared` (the main file's, [`Declared::of`]) known to the view and the edits
/// of a file ([`view`], [`crate::edit`]), as for a file the main file includes: its macros show
/// as text there, its names print. What the file itself declares comes first.
pub fn within<R>(declared: Arc<Declared>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<Declared>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let old = self.0.take();
            AROUND.with(|a| *a.borrow_mut() = old);
        }
    }
    let _restore = Restore(AROUND.with(|a| a.borrow_mut().replace(declared)));
    f()
}

/// The declarations [`within`] gives, if any.
pub fn around() -> Option<Arc<Declared>> {
    AROUND.with(|a| a.borrow().clone())
}

/// The macros known in `text`: its own, then those declared around it ([`within`]).
pub(crate) fn known_macros(text: &str) -> Vec<Macro> {
    let mut out = macros(text);
    if let Some(d) = around() {
        out.extend(d.macros.iter().filter(|m| !out.iter().any(|o| o.name == m.name)).cloned().collect::<Vec<_>>());
    }
    out
}

/// The names `text` prints: its own when it is a main file (it has `\documentclass`), else those
/// declared around it ([`within`]), else English's.
pub fn names_of(text: &str) -> Names {
    if text.contains("\\documentclass") {
        return crate::names::names(text);
    }
    around().map(|d| d.names.clone()).unwrap_or_default()
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
    let around = around();
    let others = declared.iter().chain(around.iter().flat_map(|d| d.theorems.iter()));
    for d in others {
        if !known.iter().any(|k| k.env == d.env) {
            known.push(d.clone());
        }
    }
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
    /// A `\maketitle` shows the title block: the `\title`, `\author` and `\date` of the body
    /// before it are shown there, not where they stand too.
    shows_title: bool,
    /// The commands the document defines whose calls show as text.
    macros: Vec<Macro>,
    /// What it prints (a letter's "encl:"), worked out when a letter needs it.
    names: std::cell::OnceCell<Names>,
}

/// The last required argument.
fn required(args: &[Arg]) -> Option<&Arg> {
    args.iter().rev().find(|a| !a.optional)
}

/// The first required argument.
fn required_arg(args: &[Arg]) -> Option<&Arg> {
    args.iter().find(|a| !a.optional)
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
        Kind::Environment { begin, body, .. } => {
            // After what shows nothing at its start (`\centering`, `\vfill`, `{\Large}`): where
            // text typed shows.
            let src = text.get(range.clone())?;
            let end = body
                .iter()
                .map(|n| n.range.end)
                .take_while(|end| src.get(begin.end..*end).is_some_and(crate::edit::declares_only))
                .last()
                .unwrap_or(begin.end);
            Some(range.start + end)
        }
        _ => None,
    }
}

/// Whether the environment whose source is `text[range]` starts with `\centering` (a frame, a
/// title page): its lines are centred.
pub fn centred(text: &str, range: &Range<usize>) -> bool {
    let nodes = tree::parse(text.get(range.clone()).unwrap_or_default());
    let Some(Node { kind: Kind::Environment { body, .. }, .. }) = nodes.first() else { return false };
    let src = text.get(range.clone()).unwrap_or_default();
    body.iter().find(|n| !invisible(src, n)).is_some_and(|n| matches!(&n.kind, Kind::Command { name, .. } if name == "centering"))
}

/// Where the text of the list item whose source is `text[range]` starts when it has none: after
/// `\item`, its overlay (`<2->`) and one space.
pub fn item_text_start(text: &str, range: &Range<usize>) -> usize {
    let head = tree::parse(text.get(range.clone()).unwrap_or_default());
    let src = text.get(range.clone()).unwrap_or_default();
    let end = match head.first() {
        Some(n @ Node { kind: Kind::Command { .. }, .. }) => tree::overlay(src, n).map_or(range.start + "\\item".len(), |o| range.start + o.end),
        _ => range.start + "\\item".len(),
    };
    if text.get(end..end + 1) == Some(" ") { end + 1 } else { end }
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
        BlockKind::Container { titles, .. } => titles.iter().for_each(|t| add(&t.text, &mut out)),
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
        BlockKind::Preamble | BlockKind::Formula { .. } | BlockKind::UnsupportedTable(_) | BlockKind::Source | BlockKind::Include { .. } => {}
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
        "tiny" => Style { size: -4, ..s },
        "scriptsize" => Style { size: -3, ..s },
        "footnotesize" => Style { size: -2, ..s },
        "small" => Style { size: -1, ..s },
        "normalsize" => Style { size: 0, ..s },
        "large" => Style { size: 1, ..s },
        "Large" => Style { size: 2, ..s },
        "LARGE" => Style { size: 3, ..s },
        "huge" => Style { size: 4, ..s },
        "Huge" => Style { size: 5, ..s },
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
        "quad" => "\u{2003}",
        "qquad" => "\u{2003}\u{2003}",
        "hfill" => "\u{2003}",
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
    // A line starts with no space.
    if let Some(Inline { kind: InlineKind::LineBreak, .. }) = out.last() {
        text = text.trim_start_matches(' ');
    }
    if let Some(Inline { kind: InlineKind::Text { text: last, style: last_style }, range: last_range }) = out.last_mut() {
        if last.ends_with([' ', '\n']) {
            text = text.strip_prefix(' ').unwrap_or(text);
        }
        // What the view adds (a gap, a line's end) stands for no source: it stays a run of its own.
        if *last_style == style && last_range.end == range.start && last_range.start < last_range.end && range.start < range.end {
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
        // What `\maketitle` prints is what was set before it: the preamble's, and the body's up to
        // it (an `\author` after it, or after `\end{document}`, prints nothing there).
        let maketitle = |n: &&Node| matches!(&n.kind, Kind::Command { name, .. } if name == "maketitle" || name == "titlepage");
        let document = |n: &&Node| matches!(&n.kind, Kind::Environment { name, .. } if name == "document");
        let preamble = top.iter().take_while(|n| !document(n) && !maketitle(n));
        let shows_title = top.iter().chain(body).any(|n| maketitle(&n));
        for n in preamble.chain(body.iter().take_while(|n| !maketitle(n))) {
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
        let macros = known_macros(text).into_iter().filter(Macro::shown).collect();
        View { text, titles, theorems, shows_title, macros, names: std::cell::OnceCell::new() }
    }

    fn src(&self, range: &Range<usize>) -> &'a str {
        self.text.get(range.clone()).unwrap_or_default()
    }

    /// The blocks of a sequence of nodes.
    fn flow(&self, nodes: &[Node]) -> Vec<Block> {
        let mut f = Flow::default();
        let mut i = 0;
        while let Some(n) = nodes.get(i) {
            if n.kind == Kind::ParBreak || matches!(&n.kind, Kind::Command { name, args, .. } if name == "par" && args.is_empty()) {
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
            let from = i;
            i = self.inline_at(nodes, i, &mut f.style, &mut inlines);
            // What it took with it (a macro's arguments): the paragraph covers that too.
            let took = nodes.get(i.saturating_sub(1)).filter(|_| i > from + 1).map_or(n.range.end, |l| l.range.end.max(n.range.end));
            if let [label @ Inline { kind: InlineKind::Chip { kind: ChipKind::Label, .. }, .. }] = inlines.as_slice()
                && f.attach_label(label)
            {
                continue;
            }
            if !inlines.is_empty() && matches!(n.kind, Kind::Command { .. } | Kind::Group { .. }) {
                let span = n.range.start..took;
                f.wraps = Some(f.wraps.take().map_or(span.clone(), |w| w.start.min(span.start)..w.end.max(span.end)));
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
                if name == "maketitle" || name == "titlepage" {
                    self.title()
                } else if LETTER_PARTS.contains(&name.as_str())
                    && let Some(arg) = required(args).filter(|a| a.closed)
                {
                    // Its text, edited where it stands (in its braces), after the name the page
                    // prints before it ("encl: ").
                    let mut inlines = self.inlines(&arg.body, style);
                    let names = || self.names.get_or_init(|| names_of(self.text));
                    let label = match name.as_str() {
                        "encl" => Some(&names().enclosure),
                        "cc" => Some(&names().copies),
                        _ => None,
                    };
                    if let Some(label) = label {
                        let at = arg.inner.start;
                        inlines.insert(0, text_inline(&format!("{label}: "), style, at..at));
                    }
                    return Some(Block { kind: BlockKind::Paragraph(inlines), range: arg.inner.clone() });
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
                _ => self.environment(name, args, body, end.is_some()),
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
                    // Deleting it joins the two when only the next `\author{` is between them;
                    // else (the preamble's end between them) it stands for nothing.
                    let between = before.inner.end..arg.inner.start;
                    let rest = self.src(&between).trim_start_matches(|c: char| c == '}' || c.is_whitespace()).trim_end_matches('{');
                    let joins = rest.strip_prefix("\\author").is_some_and(|r| !r.contains(['\\', '%', '{', '}']));
                    let range = if joins { between } else { arg.inner.start..arg.inner.start };
                    out.push(Inline { kind: InlineKind::LineBreak, range });
                }
                out.extend(self.inlines(&arg.body, Style::default()));
            }
            out
        });
        let slots = self.titles.each_ref().map(|args| args.last().map(|a| a.inner.clone()));
        BlockKind::Title { title, author, date, slots }
    }

    fn environment(&self, name: &str, args: &[Arg], body: &[Node], closed: bool) -> BlockKind {
        let field = |a: &Arg| Field { text: self.inlines(&a.body, Style::default()), slot: a.inner.clone() };
        let required: Vec<Field> = args.iter().filter(|a| !a.optional && a.closed).map(field).collect();
        let shown = match name {
            "itemize" => self.list(ListKind::Bullets, body),
            "enumerate" => self.list(ListKind::Numbers, body),
            "description" => self.list(ListKind::Descriptions, body),
            // Its entries as a list; what comes before the first, paragraphs before it.
            "thebibliography" => {
                let first = body.iter().position(|n| matches!(&n.kind, Kind::Command { name, .. } if name == "bibitem")).unwrap_or(body.len());
                let leading = body.get(..first).unwrap_or_default();
                let shown = |s: &[Node]| match (s.first(), s.last()) {
                    (Some(a), Some(b)) => !self.text.get(a.range.start..b.range.end).is_some_and(crate::edit::declares_only),
                    _ => false,
                };
                if first < body.len() && !shown(leading) {
                    return self.list(ListKind::Bibliography, body).unwrap_or(BlockKind::Source);
                }
                let mut blocks = self.flow(leading);
                if let (Some(list), Some(start), Some(end)) = (self.list(ListKind::Bibliography, body), body.get(first), body.last()) {
                    blocks.push(Block { kind: list, range: start.range.start..end.range.end });
                }
                Some(BlockKind::Container { name: name.to_owned(), titles: Vec::new(), blocks })
            }
            "frame" => {
                // Its title and subtitle: its arguments, or `\frametitle` and `\framesubtitle` in it.
                let mut titles = required;
                let mut rest = Vec::new();
                for n in body {
                    match &n.kind {
                        Kind::Command { name, args, .. } if matches!(name.as_str(), "frametitle" | "framesubtitle") => match required_arg(args) {
                            Some(a) if a.closed && titles.len() < 2 && (name == "frametitle") == titles.is_empty() => titles.push(field(a)),
                            _ => rest.push(n.clone()),
                        },
                        _ => rest.push(n.clone()),
                    }
                }
                Some(BlockKind::Container { name: name.to_owned(), titles, blocks: self.flow(&rest) })
            }
            "letter" | "block" | "alertblock" | "exampleblock" => {
                Some(BlockKind::Container { name: name.to_owned(), titles: required, blocks: self.flow(body) })
            }
            "figure" | "figure*" if closed => self.figure(body),
            "table" | "table*" if closed => self.table_float(body),
            "frontmatter" if closed => Some(BlockKind::FrontMatter(Box::new(self.front_matter(body)))),
            _ if CONTAINERS.contains(&name) => Some(BlockKind::Container { name: name.to_owned(), titles: Vec::new(), blocks: self.flow(body) }),
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
                "title" => {
                    (fm.title, fm.title_notes) = self.marked(&arg.body);
                    fm.title_slot = Some(arg.inner.clone());
                }
                "author" => {
                    let (name, notes) = self.marked(&arg.body);
                    let affiliations = key(args).split(',').map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()).collect();
                    fm.authors.push(Author { name, affiliations, notes, emails: Vec::new(), slot: arg.inner.clone() });
                }
                "ead" => match fm.authors.last_mut() {
                    Some(a) => a.emails.push(self.inlines(&arg.body, Style::default())),
                    None => rest.push(n.clone()),
                },
                "affiliation" | "address" => {
                    fm.affiliations.push(Affiliation { key: key(args), text: self.affiliation(arg), slot: arg.inner.clone() });
                }
                "tnotetext" | "cortext" | "fntext" => {
                    let kind = match name.as_str() {
                        "tnotetext" => NoteKind::Title,
                        "cortext" => NoteKind::Corresponding,
                        _ => NoteKind::Author,
                    };
                    fm.notes.push(Note { key: key(args), kind, text: self.inlines(&arg.body, Style::default()), slot: arg.inner.clone() });
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
        let command = if kind == ListKind::Bibliography { "bibitem" } else { "item" };
        let is_item = |n: &Node| matches!(&n.kind, Kind::Command { name, .. } if name == command);
        let starts: Vec<usize> = body.iter().enumerate().filter(|(_, n)| is_item(n)).map(|(i, _)| i).collect();
        let first = *starts.first()?;
        if kind != ListKind::Bibliography && !body[..first].iter().all(|n| invisible(self.text, n)) {
            return None;
        }
        let mut items = Vec::new();
        for (k, &s) in starts.iter().enumerate() {
            let next = starts.get(k + 1).copied().unwrap_or(body.len());
            let (Some(head), Some(last)) = (body.get(s), body.get(next - 1)) else { continue };
            let label = match &head.kind {
                // An entry's key, as a chip standing for its `\bibitem`.
                Kind::Command { args, .. } if kind == ListKind::Bibliography => {
                    let key = required_arg(args).map(|a| self.src(&a.inner).trim().to_owned()).unwrap_or_default();
                    Some(vec![Inline {
                        kind: InlineKind::Chip { kind: ChipKind::Entry, command: command.to_owned(), key },
                        range: head.range.clone(),
                    }])
                }
                Kind::Command { args, .. } => args.first().map(|a| self.inlines(&a.body, Style::default())),
                _ => None,
            };
            let mut blocks = self.flow(body.get(s + 1..next).unwrap_or_default());
            // Beamer's `\item<2->`: its specification a chip before its text.
            if let Some(spec) = tree::overlay(self.text, head) {
                let chip = self.overlay_chip("item", spec);
                // A gap the view adds after it.
                let gap = text_inline("\u{2002}", Style::default(), chip.range.end..chip.range.end);
                match blocks.first_mut() {
                    Some(Block { kind: BlockKind::Paragraph(p), range }) => {
                        range.start = chip.range.start;
                        p.insert(0, gap);
                        p.insert(0, chip);
                    }
                    _ => {
                        let range = chip.range.clone();
                        blocks.insert(0, Block { kind: BlockKind::Paragraph(vec![chip]), range });
                    }
                }
            }
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
                "\\\\" => {
                    // `\\[0.5cm]`: the space it adds goes with it.
                    if let Some(Node { kind: Kind::Text, range: next }) = nodes.get(i + 1)
                        && next.start == range.end
                        && let Some(inner) = self.src(next).strip_prefix('[')
                        && let Some(close) = inner.find(']')
                        && !inner.get(..close).unwrap_or_default().contains('\n')
                    {
                        let end = next.start + close + 2;
                        out.push(Inline { kind: InlineKind::LineBreak, range: range.start..end });
                        if end < next.end {
                            out.push(text_inline(&typography(self.src(&(end..next.end))), *style, end..next.end));
                        }
                        return i + 2;
                    }
                    out.push(Inline { kind: InlineKind::LineBreak, range });
                }
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

    /// An overlay specification's chip: `spec` with its angle brackets.
    fn overlay_chip(&self, command: &str, spec: Range<usize>) -> Inline {
        let key = self.src(&spec).trim_start_matches('<').trim_end_matches('>').to_owned();
        Inline { kind: InlineKind::Chip { kind: ChipKind::Overlay, command: command.to_owned(), key }, range: spec }
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
        if let Some(slot) = ["title", "author", "date"].iter().position(|t| *t == name)
            && self.shows_title
            && let Some(arg) = required(args)
            && self.titles.get(slot).is_some_and(|kept| kept.iter().any(|a| a.inner == arg.inner))
        {
            return i + 1;
        }
        // An empty `\title{}`, `\author{}` or `\date{}` sets nothing to print.
        if matches!(name, "title" | "author" | "date") && required(args).is_some_and(|a| a.closed && self.src(&a.inner).trim().is_empty()) {
            return i + 1;
        }
        if PLACEHOLDERS.contains(&name) {
            let key = required(args).map(|a| self.src(&a.inner).trim().to_owned()).unwrap_or_default();
            out.push(Inline { kind: InlineKind::Chip { kind: ChipKind::Placeholder, command: name.to_owned(), key }, range });
            return i + 1;
        }
        if name == "pause" {
            out.push(Inline { kind: InlineKind::Chip { kind: ChipKind::Overlay, command: name.to_owned(), key: String::new() }, range });
            return i + 1;
        }
        if OVERLAY_WRAPPERS.contains(&name) {
            let spec = tree::overlay(self.text, n);
            if let Some(spec) = spec.clone() {
                out.push(Inline { range: range.clone(), ..self.overlay_chip(name, spec) });
            }
            if let Some(Node { kind: Kind::Group { body, closed: true }, range: group }) = nodes.get(i + 1)
                && group.start == range.end
            {
                out.extend(self.inlines_keep_space(body, *style));
                return i + 2;
            }
            if spec.is_some() {
                return i + 1;
            }
        }
        if let Some(m) = self.macros.iter().find(|m| m.name == name)
            && let Some(next) = self.macro_call(nodes, i, m, *style, out)
        {
            return next;
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
            // Space the page gets, not the text.
            ("newline" | "linebreak", _) => InlineKind::LineBreak,
            // In a group (`{\Large Name\par}`): the line ends there.
            ("par", None) => InlineKind::LineBreak,
            ("vspace" | "hspace" | "smallskip" | "medskip" | "bigskip" | "noindent" | "vfill" | "centering" | "raggedright" | "raggedleft", _) => {
                return i + 1;
            }
            _ => return self.unknown(nodes, i, out),
        };
        out.push(Inline { kind, range });
        i + 1
    }

    /// A call of macro `m` at `nodes[i]`: its name as a chip, then the text of each argument given,
    /// a gap the view adds between them. Returns the next index; `None` (shown as source) when the
    /// call is not whole or gives an argument that is not text.
    fn macro_call(&self, nodes: &[Node], i: usize, m: &Macro, style: Style, out: &mut Vec<Inline>) -> Option<usize> {
        let call = macro_call(self.text, nodes, i, m)?;
        let mut shown =
            vec![Inline { kind: InlineKind::Chip { kind: ChipKind::Macro, command: m.name.clone(), key: String::new() }, range: call.name.clone() }];
        for (k, a) in call.args.iter().enumerate() {
            let arg_style = m.args.get(a.index).and_then(|d| d.text)?;
            let inner = merge_style(style, arg_style);
            if k > 0 {
                shown.push(text_inline("\u{2002}\u{b7}\u{2002}", Style::default(), a.range.start..a.range.start));
            }
            match a.body {
                Some(body) => shown.extend(self.inlines(body, inner)),
                None => shown.push(text_inline(typography(self.src(&a.inner)).trim(), inner, a.inner.clone())),
            }
        }
        if let Some(rest) = &call.rest {
            shown.push(text_inline(&typography(self.src(rest)), style, rest.clone()));
        }
        // A macro that ends its paragraph (`\cvsection{…}`): what follows starts a line, which the
        // view adds (the source has no blank line there).
        if m.breaks
            && call.rest.is_none()
            && let Some(last) = call.args.last()
        {
            shown.push(text_inline("\n", Style::default(), last.range.end..last.range.end));
        }
        out.extend(shown);
        Some(call.next)
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

/// A command the document defines (`\newcommand`, `\NewDocumentCommand`) whose calls the view
/// shows as their text: its name, and its arguments in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Macro {
    pub name: String,
    pub args: Vec<MacroArg>,
    /// Its body ends a paragraph (`\par`): a call is a paragraph of its own.
    pub breaks: bool,
}

/// An argument of a [`Macro`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroArg {
    /// `[…]` rather than `{…}`.
    pub optional: bool,
    /// How its text is set when it is text: every use of it in the definition is in running text
    /// ([`macros`]); `None` when it is not (a width, a file, a test, math).
    pub text: Option<Style>,
}

impl Macro {
    /// Whether its calls are shown as their text: it takes an argument, and each required one is
    /// text (an optional one that is not text is shown so only when a call leaves it out).
    pub(crate) fn shown(&self) -> bool {
        !self.args.is_empty() && self.args.iter().all(|a| a.optional || a.text.is_some())
    }
}

/// Commands of no arguments around a macro's `#1` that take nothing from what follows them.
pub(crate) fn takes_nothing(name: &str) -> bool {
    declared(name, Style::default()).is_some()
        || logo(name).is_some()
        || matches!(
            name,
            "par"
                | "noindent"
                | "indent"
                | "centering"
                | "raggedright"
                | "raggedleft"
                | "medskip"
                | "smallskip"
                | "bigskip"
                | "newline"
                | "hfill"
                | "vfill"
                | "hrule"
                | "relax"
                | "fi"
                | "else"
                | "null"
                | "strut"
                | "unskip"
                | "ignorespaces"
                | "today"
                | "and"
        )
}

/// The arguments of commands the view does not know that a macro's body often holds: for each,
/// whether it is text (`\textcolor{red}{#1}` prints its second).
fn known_arity(name: &str) -> Option<&'static [bool]> {
    Some(match name {
        "color" | "pagecolor" | "setcounter" | "stepcounter" | "refstepcounter" | "addtocounter" => match name {
            "setcounter" | "addtocounter" => &[false, false],
            _ => &[false],
        },
        "textcolor" | "colorbox" => &[false, true],
        "alert" | "structure" | "only" | "uncover" | "visible" | "invisible" | "onslide" => &[true],
        "fcolorbox" => &[false, false, true],
        "rule" | "setlength" | "addtolength" => &[false, false],
        "makebox" | "framebox" | "fbox" | "parbox" | "raisebox" => match name {
            "fbox" => &[true],
            "parbox" | "raisebox" => &[false, true],
            _ => &[true],
        },
        _ => return None,
    })
}

/// The commands `\newcommand`, `\renewcommand`, `\providecommand` and xparse's
/// `\NewDocumentCommand` (and kin) define in `text`, at its top level and in its document body,
/// with which of their arguments are text.
///
/// The rule: an argument is text when it is used, and every use of it in the definition is in
/// running text, reached from the definition's top level only through groups, the bodies of
/// environments (not math or verbatim), and the text arguments of the commands the view shows as
/// text (`\textbf{#1}`, `\emph`, `\footnote`, `\textcolor`'s second); never in math, in a
/// command's other argument (`\label{#1}`, `\includegraphics{#1}`, `\vspace{#1}`), in the groups
/// after a command the view does not know (they may be its arguments), or right after such a
/// command. LaTeX's emptiness tests (`\ifx\relax#1\relax`, `\if\relax\detokenize{#1}\relax`) do
/// not count: they only ask whether there is text. xparse's argument types other than `m`, `o` and
/// `O{…}` make a command not simple enough to show; so do `\def`'s.
pub fn macros(text: &str) -> Vec<Macro> {
    let tree = tree::parse(text);
    let body = tree.iter().find_map(|n| match &n.kind {
        Kind::Environment { name, body, .. } if name == "document" => Some(body.as_slice()),
        _ => None,
    });
    let mut out: Vec<Macro> = Vec::new();
    for nodes in [tree.as_slice(), body.unwrap_or_default()] {
        for (i, n) in nodes.iter().enumerate() {
            let Kind::Command { name, args, .. } = &n.kind else { continue };
            let found = match name.as_str() {
                "newcommand" | "renewcommand" | "providecommand" | "DeclareRobustCommand" => new_command(text, nodes, i, args),
                "NewDocumentCommand" | "RenewDocumentCommand" | "ProvideDocumentCommand" | "DeclareDocumentCommand" => {
                    document_command(text, nodes, i)
                }
                _ => None,
            };
            if let Some(m) = found {
                out.retain(|o| o.name != m.name);
                out.push(m);
            }
        }
    }
    out
}

/// The name a definition gives: `{\name}` or `\name`.
fn defined_name(text: &str, nodes: &[Node]) -> Option<String> {
    let node = nodes.iter().find(|n| !invisible(text, n))?;
    match &node.kind {
        Kind::Command { name, star: false, args } if args.is_empty() => Some(name.clone()),
        _ => None,
    }
}

/// `\newcommand{\name}[n][default]{body}`, or `\newcommand\name[n]{body}` (its arguments then
/// after it: the parser reads `\name` as the first).
fn new_command(text: &str, nodes: &[Node], i: usize, args: &[Arg]) -> Option<Macro> {
    let n = nodes.get(i)?;
    let (name, count, default, body) = match args.first() {
        Some(first) if !first.optional && args.last().is_some_and(|a| !a.optional) && args.len() >= 2 => {
            let name = defined_name(text, &first.body)?;
            let opts: Vec<&Arg> = args.iter().filter(|a| a.optional).collect();
            let count = match opts.first() {
                Some(a) => text.get(a.inner.clone())?.trim().parse::<usize>().ok()?,
                None => 0,
            };
            (name, count, opts.len() > 1, &args.last()?.body)
        }
        _ => {
            // `\newcommand\name[1]{…}`: the name, then `[1]` as text, then the body's group.
            if !args.is_empty() {
                return None;
            }
            let Some(Node { kind: Kind::Command { name, args: none, .. }, range }) = nodes.get(i + 1) else { return None };
            if !none.is_empty() || range.start != n.range.end {
                return None;
            }
            let mut j = i + 2;
            let mut count = 0;
            let mut default = false;
            if let Some(Node { kind: Kind::Text, range }) = nodes.get(j) {
                let src = text.get(range.clone())?;
                let (c, rest) = src.strip_prefix('[')?.split_once(']')?;
                count = c.trim().parse::<usize>().ok()?;
                if !rest.is_empty() {
                    default = rest.starts_with('[');
                    if !default {
                        return None;
                    }
                    // A default with structure: not simple.
                    let (_, after) = rest.strip_prefix('[')?.split_once(']')?;
                    if !after.trim().is_empty() {
                        return None;
                    }
                }
                j += 1;
            }
            let Some(Node { kind: Kind::Group { body, closed: true }, .. }) = nodes.get(j) else { return None };
            (name.clone(), count, default, body)
        }
    };
    if count > 9 {
        return None;
    }
    let kinds: Vec<bool> = (0..count).map(|k| k == 0 && default).collect();
    Some(Macro { name, args: analysed(text, body, &kinds), breaks: breaks(text, body) })
}

/// Whether a definition's body ends a paragraph: `\par` at its top level with only spacing after
/// it (`\vspace{…}`, `\hrule`, `\medskip`).
fn breaks(text: &str, body: &[Node]) -> bool {
    let Some(last) = body.iter().rposition(|n| matches!(&n.kind, Kind::Command { name, .. } if name == "par")) else { return false };
    body.get(last + 1..).unwrap_or_default().iter().all(|n| match &n.kind {
        Kind::Command { name, .. } => matches!(name.as_str(), "vspace" | "hrule" | "smallskip" | "medskip" | "bigskip" | "noindent" | "par"),
        Kind::Comment => true,
        _ => invisible(text, n),
    })
}

/// xparse's `\NewDocumentCommand{\name}{m o m}{body}` (or `\NewDocumentCommand\name{…}{…}`).
fn document_command(text: &str, nodes: &[Node], i: usize) -> Option<Macro> {
    let n = nodes.get(i)?;
    let mut j = i + 1;
    let mut end = n.range.end;
    let next_group = |j: &mut usize, end: &mut usize| -> Option<&Node> {
        let g = nodes.get(*j).filter(|g| g.range.start == *end && matches!(g.kind, Kind::Group { closed: true, .. }))?;
        *j += 1;
        *end = g.range.end;
        Some(g)
    };
    let name = match nodes.get(j) {
        Some(Node { kind: Kind::Command { name, args, .. }, range }) if args.is_empty() && range.start == end => {
            j += 1;
            end = range.end;
            name.clone()
        }
        _ => match &next_group(&mut j, &mut end)?.kind {
            Kind::Group { body, .. } => defined_name(text, body)?,
            _ => return None,
        },
    };
    let spec = next_group(&mut j, &mut end)?;
    let body = match &next_group(&mut j, &mut end)?.kind {
        Kind::Group { body, .. } => body,
        _ => return None,
    };
    let spec = text.get(spec.range.start + 1..spec.range.end - 1)?;
    let mut kinds = Vec::new();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'm' => kinds.push(false),
            'o' => kinds.push(true),
            'O' => {
                // Its default, in braces with no braces inside.
                chars.next_if_eq(&'{')?;
                while chars.next_if(|c| *c != '}').is_some() {}
                chars.next_if_eq(&'}')?;
                kinds.push(true);
            }
            c if c.is_whitespace() => {}
            _ => return None,
        }
    }
    if kinds.len() > 9 {
        return None;
    }
    Some(Macro { name, args: analysed(text, body, &kinds), breaks: breaks(text, body) })
}

/// The arguments of a definition whose body is `body`, `optional[k]` for each.
fn analysed(text: &str, body: &[Node], optional: &[bool]) -> Vec<MacroArg> {
    let mut uses: Vec<(usize, Option<Style>)> = Vec::new();
    uses_in(text, body, (Style::default(), true), &mut uses);
    optional
        .iter()
        .enumerate()
        .map(|(k, &optional)| {
            let mine: Vec<Option<Style>> = uses.iter().filter(|(a, _)| *a == k + 1).map(|(_, s)| *s).collect();
            let text = if mine.iter().any(Option::is_none) { None } else { mine.first().copied().flatten() };
            MacroArg { optional, text }
        })
        .collect()
}

/// The uses of `#k` in `nodes`: `k` with the style it is set in, or `None` where it is not text.
/// `ok` says whether `nodes` are running text.
fn uses_in(text: &str, nodes: &[Node], (style, ok): (Style, bool), out: &mut Vec<(usize, Option<Style>)>) {
    let mut style = style;
    // The groups after a command are its arguments: whether each one still to come is text.
    let mut pending: Vec<bool> = Vec::new();
    let name_of = |at: Option<usize>| match at.and_then(|p| nodes.get(p)).map(|n| &n.kind) {
        Some(Kind::Command { name, .. }) => Some(name.as_str()),
        _ => None,
    };
    for (i, n) in nodes.iter().enumerate() {
        match &n.kind {
            Kind::Special if text.get(n.range.clone()) == Some("#") => {
                let Some(k) = nodes
                    .get(i + 1)
                    .filter(|t| t.kind == Kind::Text && t.range.start == n.range.end)
                    .and_then(|t| text.get(t.range.clone()))
                    .and_then(|s| s.chars().next())
                    .and_then(|c| c.to_digit(10))
                else {
                    continue;
                };
                // `\ifx\relax#1\relax`: a test of whether it is empty.
                if name_of(i.checked_sub(1)) == Some("relax") && matches!(name_of(i.checked_sub(2)), Some("ifx" | "if")) {
                    continue;
                }
                // Right after a command that may take it as an argument: not text.
                let shown = ok && pending.is_empty();
                out.push((k as usize, shown.then_some(style)));
                pending.clear();
            }
            Kind::Group { body, .. } => {
                // `\if\relax\detokenize{#1}\relax`: a test of whether it is empty.
                if name_of(i.checked_sub(1)) == Some("detokenize") && name_of(i.checked_sub(2)) == Some("relax") {
                    pending.clear();
                    continue;
                }
                let inside = if pending.is_empty() { ok } else { ok && pending.remove(0) };
                uses_in(text, body, (style, inside), out);
            }
            Kind::Command { name, args, .. } => {
                pending.clear();
                if let Some(s) = declared(name, style) {
                    style = s;
                } else if !args.is_empty() {
                    let inner = styled(name, style);
                    let texty = inner.is_some()
                        || heading_level(name).is_some()
                        || matches!(name.as_str(), "footnote" | "thanks" | "caption" | "frametitle" | "framesubtitle");
                    for a in args {
                        uses_in(text, &a.body, (inner.unwrap_or(style), ok && texty && !a.optional), out);
                    }
                } else if let Some(arity) = known_arity(name) {
                    pending = arity.to_vec();
                } else if !takes_nothing(name) {
                    // Up to nine groups after it may be its arguments.
                    pending = vec![false; 9];
                }
            }
            Kind::Environment { args, body, .. } => {
                pending.clear();
                for a in args {
                    uses_in(text, &a.body, (style, false), out);
                }
                uses_in(text, body, (style, ok), out);
            }
            Kind::Math { .. } | Kind::Verbatim { .. } => {
                pending.clear();
                // Its `#k` are math: the source holds them as text.
                let src = text.get(n.range.clone()).unwrap_or_default();
                for k in 1..=9 {
                    if src.contains(&format!("#{k}")) {
                        out.push((k, None));
                    }
                }
            }
            Kind::Text | Kind::Comment | Kind::ParBreak | Kind::Special | Kind::ControlSymbol | Kind::Error => {
                // White space between a command and its arguments keeps them its arguments.
                if !text.get(n.range.clone()).unwrap_or_default().trim().is_empty() || n.kind == Kind::ParBreak {
                    pending.clear();
                }
            }
        }
    }
}

/// An argument a call of a macro gives.
pub(crate) struct CallArg<'a> {
    /// Which of the macro's arguments it is.
    pub index: usize,
    /// With its braces or brackets.
    pub range: Range<usize>,
    pub inner: Range<usize>,
    /// A required argument's nodes; an optional one's text is `inner`.
    pub body: Option<&'a [Node]>,
}

/// A call of a macro: the arguments it gives, the text after an optional last one's `]` in the
/// same text node, and the index of the node after it.
pub(crate) struct Call<'a> {
    /// The command's name (`\\cventry`): its chip.
    pub name: Range<usize>,
    pub args: Vec<CallArg<'a>>,
    pub rest: Option<Range<usize>>,
    pub next: usize,
}

/// The call of macro `m` at `nodes[i]`: each required argument a group after it (white space
/// between them, no blank line), an optional one `[…]` in the text right after it (with no
/// structure inside). `None` when it is not whole, or is starred.
pub(crate) fn macro_call<'a>(text: &str, nodes: &'a [Node], i: usize, m: &Macro) -> Option<Call<'a>> {
    let n = nodes.get(i)?;
    if matches!(n.kind, Kind::Command { star: true, .. }) {
        return None;
    }
    let Kind::Command { name, args: parsed, .. } = &n.kind else { return None };
    let mut args = Vec::new();
    let mut rest: Option<Range<usize>> = None;
    let mut j = i + 1;
    let mut end = n.range.end;
    // Arguments the parser took already (a name it knows: `\exercise[…]`, a theorem's).
    let mut parsed = parsed.iter().peekable();
    for (k, a) in m.args.iter().enumerate() {
        if rest.is_some() {
            return None;
        }
        if let Some(p) = parsed.peek() {
            if p.optional != a.optional {
                if a.optional {
                    continue;
                }
                return None;
            }
            args.push(CallArg { index: k, range: p.range.clone(), inner: p.inner.clone(), body: Some(&p.body) });
            parsed.next();
            continue;
        }
        if a.optional {
            let Some(Node { kind: Kind::Text, range }) = nodes.get(j).filter(|t| t.range.start == end) else { continue };
            let src = text.get(range.clone())?;
            let Some(inner) = src.strip_prefix('[') else { continue };
            let close = inner.find(']')?;
            let inner = range.start + 1..range.start + 1 + close;
            args.push(CallArg { index: k, range: range.start..inner.end + 1, inner: inner.clone(), body: None });
            j += 1;
            end = range.end;
            if inner.end + 1 < range.end {
                let after = inner.end + 1..range.end;
                let more = m.args.get(k + 1..).unwrap_or_default().iter().any(|a| !a.optional);
                if more && !text.get(after.clone())?.trim().is_empty() {
                    return None;
                }
                if !more {
                    rest = Some(after);
                }
            }
        } else {
            while let Some(Node { kind: Kind::Text, range }) = nodes.get(j) {
                let src = text.get(range.clone())?;
                if !src.trim().is_empty() || range.start != end {
                    break;
                }
                j += 1;
                end = range.end;
            }
            let Some(Node { kind: Kind::Group { body, closed: true }, range }) = nodes.get(j) else { return None };
            if range.start != end {
                return None;
            }
            args.push(CallArg { index: k, range: range.clone(), inner: range.start + 1..range.end - 1, body: Some(body) });
            j += 1;
            end = range.end;
        }
    }
    if parsed.next().is_some() {
        return None;
    }
    Some(Call { name: n.range.start..n.range.start + 1 + name.len(), args, rest, next: j })
}

/// `inner` applied inside `outer`.
fn merge_style(outer: Style, inner: Style) -> Style {
    Style {
        bold: outer.bold || inner.bold,
        italic: outer.italic != inner.italic,
        underline: outer.underline || inner.underline,
        mono: outer.mono || inner.mono,
        sans: outer.sans || inner.sans,
        small_caps: outer.small_caps || inner.small_caps,
        sup: outer.sup || inner.sup,
        size: if inner.size != 0 { inner.size } else { outer.size },
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
