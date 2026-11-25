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
    /// A text environment shown as its formatted body under its name: `abstract`, `quote`, `center`…
    Container {
        name: String,
        blocks: Vec<Block>,
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
pub(crate) const CONTAINERS: &[&str] = &["abstract", "quote", "quotation", "verse", "center", "flushleft", "flushright"];

/// The view of `text`.
pub fn view(text: &str) -> Vec<Block> {
    build(text, &tree::parse(text))
}

/// The view of `text` from its tree.
pub fn build(text: &str, tree: &[Node]) -> Vec<Block> {
    let document = tree.iter().position(|n| matches!(&n.kind, Kind::Environment { name, .. } if name == "document"));
    let Some((at, Some(Node { kind: Kind::Environment { body, .. }, range }))) = document.map(|i| (i, tree.get(i))) else {
        return View::new(text, tree, &[]).flow(tree);
    };
    let view = View::new(text, tree, body);
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
    /// The bodies of `\title`, `\author` and `\date`.
    titles: [Option<&'a [Node]>; 3],
}

/// The last required argument.
fn required(args: &[Arg]) -> Option<&Arg> {
    args.iter().rev().find(|a| !a.optional)
}

/// The level of a sectioning command.
pub(crate) fn heading_level(name: &str) -> Option<u8> {
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
fn declared(name: &str, s: Style) -> Option<Style> {
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
        "textbackslash" => "\\",
        "textasciitilde" => "~",
        "textasciicircum" => "^",
        "textendash" => "–",
        "textemdash" => "—",
        _ => return None,
    })
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
        if c.is_whitespace() {
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
}

impl Flow {
    fn flush(&mut self) {
        let mut para = std::mem::take(&mut self.para);
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = para.first_mut() {
            *text = text.trim_start().to_owned();
        }
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = para.last_mut() {
            *text = text.trim_end().to_owned();
        }
        para.retain(|i| !matches!(&i.kind, InlineKind::Text { text, .. } if text.is_empty()));
        if let (Some(first), Some(last)) = (para.first(), para.last()) {
            let range = first.range.start..last.range.end;
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
    fn new(text: &'a str, top: &'a [Node], body: &'a [Node]) -> Self {
        let mut titles = [None; 3];
        for n in top.iter().chain(body) {
            if let Kind::Command { name, args, .. } = &n.kind
                && let Some(slot) = ["title", "author", "date"].iter().position(|t| t == name)
                && let Some(arg) = required(args)
            {
                titles[slot] = Some(arg.body.as_slice());
            }
        }
        View { text, titles }
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
                f.flush();
                i += 1;
                continue;
            }
            if let Some(block) = self.block(n, f.style) {
                f.flush();
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
            for inline in inlines {
                match inline.kind {
                    InlineKind::Text { text, style } => push_text(&mut f.para, &text, style, inline.range),
                    _ => f.para.push(inline),
                }
            }
        }
        f.flush();
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
            Kind::Environment { name, body, end, .. } => self.environment(name, body, end.is_some()),
            _ => return None,
        };
        Some(Block { kind, range })
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
        let [title, author, date] = self.titles.map(|t| t.map(|nodes| self.inlines(nodes, Style::default())).unwrap_or_default());
        BlockKind::Title { title, author, date }
    }

    fn environment(&self, name: &str, body: &[Node], closed: bool) -> BlockKind {
        let shown = match name {
            "itemize" => self.list(ListKind::Bullets, body),
            "enumerate" => self.list(ListKind::Numbers, body),
            "description" => self.list(ListKind::Descriptions, body),
            "figure" | "figure*" if closed => self.figure(body),
            "table" | "table*" if closed => self.table_float(body),
            _ if CONTAINERS.contains(&name) => Some(BlockKind::Container { name: name.to_owned(), blocks: self.flow(body) }),
            _ => None,
        };
        shown.unwrap_or(BlockKind::Source)
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
            *text = text.trim_start().to_owned();
        }
        if let Some(Inline { kind: InlineKind::Text { text, .. }, .. }) = out.last_mut() {
            *text = text.trim_end().to_owned();
        }
        out.retain(|i| !matches!(&i.kind, InlineKind::Text { text, .. } if text.is_empty()));
        out
    }

    /// The inlines of `nodes[i]` (and of the nodes it takes with it); returns the next index.
    fn inline_at(&self, nodes: &[Node], i: usize, style: &mut Style, out: &mut Vec<Inline>) -> usize {
        let Some(n) = nodes.get(i) else { return i + 1 };
        let range = n.range.clone();
        let source = Inline { kind: InlineKind::Source, range: range.clone() };
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
