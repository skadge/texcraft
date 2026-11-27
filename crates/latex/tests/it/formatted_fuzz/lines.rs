//! The lines of the formatted view, built from the view model as `texcraft-ui-egui`'s
//! `formatted.rs` builds them while drawing (its `paragraph`, `line_of`, `phantom` and the block
//! functions), without a window: what each character stands for, and the characters shown.
//!
//! The decorations and atoms are shown as the window shows them without a build (no heading or
//! figure numbers, references and citations not found); their exact characters matter only for
//! where the caret stops, not for what an edit does to the source.

use std::ops::Range;

use texcraft_latex::edit::{self, Between, Doc, Line, Part};
use texcraft_latex::table::Table;
use texcraft_latex::view::{self, Block, BlockKind, ChipKind, FrontMatter, Inline, InlineKind, Item, Style};

/// A line of the view: its caret stops and the characters it shows.
#[derive(Debug, Clone)]
pub struct ViewLine {
    pub line: Line,
    pub chars: Vec<char>,
    /// The table cell it is: the table's first byte, its row and column.
    pub cell: Option<(usize, usize, usize)>,
    pub phantom: bool,
}

impl ViewLine {
    /// Its characters that stand for source.
    pub fn sourced(&self) -> String {
        let keep = self.line.parts().iter().flat_map(|p| std::iter::repeat_n(!matches!(p, Part::Decoration(_)), p.chars()));
        self.chars.iter().zip(keep).filter(|(_, k)| *k).map(|(c, _)| *c).collect()
    }
}

/// The view of a text with its lines.
pub struct Shown {
    pub blocks: Vec<Block>,
    pub lines: Vec<ViewLine>,
}

impl Shown {
    pub fn doc(&self) -> Doc {
        Doc { lines: self.lines.iter().map(|l| l.line.clone()).collect() }
    }

    /// The characters of the lines that stand for source (not what the view adds), a line end
    /// between them, phantom lines left out.
    pub fn plain(&self) -> String {
        let lines: Vec<String> = self.lines.iter().filter(|l| !l.phantom).map(ViewLine::sourced).collect();
        lines.join("\n")
    }
}

/// The view of `text` with the caret at `caret` (it may stand in an empty paragraph).
pub fn shown(text: &str, caret: Option<usize>) -> Shown {
    let blocks = view::view(text);
    let mut b = Builder { text, caret, lines: Vec::new() };
    b.blocks(&blocks);
    if b.lines.is_empty() {
        let at = edit::empty_view_caret(text, caret.unwrap_or(text.len()));
        b.lines.push(ViewLine { line: Line::empty(at), chars: Vec::new(), cell: None, phantom: true });
    }
    Shown { blocks, lines: b.lines }
}

struct Builder<'a> {
    text: &'a str,
    caret: Option<usize>,
    lines: Vec<ViewLine>,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decoration(text: &str, at: usize) -> Inline {
    Inline { kind: InlineKind::Text { text: text.to_owned(), style: Style::default() }, range: at..at }
}

fn block_end(text: &str, block: &Block) -> usize {
    const BEGIN: &str = "\\begin{document}";
    match &block.kind {
        BlockKind::Heading { label: Some(label), .. } => block.range.end.max(label.range.end),
        BlockKind::Preamble if text.get(block.range.end..).is_some_and(|s| s.starts_with(BEGIN)) => block.range.end + BEGIN.len(),
        _ => block.range.end,
    }
}

fn para(b: &Block) -> bool {
    matches!(b.kind, BlockKind::Paragraph(_))
}

/// A run of a line: its characters and what it stands for.
struct Piece {
    shown: String,
    part: Part,
}

impl Builder<'_> {
    fn blocks(&mut self, blocks: &[Block]) {
        let text = self.text;
        if let Some(first) = blocks.first().filter(|b| !matches!(b.kind, BlockKind::Preamble)) {
            let start = edit::invisible_before(text, first.range.start);
            self.phantom(Between { prev: start, next: first.range.start, prev_paragraph: false, next_paragraph: para(first) });
        }
        for (i, block) in blocks.iter().enumerate() {
            if let Some(prev) = i.checked_sub(1).and_then(|p| blocks.get(p)) {
                self.phantom(Between {
                    prev: block_end(text, prev),
                    next: block.range.start,
                    prev_paragraph: para(prev),
                    next_paragraph: para(block),
                });
            }
            self.block(block);
        }
        if let Some(last) = blocks.last() {
            let end = block_end(text, last);
            let next = end + edit::invisible_len(text.get(end..).unwrap_or_default());
            self.phantom(Between { prev: end, next, prev_paragraph: para(last), next_paragraph: false });
        }
    }

    fn phantom(&mut self, at: Between) {
        let Some(c) = self.caret.filter(|c| edit::empty_paragraph_at(self.text, *c, at)) else { return };
        if self.lines.iter().any(|l| l.phantom) {
            return;
        }
        self.lines.push(ViewLine { line: Line::empty(c), chars: Vec::new(), cell: None, phantom: true });
    }

    fn block(&mut self, block: &Block) {
        let range = &block.range;
        match &block.kind {
            BlockKind::Title { title, author, date, slots } => {
                let caret = self.caret;
                let here = |part: &Vec<Inline>, slot: &Option<Range<usize>>| {
                    caret.filter(|c| part.is_empty() && slot.as_ref().is_some_and(|s| s.start <= *c && *c <= s.end))
                };
                let (title_at, author_at) = (here(title, &slots[0]), here(author, &slots[1]));
                if title.is_empty() && author.is_empty() && title_at.is_none() && author_at.is_none() {
                    self.paragraph(&[Inline { kind: InlineKind::Source, range: range.clone() }], range, None, None);
                }
                let date_at = here(date, &slots[2]);
                for (part, at) in [(title, title_at), (author, author_at), (date, date_at)] {
                    if !part.is_empty() || at.is_some() {
                        self.paragraph(part, range, at, None);
                    }
                }
            }
            BlockKind::Heading { content, label, .. } => {
                let mut inlines = content.clone();
                if let Some(label) = label {
                    inlines.push(decoration("\u{2002}", label.range.start));
                    inlines.push(label.clone());
                }
                let empty_at = if content.is_empty() { view::heading_slot(self.text, range) } else { None };
                self.paragraph(&inlines, range, empty_at, None);
            }
            BlockKind::Paragraph(inlines) => self.paragraph(inlines, range, None, None),
            BlockKind::List { items, .. } => items.iter().for_each(|item| self.item(item)),
            BlockKind::Container { blocks, .. } | BlockKind::Theorem { blocks, .. } => {
                match view::body_start(self.text, range).filter(|_| blocks.is_empty()) {
                    Some(at) => self.blocks(&[Block { kind: BlockKind::Paragraph(Vec::new()), range: at..at }]),
                    None => self.blocks(blocks),
                }
            }
            BlockKind::FrontMatter(fm) => self.front_matter(fm),
            BlockKind::Figure { caption, label, .. } => {
                self.caption(range, "Figure. ", caption, label.as_ref());
            }
            BlockKind::Table(tb) => {
                if let Some(f) = tb.float.as_ref().filter(|f| f.caption_above) {
                    self.caption(range, "Table. ", &f.caption, f.label.as_ref());
                }
                self.grid(&tb.table, &tb.cells);
                if let Some(f) = tb.float.as_ref().filter(|f| !f.caption_above) {
                    self.caption(range, "Table. ", &f.caption, f.label.as_ref());
                }
            }
            BlockKind::Preamble | BlockKind::Include { .. } | BlockKind::Formula { .. } | BlockKind::UnsupportedTable(_) | BlockKind::Source => {}
        }
    }

    fn caption_slot(&self, range: &Range<usize>) -> Option<usize> {
        let src = self.text.get(range.clone())?;
        let at = src.find("\\caption{")? + "\\caption{".len();
        src.get(at..)?.starts_with('}').then_some(range.start + at)
    }

    fn caption(&mut self, range: &Range<usize>, name: &str, caption: &[Inline], label: Option<&Inline>) {
        let empty_at = if caption.is_empty() { self.caption_slot(range) } else { None };
        let mut inlines = vec![decoration(name, range.start)];
        inlines.extend(caption.iter().cloned());
        if let Some(label) = label {
            inlines.push(decoration("\u{2002}", label.range.start));
            inlines.push(label.clone());
        }
        self.paragraph(&inlines, range, empty_at, None);
        // No caption to type in: not a place for the caret.
        if caption.is_empty()
            && empty_at.is_none()
            && let Some(l) = self.lines.last_mut()
        {
            l.line = Line::new(l.line.parts().iter().map(|p| Part::Decoration(p.chars())).collect());
        }
    }

    fn grid(&mut self, table: &Table, cells: &[Vec<Vec<Inline>>]) {
        for (r, (row, inlines)) in table.rows.iter().zip(cells).enumerate() {
            for (cell, inlines) in row.cells.iter().zip(inlines) {
                let at = (table.range.start, r, cell.column);
                self.paragraph(inlines, &cell.content, Some(cell.content.start), Some(at));
            }
        }
    }

    fn item(&mut self, item: &Item) {
        let mut blocks = item.blocks.clone();
        if let Some(label) = &item.label {
            let space = decoration("\u{2002}", item.range.start);
            match blocks.first_mut() {
                Some(Block { kind: BlockKind::Paragraph(p), .. }) => {
                    let rest = std::mem::take(p);
                    *p = label.iter().cloned().chain([space]).chain(rest).collect();
                }
                _ => blocks.insert(0, Block { kind: BlockKind::Paragraph(label.clone()), range: item.range.clone() }),
            }
        }
        if blocks.is_empty() {
            let start = item.range.start + "\\item".len();
            let at = if self.text.get(start..start + 1) == Some(" ") { start + 1 } else { start };
            blocks.push(Block { kind: BlockKind::Paragraph(Vec::new()), range: at..at.max(item.range.end) });
        }
        self.blocks(&blocks);
    }

    fn front_matter(&mut self, fm: &FrontMatter) {
        let span = |inlines: &[Inline]| {
            let start = inlines.iter().map(|i| i.range.start).min().unwrap_or(0);
            start..inlines.iter().map(|i| i.range.end).max().unwrap_or(0).max(start)
        };
        if !fm.title.is_empty() {
            let range = span(&fm.title);
            let mut inlines = fm.title.clone();
            if !fm.title_notes.is_empty() {
                inlines.push(decoration("*", range.end));
            }
            self.paragraph(&inlines, &range, None, None);
        }
        for author in &fm.authors {
            let range = span(&author.name);
            let mut inlines = author.name.clone();
            if !author.affiliations.is_empty() || !author.notes.is_empty() {
                inlines.push(decoration("1", range.end));
            }
            for email in &author.emails {
                let at = email.iter().map(|i| i.range.start).min().unwrap_or(range.end);
                inlines.push(decoration("\u{2003}", at));
                inlines.extend(email.iter().cloned());
            }
            self.paragraph(&inlines, &range, None, None);
        }
        let numbered = fm.affiliations.len() > 1 || fm.affiliations.iter().any(|a| !a.key.is_empty());
        for (i, a) in fm.affiliations.iter().enumerate() {
            let range = span(&a.text);
            let mut inlines = Vec::new();
            if numbered {
                inlines.push(decoration(&format!("{}\u{2009}", i + 1), range.start));
            }
            inlines.extend(a.text.iter().cloned());
            self.paragraph(&inlines, &range, None, None);
        }
        for note in &fm.notes {
            let range = span(&note.text);
            let mut inlines = vec![decoration("*\u{2009}", range.start)];
            inlines.extend(note.text.iter().cloned());
            self.paragraph(&inlines, &range, None, None);
        }
        self.blocks(&fm.blocks);
    }

    fn pieces(&self, inline: &Inline, out: &mut Vec<Piece>) {
        let range = inline.range.clone();
        let atom = |shown: String| {
            let chars = shown.chars().count();
            Piece { shown, part: Part::Atom { chars, range: range.clone() } }
        };
        let piece = match &inline.kind {
            InlineKind::Text { text, style } => {
                let shown = if style.small_caps { text.to_uppercase() } else { text.clone() };
                let offsets = edit::offsets(self.text, range.clone(), text).filter(|o| o.len() == shown.chars().count() + 1);
                let chars = shown.chars().count();
                let part = match offsets {
                    Some(o) => Part::Text(o),
                    None if range.is_empty() => Part::Decoration(chars),
                    None => Part::Atom { chars, range: range.clone() },
                };
                Piece { shown, part }
            }
            InlineKind::LineBreak => atom("\n".to_owned()),
            InlineKind::Formula { source, .. } => atom(format!("\u{a0}{}\u{a0}", collapse(self.text.get(source.clone()).unwrap_or_default()))),
            InlineKind::Chip { kind, key, .. } => {
                let shown = match kind {
                    ChipKind::Label => "\u{a0}".repeat(5),
                    ChipKind::File => format!("{}{}", "\u{a0}".repeat(5), key.replace(' ', "\u{a0}")),
                    ChipKind::Reference | ChipKind::Citation => format!("{}\u{a0}?", key.replace(' ', "\u{a0}")),
                };
                atom(format!("\u{a0}{shown}\u{a0}"))
            }
            InlineKind::Footnote(_) => atom("\u{200a}1".to_owned()),
            InlineKind::Link { text, .. } => {
                let mut inner = Vec::new();
                text.iter().for_each(|i| self.pieces(i, &mut inner));
                atom(inner.into_iter().map(|p| p.shown).collect())
            }
            InlineKind::Source => atom(format!("\u{a0}{}\u{a0}", collapse(self.text.get(range.clone()).unwrap_or_default()))),
        };
        // The window lays out nothing for an empty run.
        if !piece.shown.is_empty() {
            out.push(piece);
        }
    }

    fn paragraph(&mut self, inlines: &[Inline], range: &Range<usize>, empty_at: Option<usize>, cell: Option<(usize, usize, usize)>) {
        let mut pieces = Vec::new();
        inlines.iter().for_each(|i| self.pieces(i, &mut pieces));
        let chars: Vec<char> = pieces.iter().flat_map(|p| p.shown.chars()).collect();
        let mut line =
            if pieces.is_empty() { Line::empty(empty_at.unwrap_or(range.start)) } else { Line::new(pieces.into_iter().map(|p| p.part).collect()) };
        if let Some(at) = empty_at.filter(|_| !line.parts().iter().any(|p| matches!(p, Part::Text(_)))) {
            let mut parts = line.parts().to_vec();
            let i = parts.iter().position(|p| matches!(p, Part::Atom { range, .. } if range.start >= at)).unwrap_or(parts.len());
            parts.insert(i, Part::Text(vec![at]));
            line = Line::new(parts);
        }
        self.lines.push(ViewLine { line, chars, cell, phantom: false });
    }
}

/// The stops of `line`, in order.
pub fn stops(line: &Line) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = line.first_stop();
    while let Some(v) = at {
        out.push(v);
        at = line.next_stop(v);
    }
    out
}
