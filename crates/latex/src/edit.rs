//! Editing in the formatted view (docs/ROADMAP.md, F3): where a caret in the view stands in the
//! source, and the source edits its keys make. Every edit is a minimal edit of the source: typed
//! text goes in at the caret's source offset with LaTeX's special characters escaped, a deletion
//! removes the source of what it deletes and nothing else, and a structure (a style, a heading, a
//! list) changes by its command alone.
//!
//! A paragraph of the view is a [`Line`] of [`Part`]s: text, each of whose characters stands for a
//! span of the source; atoms (chips, formulas, what is shown as its source), one caret stop on each
//! side; and decorations, which the view adds and the caret skips. Where two runs meet the caret
//! stands at the end of the first, so typing at a style boundary continues the style before it.

use std::ops::Range;

use crate::tree::{self, Arg, Kind, Node};
use crate::view::{self, Block, BlockKind, Style};

/// What a run of a line of the view stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Text whose character `i` stands for the source bytes `offsets[i]..offsets[i + 1]`.
    Text(Vec<usize>),
    /// Shown as `chars` characters, edited as a whole: a caret stop on each side.
    Atom { chars: usize, range: Range<usize> },
    /// Shown by the view alone (a gap before a label, "Figure."): no caret stop inside it.
    Decoration(usize),
}

impl Part {
    /// How many characters it shows.
    pub fn chars(&self) -> usize {
        match self {
            Part::Text(offsets) => offsets.len().saturating_sub(1),
            Part::Atom { chars, .. } => *chars,
            Part::Decoration(chars) => *chars,
        }
    }
}

/// A line of the view (a paragraph, a heading, an item's text) and its caret stops.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    parts: Vec<Part>,
    /// The caret stops: positions in the line (chars) with the source offset each stands for, in
    /// order.
    stops: Vec<(usize, usize)>,
}

impl Line {
    pub fn new(parts: Vec<Part>) -> Self {
        let mut stops: Vec<(usize, usize)> = Vec::new();
        let mut at = 0;
        // Where two parts meet, the first part's end wins: the caret is in the style before it.
        let mut push = |v: usize, s: usize| {
            if stops.last().is_none_or(|(last, _)| *last < v) {
                stops.push((v, s));
            }
        };
        for part in &parts {
            match part {
                Part::Text(offsets) => {
                    for (k, s) in offsets.iter().enumerate() {
                        push(at + k, *s);
                    }
                }
                Part::Atom { chars, range } => {
                    push(at, range.start);
                    push(at + chars, range.end);
                }
                Part::Decoration(_) => {}
            }
            at += part.chars();
        }
        Line { parts, stops }
    }

    /// An empty line whose caret stands at source offset `at`.
    pub fn empty(at: usize) -> Self {
        Line::new(vec![Part::Text(vec![at])])
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// How many characters it shows.
    pub fn len(&self) -> usize {
        self.parts.iter().map(Part::chars).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_stop(&self, v: usize) -> bool {
        self.stops.iter().any(|(s, _)| *s == v)
    }

    pub fn first_stop(&self) -> Option<usize> {
        self.stops.first().map(|s| s.0)
    }

    pub fn last_stop(&self) -> Option<usize> {
        self.stops.last().map(|s| s.0)
    }

    pub fn next_stop(&self, v: usize) -> Option<usize> {
        self.stops.iter().map(|s| s.0).find(|s| *s > v)
    }

    pub fn prev_stop(&self, v: usize) -> Option<usize> {
        self.stops.iter().map(|s| s.0).rev().find(|s| *s < v)
    }

    /// The stop nearest to `v`.
    pub fn nearest_stop(&self, v: usize) -> Option<usize> {
        self.stops.iter().map(|s| s.0).min_by_key(|s| s.abs_diff(v))
    }

    /// The source offset of the stop nearest to `v`.
    pub fn source_at(&self, v: usize) -> Option<usize> {
        let stop = self.nearest_stop(v)?;
        self.stops.iter().find(|s| s.0 == stop).map(|s| s.1)
    }

    /// The source it spans, from its first stop to its last.
    pub fn span(&self) -> Option<Range<usize>> {
        Some(self.stops.first()?.1..self.stops.last()?.1)
    }

    /// The stop that stands for source offset `byte`: its own, or the first after it (the last
    /// stop for a byte after the line).
    pub fn view_at(&self, byte: usize) -> Option<usize> {
        if let Some(s) = self.stops.iter().find(|s| s.1 == byte) {
            return Some(s.0);
        }
        self.stops.iter().find(|s| s.1 > byte).or(self.stops.last()).map(|s| s.0)
    }

    /// The parts with the position each starts at.
    fn placed(&self) -> impl Iterator<Item = (usize, &Part)> {
        self.parts.iter().scan(0, |at, p| {
            let start = *at;
            *at += p.chars();
            Some((start, p))
        })
    }

    /// The source of the atom that ends at `v`.
    pub fn atom_before(&self, v: usize) -> Option<Range<usize>> {
        self.placed().find_map(|(s, p)| match p {
            Part::Atom { chars, range } if s + chars == v && *chars > 0 => Some(range.clone()),
            _ => None,
        })
    }

    /// The source of the atom that starts at `v`.
    pub fn atom_after(&self, v: usize) -> Option<Range<usize>> {
        self.placed().find_map(|(s, p)| match p {
            Part::Atom { chars, range } if s == v && *chars > 0 => Some(range.clone()),
            _ => None,
        })
    }

    /// The source of the character of text that ends at `v`.
    pub fn char_before(&self, v: usize) -> Option<Range<usize>> {
        let v = v.checked_sub(1)?;
        self.char_after(v)
    }

    /// The source of the character of text that starts at `v`.
    pub fn char_after(&self, v: usize) -> Option<Range<usize>> {
        self.placed().find_map(|(s, p)| match p {
            Part::Text(offsets) if s <= v && v < s + p.chars() => Some(*offsets.get(v - s)?..*offsets.get(v - s + 1)?),
            _ => None,
        })
    }

    /// The source of what shows between `a` and `b`: the text's spans, and whole atoms.
    pub fn pieces(&self, a: usize, b: usize) -> Vec<Range<usize>> {
        let (a, b) = (a.min(b), a.max(b));
        let mut out = Vec::new();
        for (s, p) in self.placed() {
            let e = s + p.chars();
            match p {
                Part::Text(offsets) => {
                    let (from, to) = (a.max(s), b.min(e));
                    if from < to
                        && let (Some(x), Some(y)) = (offsets.get(from - s), offsets.get(to - s))
                    {
                        out.push(*x..*y);
                    }
                }
                Part::Atom { range, .. } if a < e && s < b => out.push(range.clone()),
                _ => {}
            }
        }
        out
    }
}

/// The lines of the view, in the order it shows them.
#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub lines: Vec<Line>,
}

/// A caret position in a [`Doc`]: a line and a stop in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub at: usize,
}

impl Doc {
    /// Where source offset `byte` is shown: in line `hint` if it spans it, else in the first line
    /// that does, else at the start of the first line after it.
    pub fn locate(&self, byte: usize, hint: Option<usize>) -> Option<Pos> {
        let spans = |i: usize| self.lines.get(i).and_then(Line::span).is_some_and(|s| s.start <= byte && byte <= s.end);
        let line = hint.filter(|h| spans(*h)).or_else(|| (0..self.lines.len()).find(|i| spans(*i)));
        let line = line.or_else(|| {
            let after = (0..self.lines.len()).filter(|i| self.lines.get(*i).and_then(Line::span).is_some_and(|s| s.start > byte));
            after.min_by_key(|i| self.lines.get(*i).and_then(Line::span).map_or(usize::MAX, |s| s.start))
        });
        let line = line.or_else(|| self.lines.len().checked_sub(1))?;
        let at = self.lines.get(line)?.view_at(byte)?;
        Some(Pos { line, at })
    }

    pub fn source(&self, p: Pos) -> Option<usize> {
        self.lines.get(p.line)?.source_at(p.at)
    }

    /// The source between line `i` and the next, when it is only white space: a paragraph break
    /// that deleting joins the two.
    pub fn gap_after(&self, text: &str, i: usize) -> Option<Range<usize>> {
        let a = self.lines.get(i)?.span()?.end;
        let b = self.lines.get(i + 1)?.span()?.start;
        (a <= b && text.get(a..b)?.trim().is_empty()).then_some(a..b)
    }

    /// The source of what shows from `a` to `b`, sorted and merged: the text's spans, whole atoms,
    /// and the paragraph breaks between lines that are only white space.
    pub fn pieces(&self, text: &str, a: Pos, b: Pos) -> Vec<Range<usize>> {
        let (a, b) = (a.min(b), a.max(b));
        let mut out = Vec::new();
        for i in a.line..=b.line {
            let Some(line) = self.lines.get(i) else { break };
            let from = if i == a.line { a.at } else { 0 };
            let to = if i == b.line { b.at } else { line.len() };
            out.extend(line.pieces(from, to));
            if i < b.line
                && let Some(gap) = self.gap_after(text, i)
            {
                out.push(gap);
            }
        }
        merge(out)
    }
}

/// Ranges sorted, with those that touch or overlap made one.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// The source offsets of the characters of `shown`, a text run of the view standing for
/// `text[range]`: `shown.chars().count() + 1` offsets, character `i` standing for the bytes from
/// offset `i` to offset `i + 1`. `None` when the run does not read as its source (the caller
/// then edits it as a whole).
pub fn offsets(text: &str, range: Range<usize>, shown: &str) -> Option<Vec<usize>> {
    let src = text.get(range.clone())?;
    let base = range.start;
    let chars: Vec<char> = shown.chars().collect();
    let mut out = Vec::with_capacity(chars.len() + 1);
    let mut i = 0;
    let mut k = 0;
    while let Some(&c) = chars.get(k) {
        let rest = src.get(i..)?;
        let first = rest.chars().next()?;
        if first.is_whitespace() {
            let run = rest.len() - rest.trim_start().len();
            if c == ' ' {
                out.push(base + i);
                k += 1;
            }
            i += run;
            continue;
        }
        if first == '%' {
            // A comment, its line end and the next line's indentation show nothing.
            let line = rest.find('\n').map_or(rest.len(), |n| n + 1);
            let after = rest.get(line..).unwrap_or_default();
            i += line + (after.len() - after.trim_start().len());
            continue;
        }
        if first == '\\' {
            let after = rest.get(1..).unwrap_or_default();
            let letters = after.len() - after.trim_start_matches(|c: char| c.is_ascii_alphabetic()).len();
            if letters > 0 {
                // A command standing for a fixed text: its characters are edited as a whole.
                let word = view::logo(after.get(..letters)?)?;
                let n = word.chars().count();
                if chars.get(k..k + n)?.iter().copied().ne(word.chars()) {
                    return None;
                }
                out.extend(std::iter::repeat_n(base + i, n));
                i += 1 + letters;
                k += n;
                continue;
            }
            let symbol_len = 1 + after.chars().next().map_or(0, char::len_utf8);
            let symbol = rest.get(..symbol_len)?;
            let shown_as = view::control_symbol(symbol)?;
            if !shown_as.is_empty() {
                if shown_as.chars().ne(std::iter::once(c)) {
                    return None;
                }
                out.push(base + i);
                k += 1;
            }
            i += symbol_len;
            continue;
        }
        let consumed = match (c, first) {
            ('\u{a0}', '~') => 1,
            ('—', '-') if rest.starts_with("---") => 3,
            ('–', '-') if rest.starts_with("--") => 2,
            ('“', '`') if rest.starts_with("``") => 2,
            ('”', '\'') if rest.starts_with("''") => 2,
            ('‘', '`') | ('’', '\'') => 1,
            _ if c == first => first.len_utf8(),
            _ => return None,
        };
        out.push(base + i);
        i += consumed;
        k += 1;
    }
    out.push(base + i);
    Some(out)
}

/// Typed text as LaTeX source: the special characters in their safe forms.
pub fn escape(typed: &str) -> String {
    let mut out = String::with_capacity(typed.len());
    for c in typed.chars() {
        match c {
            '#' | '$' | '%' | '&' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// A change of the source: `range` replaced by `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub range: Range<usize>,
    pub text: String,
}

/// Changes that do not overlap, in order, and where the caret goes (in the text after them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEdit {
    pub changes: Vec<Change>,
    pub caret: usize,
}

fn change(range: Range<usize>, text: impl Into<String>) -> Change {
    Change { range, text: text.into() }
}

/// What encloses a source offset, innermost last.
enum Frame<'a> {
    /// In an argument of a command.
    Command {
        node: &'a Node,
        name: &'a str,
        arg: &'a Arg,
    },
    Group(&'a Node),
    Environment {
        node: &'a Node,
        name: &'a str,
        body: &'a [Node],
    },
    Math,
}

/// The frames around `at` in `nodes`.
fn path<'a>(nodes: &'a [Node], at: usize, out: &mut Vec<Frame<'a>>) {
    for n in nodes {
        if !(n.range.start < at && at < n.range.end) {
            continue;
        }
        match &n.kind {
            Kind::Command { name, args, .. } => {
                if let Some(arg) = args.iter().find(|a| a.inner.start <= at && at <= a.inner.end) {
                    out.push(Frame::Command { node: n, name, arg });
                    path(&arg.body, at, out);
                }
            }
            Kind::Group { body, .. } => {
                out.push(Frame::Group(n));
                path(body, at, out);
            }
            Kind::Environment { name, begin, body, end, .. } => {
                let body_end = end.as_ref().map_or(n.range.end, |e| e.start);
                if begin.end <= at && at <= body_end {
                    out.push(Frame::Environment { node: n, name, body });
                    path(body, at, out);
                }
            }
            Kind::Math { .. } | Kind::Verbatim { .. } => out.push(Frame::Math),
            _ => {}
        }
        return;
    }
}

/// Where a line break typed at a source offset goes.
enum Context<'a> {
    Paragraph,
    Heading(&'a Node),
    /// In a list: its body.
    Item(&'a [Node]),
}

/// The context of `at` and the styled groups around it there, outermost first: their nodes and the
/// source that opens each (`\textbf{`).
fn context<'a>(text: &str, frames: &[Frame<'a>]) -> Option<(Context<'a>, Vec<(&'a Node, String)>)> {
    let mut styles = Vec::new();
    let mut found = Context::Paragraph;
    for f in frames.iter().rev() {
        match f {
            Frame::Math => return None,
            Frame::Command { node, name, .. } if view::heading_level(name).is_some() => {
                found = Context::Heading(node);
                break;
            }
            Frame::Command { node, name, arg } if view::styled(name, Style::default()).is_some() => {
                styles.push((*node, text.get(node.range.start..arg.inner.start)?.to_owned()));
            }
            Frame::Command { .. } => return None,
            Frame::Group(node) => styles.push((*node, "{".to_owned())),
            Frame::Environment { name, body, .. } if matches!(*name, "itemize" | "enumerate" | "description") => {
                found = Context::Item(body);
                break;
            }
            Frame::Environment { name, .. } if *name == "document" || view::CONTAINERS.contains(name) => break,
            Frame::Environment { .. } => return None,
        }
    }
    styles.reverse();
    Some((found, styles))
}

/// The paragraph of `blocks` (at any depth) whose source holds `at`.
fn paragraph_at(blocks: &[Block], at: usize) -> Option<&Block> {
    for b in blocks {
        match &b.kind {
            BlockKind::Paragraph(_) if b.range.start <= at && at <= b.range.end => return Some(b),
            BlockKind::Container { blocks, .. } => {
                if let Some(p) = paragraph_at(blocks, at) {
                    return Some(p);
                }
            }
            BlockKind::List { items, .. } => {
                for item in items {
                    if let Some(p) = paragraph_at(&item.blocks, at) {
                        return Some(p);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The white space around `at` (with line ends when `lines`).
fn space_around(text: &str, at: usize, lines: bool) -> Range<usize> {
    let is_space = |c: char| c == ' ' || c == '\t' || (lines && (c == '\n' || c == '\r'));
    let before = text.get(..at).unwrap_or_default();
    let after = text.get(at..).unwrap_or_default();
    let start = at - (before.len() - before.trim_end_matches(is_space).len());
    let end = at + (after.len() - after.trim_start_matches(is_space).len());
    start..end
}

/// An empty paragraph at `pos`, between blank lines, with the caret in it.
fn open_paragraph(text: &str, pos: usize) -> SourceEdit {
    let before = text.get(..pos).unwrap_or_default();
    let after = text.get(pos..).unwrap_or_default();
    let lead = &before[before.trim_end().len()..];
    let trail = &after[..after.len() - after.trim_start().len()];
    let need_before = 2usize.saturating_sub(lead.matches('\n').count());
    let need_after = if after.trim().is_empty() { 0 } else { 2usize.saturating_sub(trail.matches('\n').count()) };
    let inserted = format!("{}{}", "\n".repeat(need_before), "\n".repeat(need_after));
    SourceEdit { changes: vec![change(pos..pos, inserted)], caret: pos + need_before }
}

/// The edit Enter makes at source offset `at`: in a paragraph, a paragraph break (the styles open
/// at the caret closed before it and opened again after it); at a paragraph's or a heading's end,
/// an empty paragraph after it; in a list, a new item. `None` where a line break cannot go (a
/// caption, a formula, a footnote).
pub fn enter(text: &str, at: usize) -> Option<SourceEdit> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let (found, styles) = context(text, &frames)?;
    let closes = "}".repeat(styles.len());
    let opens: String = styles.iter().map(|(_, open)| open.as_str()).collect();
    match found {
        Context::Heading(node) => {
            let blocks = view::build(text, &tree);
            // After its label, when the view shows the label with it.
            let end = heading_end(&blocks, node.range.clone()).unwrap_or(node.range.end);
            Some(open_paragraph(text, end))
        }
        Context::Item(body) => {
            let item = body.iter().rev().find(|n| matches!(&n.kind, Kind::Command { name, .. } if name == "item") && n.range.start < at)?;
            let line_start = text.get(..item.range.start)?.rfind('\n').map_or(0, |n| n + 1);
            let indent = text.get(line_start..item.range.start).filter(|s| s.trim().is_empty()).unwrap_or_default();
            let space = space_around(text, at, false);
            let inserted = format!("{closes}\n{indent}\\item {opens}");
            let caret = space.start + inserted.len();
            Some(SourceEdit { changes: vec![change(space, inserted)], caret })
        }
        Context::Paragraph => {
            let blocks = view::build(text, &tree);
            let Some(para) = paragraph_at(&blocks, at) else {
                // An empty paragraph (only the caret shows it): another after it.
                return Some(SourceEdit { changes: vec![change(at..at, "\n\n")], caret: at + 2 });
            };
            let outer = styles.first().map(|(n, _)| n.range.clone());
            if at >= para.range.end {
                return Some(open_paragraph(text, outer.map_or(at, |r| r.end)));
            }
            if at <= para.range.start {
                return Some(open_paragraph(text, outer.map_or(at, |r| r.start)));
            }
            let space = space_around(text, at, true);
            let inserted = format!("{closes}\n\n{opens}");
            let caret = space.start + inserted.len();
            Some(SourceEdit { changes: vec![change(space, inserted)], caret })
        }
    }
}

/// Where a heading ends in the view: after the label shown with it.
fn heading_end(blocks: &[Block], range: Range<usize>) -> Option<usize> {
    blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Heading { label, .. } if b.range == range => Some(label.as_ref().map_or(b.range.end, |l| l.range.end)),
        _ => None,
    })
}

/// Pasted plain text as source at `at`: escaped, its line ends kept in a paragraph and made spaces
/// where a paragraph break cannot go (a style, a heading, a caption).
pub fn paste(text: &str, at: usize, pasted: &str) -> String {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let plain = matches!(context(text, &frames), Some((Context::Paragraph | Context::Item(_), styles)) if styles.is_empty());
    let pasted = pasted.replace("\r\n", "\n");
    if plain { escape(&pasted) } else { escape(&pasted.split_whitespace().collect::<Vec<_>>().join(" ")) }
}

/// The command `name` around source span `r` whose argument is exactly `r`, at any depth.
fn wrapper<'a>(nodes: &'a [Node], name: &str, r: &Range<usize>) -> Option<(&'a Node, &'a Arg)> {
    for n in nodes {
        if n.range.end < r.start || n.range.start > r.end {
            continue;
        }
        let found = match &n.kind {
            Kind::Command { name: m, args, .. } => {
                if let Some(arg) = args.iter().find(|a| !a.optional && m == name && a.inner == *r) {
                    return Some((n, arg));
                }
                args.iter().find_map(|a| wrapper(&a.body, name, r))
            }
            Kind::Group { body, .. } | Kind::Environment { body, .. } => wrapper(body, name, r),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Whether span `r` is inside an argument of command `name`.
fn inside(nodes: &[Node], name: &str, r: &Range<usize>) -> bool {
    let mut frames = Vec::new();
    path(nodes, r.start, &mut frames);
    frames.iter().any(|f| match f {
        Frame::Command { name: m, arg, .. } => *m == name && arg.inner.start <= r.start && r.end <= arg.inner.end,
        _ => false,
    })
}

/// A style command (`textbf`, `textit`) on the source spans `pieces` (as [`Doc::pieces`] gives):
/// when each is exactly the argument of that command, the commands are removed; otherwise each
/// span not already in one is wrapped in it.
pub fn toggle_style(text: &str, pieces: &[Range<usize>], name: &str) -> Vec<Change> {
    let tree = tree::parse(text);
    let pieces: Vec<&Range<usize>> = pieces.iter().filter(|r| r.start < r.end).collect();
    let wrapped: Vec<Option<(&Node, &Arg)>> = pieces.iter().map(|r| wrapper(&tree, name, r)).collect();
    let mut out = Vec::new();
    if !wrapped.is_empty() && wrapped.iter().all(Option::is_some) {
        for (node, arg) in wrapped.into_iter().flatten() {
            out.push(change(node.range.start..arg.inner.start, ""));
            out.push(change(arg.inner.end..node.range.end, ""));
        }
    } else {
        for r in pieces {
            if inside(&tree, name, r) {
                continue;
            }
            out.push(change(r.start..r.start, format!("\\{name}{{")));
            out.push(change(r.end..r.end, "}"));
        }
    }
    out.sort_by_key(|c| (c.range.start, c.range.end));
    out
}

/// A heading command (`section`) at source offset `at`: a heading there takes that command's
/// name (or becomes a paragraph again when it has it already); a paragraph there becomes such a
/// heading. `None` elsewhere.
pub fn set_heading(text: &str, at: usize, name: &str) -> Option<Vec<Change>> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let heading = frames.iter().rev().find_map(|f| match f {
        Frame::Command { node, name, arg } if view::heading_level(name).is_some() => Some((*node, *name, *arg)),
        _ => None,
    });
    if let Some((node, old, arg)) = heading {
        let name_start = node.range.start + 1;
        if old == name {
            return Some(vec![change(node.range.start..arg.inner.start, ""), change(arg.inner.end..node.range.end, "")]);
        }
        return Some(vec![change(name_start..name_start + old.len(), name)]);
    }
    let blocks = view::build(text, &tree);
    let para = paragraph_at(&blocks, at)?;
    Some(vec![change(para.range.start..para.range.start, format!("\\{name}{{")), change(para.range.end..para.range.end, "}")])
}

/// A list environment (`itemize`, `enumerate`) at source offset `at`: a list of another kind there
/// is renamed; a paragraph there becomes the list's one item. `None` in a list of that kind already
/// or outside a paragraph.
pub fn set_list(text: &str, at: usize, name: &str) -> Option<Vec<Change>> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let list = frames.iter().rev().find_map(|f| match f {
        Frame::Environment { node, name, .. } if matches!(*name, "itemize" | "enumerate" | "description") => Some((*node, *name)),
        _ => None,
    });
    if let Some((node, old)) = list {
        if old == name {
            return None;
        }
        let Kind::Environment { begin, end, .. } = &node.kind else { return None };
        let mut out = Vec::new();
        for r in std::iter::once(begin).chain(end.as_ref()) {
            let src = text.get(r.clone())?;
            let at = src.find(old)?;
            out.push(change(r.start + at..r.start + at + old.len(), name));
        }
        return Some(out);
    }
    let blocks = view::build(text, &tree);
    let para = paragraph_at(&blocks, at)?;
    Some(vec![
        change(para.range.start..para.range.start, format!("\\begin{{{name}}}\n  \\item ")),
        change(para.range.end..para.range.end, format!("\n\\end{{{name}}}")),
    ])
}

/// `changes` (in order, not overlapping) made on `text`.
pub fn apply(text: &str, changes: &[Change]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for c in changes {
        out.push_str(text.get(at..c.range.start).unwrap_or_default());
        out.push_str(&c.text);
        at = c.range.end;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    out
}
