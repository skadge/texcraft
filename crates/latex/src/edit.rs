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

    /// The stop that stands for source offset `byte`: its own, else the last before it (a byte
    /// in markup the view hides, such as just inside `\textbf{`, shows where that markup is), the
    /// first stop for a byte before the line.
    pub fn view_at(&self, byte: usize) -> Option<usize> {
        if let Some(s) = self.stops.iter().find(|s| s.1 == byte) {
            return Some(s.0);
        }
        self.stops.iter().rev().find(|s| s.1 < byte).or(self.stops.first()).map(|s| s.0)
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

    /// The source deleting the selection from `a` to `b` removes: [`Self::pieces`], and across
    /// lines the structure of the lists it spans ([`deletion`]).
    pub fn deleted(&self, text: &str, a: Pos, b: Pos) -> Vec<Range<usize>> {
        let (a, b) = (a.min(b), a.max(b));
        let pieces = self.pieces(text, a, b);
        if a.line == b.line {
            return pieces;
        }
        match (self.source(a), self.source(b)) {
            (Some(sa), Some(sb)) if sa < sb => deletion(text, pieces, sa..sb),
            _ => pieces,
        }
    }
}

/// The source a deletion of the view's selection removes, from the `pieces` of what it shows
/// ([`Doc::pieces`]) and the source `sel` from its start to its end, when it spans lines: the
/// `\item`s of the items it holds whole go with their text (the list's environment too when it
/// holds them all), and when it starts and ends in two items of one list the source between goes,
/// so what is left of the second item joins the first. Sorted and merged.
pub fn deletion(text: &str, pieces: Vec<Range<usize>>, sel: Range<usize>) -> Vec<Range<usize>> {
    let tree = tree::parse(text);
    let mut out = pieces;
    whole_items(text, &tree, &sel, &mut out);
    // From one item to another of the same list, with nothing but the list around both ends: what
    // is between them goes, balanced as it is.
    let (mut fa, mut fb) = (Vec::new(), Vec::new());
    path(&tree, sel.start, &mut fa);
    path(&tree, sel.end, &mut fb);
    let innermost = |frames: &[Frame<'_>]| match frames.last() {
        Some(Frame::Environment { node, name, .. }) if is_list(name) => Some(node.range.clone()),
        _ => None,
    };
    if let (Some(la), Some(lb)) = (list_at(&fa, sel.start), list_at(&fb, sel.end))
        && innermost(&fa) == Some(la.node.range.clone())
        && innermost(&fb) == Some(lb.node.range.clone())
        && la.node.range == lb.node.range
        && la.k < lb.k
        && !la.covers(text, la.k, &sel)
        && !lb.covers(text, lb.k, &sel)
    {
        out.push(sel.clone());
    }
    merge(out)
}

/// The source of the list items (at any depth in `nodes`) that `sel` holds whole: each run of them
/// with its `\item`s, on whole lines; a list whose items it all holds, the whole environment.
fn whole_items(text: &str, nodes: &[Node], sel: &Range<usize>, out: &mut Vec<Range<usize>>) {
    for n in nodes {
        if n.range.end < sel.start || n.range.start > sel.end {
            continue;
        }
        match &n.kind {
            Kind::Environment { name, body, .. } => {
                if is_list(name) {
                    list_items(text, n, name, body, sel, out);
                }
                whole_items(text, body, sel, out);
            }
            Kind::Group { body, .. } => whole_items(text, body, sel, out),
            Kind::Command { args, .. } => args.iter().for_each(|a| whole_items(text, &a.body, sel, out)),
            _ => {}
        }
    }
}

fn list_items<'a>(text: &str, node: &'a Node, name: &'a str, body: &'a [Node], sel: &Range<usize>, out: &mut Vec<Range<usize>>) {
    let is_item = |n: &Node| matches!(&n.kind, Kind::Command { name, .. } if name == "item");
    let items: Vec<usize> = body.iter().enumerate().filter(|(_, n)| is_item(n)).map(|(i, _)| i).collect();
    if items.is_empty() {
        return;
    }
    let list = ListAt { node, name, body, items, k: 0, outer: false };
    let covered: Vec<bool> = (0..list.items.len()).map(|k| list.covers(text, k, sel)).collect();
    if covered.iter().all(|c| *c)
        && let (Some(begin), Some(end)) = (list.begin(), list.end())
    {
        out.push(line_start(text, begin.start)..line_end(text, end.end));
        return;
    }
    let mut k = 0;
    while k < covered.len() {
        if !covered.get(k).copied().unwrap_or(false) {
            k += 1;
            continue;
        }
        let first = k;
        while covered.get(k + 1).copied().unwrap_or(false) {
            k += 1;
        }
        if let (Some(head), Some(next)) = (list.item(first), list.after(k)) {
            out.push(line_start(text, head.range.start)..line_start(text, next));
        }
        k += 1;
    }
}

/// The start of `pos`'s line when only white space is before it there, else `pos`.
fn line_start(text: &str, pos: usize) -> usize {
    pos - indent_at(text, pos).len()
}

/// After the line end that follows `pos` when only white space is after it there, else `pos`.
fn line_end(text: &str, pos: usize) -> usize {
    let after = text.get(pos..).unwrap_or_default();
    let line = after.find('\n').map_or(after.len(), |n| n + 1);
    if after.get(..line).is_some_and(|l| l.trim().is_empty()) { pos + line } else { pos }
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

/// The typographic replacement typing makes, `at` just after what was typed: three dots become
/// `\dots` (shown as an ellipsis). `None` where it does not apply: in math, verbatim, typewriter
/// text, an argument that is not text (a label, an address), after a fourth dot or `\.`.
pub fn ellipsis(text: &str, at: usize) -> Option<Change> {
    let start = at.checked_sub(3)?;
    if text.get(start..at)? != "..." || text.get(..start)?.ends_with(['.', '\\']) {
        return None;
    }
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, start, &mut frames);
    let text_arg = |name: &str| {
        view::heading_level(name).is_some()
            || view::styled(name, Style::default()).is_some()
            || matches!(name, "footnote" | "caption" | "thanks" | "title" | "author" | "date" | "tnotetext" | "cortext" | "fntext")
    };
    let in_text = frames.iter().all(|f| match f {
        Frame::Math => false,
        Frame::Command { name, .. } => text_arg(name),
        Frame::Group(_) | Frame::Environment { .. } => true,
    });
    if !in_text || here(text, start).code {
        return None;
    }
    Some(change(start..at, "\\dots"))
}

/// Whether source offset `at` is just after a control word (`\dots`), whose name a letter typed
/// there would lengthen and after which TeX skips a space.
pub fn after_control_word(text: &str, at: usize) -> bool {
    let Some(before) = text.get(..at) else { return false };
    let name = before.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    if name.len() == before.len() {
        return false;
    }
    let slashes = name.len() - name.trim_end_matches('\\').len();
    slashes % 2 == 1
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
            Frame::Environment { name, .. } if *name == "document" || view::shown_as_text(text, name) => break,
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
            BlockKind::Container { blocks, .. } | BlockKind::Theorem { blocks, .. } => {
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
/// an empty paragraph after it; in a list, a new item, and on an empty item, the item one level
/// out ([`outdent`]: out of the list at the top). `None` where a line break cannot go (a caption, a
/// formula, a footnote).
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
        Context::Item(_) if styles.is_empty() && item_is_empty(text, &frames, at) => outdent(text, at),
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
/// where a paragraph break cannot go (a style, a heading, a caption). In a paragraph, lines that
/// start with a bullet or a number become a bulleted or numbered list.
pub fn paste(text: &str, at: usize, pasted: &str) -> String {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let plain = matches!(context(text, &frames), Some((Context::Paragraph | Context::Item(_), styles)) if styles.is_empty());
    let pasted = pasted.replace("\r\n", "\n");
    if !plain {
        return escape(&pasted.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    if !pasted.contains('\n') {
        return escape(&pasted);
    }
    // Lines that start with a bullet or a number are a list's items.
    let lines: Vec<&str> = pasted.split('\n').collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(line) = lines.get(i) {
        if let Some((name, _)) = list_marker(line) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("\\begin{{{name}}}\n"));
            while let Some((_, item)) = lines.get(i).and_then(|l| list_marker(l)).filter(|(n, _)| *n == name) {
                out.push_str(&format!("  \\item {}\n", escape(item.trim())));
                i += 1;
            }
            out.push_str(&format!("\\end{{{name}}}"));
            if i < lines.len() {
                out.push('\n');
            }
            continue;
        }
        out.push_str(&escape(line));
        if i + 1 < lines.len() {
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// The list a pasted line is an item of (`- `, `* `, `• ` for bullets, `1. ` or `1) ` for
/// numbers), and its text.
fn list_marker(line: &str) -> Option<(&'static str, &str)> {
    let line = line.trim_start();
    for bullet in ["- ", "* ", "• ", "◦ ", "‣ ", "– "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some(("itemize", rest));
        }
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let rest = line.get(digits..)?;
    let rest = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))?;
    (digits > 0).then_some(("enumerate", rest))
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
    let para = trimmed(text, paragraph_at(&blocks, at)?.range.clone());
    Some(vec![change(para.start..para.start, format!("\\{name}{{")), change(para.end..para.end, "}")])
}

/// A list environment (`itemize`, `enumerate`) at source offset `at`: a list of another kind there
/// is renamed; a paragraph there becomes the list's one item; on an empty line between paragraphs
/// (an empty paragraph only the caret shows), a new list with one empty item, the caret in it.
/// `None` in a list of that kind already or outside a paragraph.
pub fn set_list(text: &str, at: usize, name: &str) -> Option<SourceEdit> {
    let changes = list_changes(text, at, name);
    match changes {
        Some(changes) => {
            let caret = map_offset(&changes, at);
            Some(SourceEdit { changes, caret })
        }
        None => new_list(text, at, name),
    }
}

/// A new list on the empty line at `at`, in body text (not in a style, a heading, a list, math).
fn new_list(text: &str, at: usize, name: &str) -> Option<SourceEdit> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    if !matches!(context(text, &frames), Some((Context::Paragraph, styles)) if styles.is_empty()) {
        return None;
    }
    // Not where the view shows a paragraph.
    if paragraph_at(&view::build(text, &tree), at).is_some() {
        return None;
    }
    let line_start = text.get(..at)?.rfind('\n').map_or(0, |n| n + 1);
    let line_end = text.get(at..)?.find('\n').map_or(text.len(), |n| at + n);
    if !text.get(line_start..line_end)?.trim().is_empty() {
        return None;
    }
    // The list on lines of its own, a blank line from the text around it.
    let before = text.get(..line_start)?;
    let after = text.get(line_end..)?;
    let lead = if before.trim().is_empty() || before.ends_with("\n\n") { "" } else { "\n" };
    let trail = if after.trim().is_empty() || after.starts_with("\n\n") { "" } else { "\n" };
    let open = format!("{lead}\\begin{{{name}}}\n  \\item ");
    let inserted = format!("{open}\n\\end{{{name}}}{trail}");
    Some(SourceEdit { changes: vec![change(line_start..line_end, inserted)], caret: line_start + open.len() })
}

fn list_changes(text: &str, at: usize, name: &str) -> Option<Vec<Change>> {
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
    let para = trimmed(text, paragraph_at(&blocks, at)?.range.clone());
    Some(vec![change(para.start..para.start, format!("\\begin{{{name}}}\n  \\item ")), change(para.end..para.end, format!("\n\\end{{{name}}}"))])
}

/// `r` without the white space at its ends.
fn trimmed(text: &str, r: Range<usize>) -> Range<usize> {
    let Some(src) = text.get(r.clone()) else { return r };
    let start = r.start + (src.len() - src.trim_start().len());
    start..(r.start + src.trim_end().len()).max(start)
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

/// What sets a style at a place: the toolbar's buttons undo a command, and say why they cannot
/// undo a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyledBy {
    /// `\textbf{…}`, `\textit{…}`, `\texttt{…}`: their braces can be taken off.
    Command,
    /// `\emph{…}`, which sets italic in upright text.
    Emph,
    /// `\bfseries`, `\itshape`, `\ttfamily` and kin, to the end of their group.
    Declaration,
}

/// Where source offset `at` stands in the document's structure: the toolbar's state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Here {
    /// The style the commands and declarations around it give.
    pub style: Style,
    /// Whether the text there is bold, italic or typewriter (the buttons shown pressed)…
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// …and what makes it so.
    pub bold_by: Option<StyledBy>,
    pub italic_by: Option<StyledBy>,
    pub code_by: Option<StyledBy>,
    /// The heading's level ([`view::heading_level`]) when it is in a heading.
    pub heading: Option<u8>,
    /// The innermost list around it, and how many lists deep it is (1 for a list in none).
    pub list: Option<(view::ListKind, usize)>,
}

impl Here {
    /// The style after command or declaration `name`, `by` it.
    fn apply(&mut self, name: &str, by: StyledBy) {
        let before = self.style;
        let after = match by {
            StyledBy::Declaration => view::declared(name, before),
            _ => view::styled(name, before),
        };
        let Some(after) = after else { return };
        // A style a command turns off, or turns on, is set by it; one it leaves is kept.
        let emph = if by == StyledBy::Command && name == "emph" { StyledBy::Emph } else { by };
        let set = |on: bool, was: bool, old: Option<StyledBy>| match (on, was) {
            (false, _) => None,
            (true, false) => Some(by),
            (true, true) => old,
        };
        self.bold_by = set(after.bold, before.bold, self.bold_by);
        self.italic_by = match (after.italic, before.italic) {
            (true, false) => Some(emph),
            (true, true) if matches!(name, "textit" | "textsl") => Some(StyledBy::Command),
            (on, was) => set(on, was, self.italic_by),
        };
        self.code_by = set(after.mono, before.mono, self.code_by);
        if by == StyledBy::Command {
            match name {
                "textbf" => self.bold_by = Some(StyledBy::Command),
                "texttt" => self.code_by = Some(StyledBy::Command),
                _ => {}
            }
        }
        self.style = after;
        (self.bold, self.italic, self.code) = (after.bold, after.italic, after.mono);
    }

    /// The declarations in `body` before `at`: they hold to the end of the group.
    fn declarations(&mut self, body: &[Node], at: usize) {
        for n in body.iter().take_while(|n| n.range.end <= at) {
            if let Kind::Command { name, .. } = &n.kind {
                self.apply(name, StyledBy::Declaration);
            }
        }
    }
}

/// What encloses source offset `at`.
pub fn here(text: &str, at: usize) -> Here {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let mut out = Here::default();
    for f in &frames {
        match f {
            Frame::Command { name, arg, .. } => {
                if let Some(level) = view::heading_level(name) {
                    out.heading = Some(level);
                }
                out.apply(name, StyledBy::Command);
                out.declarations(&arg.body, at);
            }
            Frame::Environment { name, body, .. } => {
                let kind = match *name {
                    "itemize" => view::ListKind::Bullets,
                    "enumerate" => view::ListKind::Numbers,
                    "description" => view::ListKind::Descriptions,
                    _ => {
                        out.declarations(body, at);
                        continue;
                    }
                };
                let depth = out.list.map_or(0, |(_, d)| d) + 1;
                out.list = Some((kind, depth));
                out.declarations(body, at);
            }
            Frame::Group(node) => {
                if let Kind::Group { body, .. } = &node.kind {
                    out.declarations(body, at);
                }
            }
            Frame::Math => {}
        }
    }
    out
}

/// The sectioning commands of the paragraph styles Heading 1, 2 and 3 in a document whose main
/// file is `main`: `\chapter`, `\section`, `\subsection` in a class with chapters (book, report and
/// their kin), else `\section`, `\subsection`, `\subsubsection`.
pub fn heading_commands(main: &str) -> [&'static str; 3] {
    const WITH_CHAPTERS: &[&str] = &["book", "report", "memoir", "scrbook", "scrreprt", "ctexbook", "ctexrep", "thesis"];
    let tree = tree::parse(main);
    let class = tree.iter().find_map(|n| match &n.kind {
        Kind::Command { name, args, .. } if name == "documentclass" => args.iter().find(|a| !a.optional).and_then(|a| main.get(a.inner.clone())),
        _ => None,
    });
    if class.is_some_and(|c| WITH_CHAPTERS.contains(&c.trim())) {
        ["chapter", "section", "subsection"]
    } else {
        ["section", "subsection", "subsubsection"]
    }
}

/// A heading at source offset `at` made a paragraph again: its command and braces removed. `None`
/// outside a heading.
pub fn set_body(text: &str, at: usize) -> Option<Vec<Change>> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    frames.iter().rev().find_map(|f| match f {
        Frame::Command { node, name, arg } if view::heading_level(name).is_some() => {
            Some(vec![change(node.range.start..arg.inner.start, ""), change(arg.inner.end..node.range.end, "")])
        }
        _ => None,
    })
}

/// The one change that makes `old`, the source at byte `start`, into `new`: only the bytes between
/// what they share at both ends. `None` when they are the same.
pub fn minimal_change(start: usize, old: &str, new: &str) -> Option<Change> {
    if old == new {
        return None;
    }
    let prefix: usize = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let (old_rest, new_rest) = (old.get(prefix..)?, new.get(prefix..)?);
    let suffix: usize = old_rest.chars().rev().zip(new_rest.chars().rev()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let text = new_rest.get(..new_rest.len() - suffix)?.to_owned();
    Some(change(start + prefix..start + old.len() - suffix, text))
}

/// Where byte `at` of the text goes once `changes` (in order, not overlapping) are made: after
/// what they insert there, at the start of a change that replaces it.
pub fn map_offset(changes: &[Change], at: usize) -> usize {
    let mut delta: isize = 0;
    for c in changes {
        if c.range.start > at || (c.range.start == at && c.range.end > at) {
            break;
        }
        if at < c.range.end {
            return c.range.start.saturating_add_signed(delta);
        }
        delta += c.text.len() as isize - (c.range.end - c.range.start) as isize;
    }
    at.saturating_add_signed(delta)
}

/// A list around a source offset: the list's node and name, the `\item`s of its body (indices),
/// and which of them holds the offset.
struct ListAt<'a> {
    node: &'a Node,
    name: &'a str,
    body: &'a [Node],
    items: Vec<usize>,
    /// Index in `items`.
    k: usize,
    /// The list it is in (directly, in one of its items), if any.
    outer: bool,
}

fn is_list(name: &str) -> bool {
    matches!(name, "itemize" | "enumerate" | "description")
}

fn list_at<'a>(frames: &[Frame<'a>], at: usize) -> Option<ListAt<'a>> {
    let lists: Vec<(usize, &Frame<'a>)> =
        frames.iter().enumerate().filter(|(_, f)| matches!(f, Frame::Environment { name, .. } if is_list(name))).collect();
    let (i, Frame::Environment { node, name, body }) = lists.last().copied()? else { return None };
    let is_item = |n: &Node| matches!(&n.kind, Kind::Command { name, .. } if name == "item");
    let items: Vec<usize> = body.iter().enumerate().filter(|(_, n)| is_item(n)).map(|(i, _)| i).collect();
    let k = items.iter().rposition(|&j| body.get(j).is_some_and(|n| n.range.start < at))?;
    // Directly in another list: nothing but that list's item between them.
    let outer = i > 0 && frames.get(i - 1).is_some_and(|f| matches!(f, Frame::Environment { name, .. } if is_list(name)));
    Some(ListAt { node, name, body, items, k, outer })
}

impl ListAt<'_> {
    /// The `\item` node of item `k`.
    fn item(&self, k: usize) -> Option<&Node> {
        self.body.get(*self.items.get(k)?)
    }

    /// Where the text of item `k` starts (after `\item` and the space after it) and ends (before
    /// the white space before the next item or the list's end).
    fn content(&self, text: &str, k: usize) -> Option<Range<usize>> {
        let head = self.item(k)?;
        let next = self.items.get(k + 1).copied().unwrap_or(self.body.len());
        let nodes = self.body.get(self.items.get(k)? + 1..next).unwrap_or_default();
        let start = head.range.end + (text.get(head.range.end..)?.len() - text.get(head.range.end..)?.trim_start().len());
        let end = nodes
            .iter()
            .rev()
            .find_map(|n| {
                let src = text.get(n.range.clone())?;
                let trimmed = src.trim_end();
                (!trimmed.is_empty()).then_some(n.range.start + trimmed.len())
            })
            .unwrap_or(head.range.end);
        Some(start.min(end.max(head.range.end))..end.max(head.range.end))
    }

    /// Whether `sel` holds item `k` whole: its text, or for an empty item where its caret stands
    /// (after `\item `).
    fn covers(&self, text: &str, k: usize, sel: &Range<usize>) -> bool {
        let Some(c) = self.content(text, k) else { return false };
        if c.is_empty() {
            let at = c.start + usize::from(text.get(c.start..c.start + 1) == Some(" "));
            return sel.start <= at && at <= sel.end;
        }
        sel.start <= c.start && c.end <= sel.end
    }

    /// Where the next item, or the list's `\end`, starts.
    fn after(&self, k: usize) -> Option<usize> {
        match self.item(k + 1) {
            Some(n) => Some(n.range.start),
            None => match &self.node.kind {
                Kind::Environment { end: Some(e), .. } => Some(e.start),
                _ => None,
            },
        }
    }

    fn begin(&self) -> Option<Range<usize>> {
        match &self.node.kind {
            Kind::Environment { begin, .. } => Some(begin.clone()),
            _ => None,
        }
    }

    fn end(&self) -> Option<Range<usize>> {
        match &self.node.kind {
            Kind::Environment { end, .. } => end.clone(),
            _ => None,
        }
    }
}

/// The white space before `pos` on its line, when only white space is there.
fn indent_at(text: &str, pos: usize) -> &str {
    let start = text.get(..pos).and_then(|s| s.rfind('\n')).map_or(0, |n| n + 1);
    text.get(start..pos).filter(|s| s.trim().is_empty()).unwrap_or_default()
}

/// `pos` moved back over the white space (line ends included) before it.
fn space_before(text: &str, pos: usize) -> usize {
    let before = text.get(..pos).unwrap_or_default();
    before.trim_end().len()
}

/// `pos` moved on over the white space after it.
fn space_after(text: &str, pos: usize) -> usize {
    let after = text.get(pos..).unwrap_or_default();
    pos + (after.len() - after.trim_start().len())
}

/// Tab on a list item: the item at source offset `at` becomes the last item of a list of the same
/// kind inside the item before it (joining the list that item ends with, if any). `None` outside a
/// list or on its first item.
pub fn indent(text: &str, at: usize) -> Option<SourceEdit> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let list = list_at(&frames, at)?;
    if list.k == 0 {
        return None;
    }
    let head = list.item(list.k)?;
    let content = list.content(text, list.k)?;
    let indent = indent_at(text, head.range.start);
    let name = list.name;
    // The item before ends with a list: the item joins it.
    let prev_start = list.items.get(list.k - 1).map_or(0, |i| i + 1);
    let prev_nodes = list.body.get(prev_start..*list.items.get(list.k)?).unwrap_or_default();
    let last = prev_nodes.iter().rev().find(|n| !invisible_node(text, n));
    let changes = match last.map(|n| &n.kind) {
        Some(Kind::Environment { name: inner, end: Some(end), body, .. }) if is_list(inner) => {
            let inner_item = body.iter().find(|n| matches!(&n.kind, Kind::Command { name, .. } if name == "item"));
            let inner_indent = inner_item.map_or(indent, |n| indent_at(text, n.range.start));
            let end_indent = indent_at(text, end.start);
            let end_src = text.get(end.clone())?;
            vec![
                change(space_before(text, end.start)..head.range.start, format!("\n{inner_indent}")),
                change(content.end..content.end, format!("\n{end_indent}{end_src}")),
            ]
        }
        _ if text
            .get(..head.range.start)
            .is_some_and(|s| s.ends_with(indent) && (s.len() == indent.len() || s[..s.len() - indent.len()].ends_with('\n'))) =>
        {
            let line = head.range.start - indent.len();
            vec![
                change(line..head.range.start, format!("{indent}\\begin{{{name}}}\n{indent}  ")),
                change(content.end..content.end, format!("\n{indent}\\end{{{name}}}")),
            ]
        }
        _ => vec![
            change(head.range.start..head.range.start, format!("\\begin{{{name}}} ")),
            change(content.end..content.end, format!(" \\end{{{name}}}")),
        ],
    };
    let caret = map_offset(&changes, at.max(content.start));
    Some(SourceEdit { changes, caret })
}

fn invisible_node(text: &str, n: &Node) -> bool {
    match n.kind {
        Kind::Text => text.get(n.range.clone()).is_none_or(|s| s.trim().is_empty()),
        Kind::ParBreak => true,
        _ => false,
    }
}

/// Shift+Tab on a list item (and Enter on an empty one): the item at source offset `at` goes one
/// level out, after the item holding its list; in a list in none, it leaves the list and becomes a
/// paragraph. The items before and after it stay in their list. `None` outside a list.
pub fn outdent(text: &str, at: usize) -> Option<SourceEdit> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let list = list_at(&frames, at)?;
    let head = list.item(list.k)?;
    let content = list.content(text, list.k)?;
    let (begin, end) = (list.begin()?, list.end()?);
    let first = list.k == 0;
    let last = list.k + 1 == list.items.len();
    let name = list.name;
    let begin_src = text.get(begin.clone())?;
    let at_begin = indent_at(text, begin.start);
    let mut changes = Vec::new();
    if list.outer {
        // Still an item, of the list around.
        if first {
            changes.push(change(space_before(text, begin.start)..head.range.start, format!("\n{at_begin}")));
        } else {
            changes.push(change(space_before(text, head.range.start)..head.range.start, format!("\n{at_begin}\\end{{{name}}}\n{at_begin}")));
        }
        // An empty item keeps the space after `\item`, for the text to come.
        let space = if content.is_empty() { " " } else { "" };
        if last {
            changes.push(change(content.end..end.end, space));
        } else {
            let next = list.after(list.k)?;
            changes.push(change(content.end..next, format!("{space}\n{at_begin}{begin_src}\n{}", indent_at(text, next))));
        }
    } else {
        // A paragraph between the parts of the list.
        if first {
            changes.push(change(space_before(text, begin.start)..content.start, "\n\n"));
        } else {
            changes.push(change(space_before(text, head.range.start)..content.start, format!("\n{at_begin}\\end{{{name}}}\n\n")));
        }
        if last {
            let after = space_after(text, end.end);
            let rest = if after >= text.len() { "\n" } else { "\n\n" };
            changes.push(change(content.end..after, rest));
        } else {
            let next = list.after(list.k)?;
            changes.push(change(content.end..next, format!("\n\n{at_begin}{begin_src}\n{}", indent_at(text, next))));
        }
    }
    changes.retain(|c| !(c.range.is_empty() && c.text.is_empty()));
    let caret = map_offset(&changes, at.clamp(content.start, content.end));
    // After the space an empty item keeps.
    let caret = caret + usize::from(list.outer && content.is_empty());
    Some(SourceEdit { changes, caret })
}

/// Whether source offset `at` is where the text of the list item holding it starts.
fn at_item_start(text: &str, frames: &[Frame<'_>], at: usize) -> bool {
    list_at(frames, at).and_then(|l| l.content(text, l.k)).is_some_and(|c| c.start == at)
}

/// Whether the list item holding source offset `at` has no text.
fn item_is_empty(text: &str, frames: &[Frame<'_>], at: usize) -> bool {
    list_at(frames, at).and_then(|l| l.content(text, l.k)).is_some_and(|c| c.is_empty())
}

/// Backspace at the start of a line of the view: at a heading's start the heading becomes a
/// paragraph, at a list item's start the item goes one level out (out of the list at the top).
/// `None` elsewhere (the caller joins the paragraphs).
pub fn backspace_at_start(text: &str, at: usize) -> Option<SourceEdit> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let heading = frames.iter().rev().find_map(|f| match f {
        Frame::Command { name, arg, .. } if view::heading_level(name).is_some() => Some(arg.inner.clone()),
        _ => None,
    });
    if let Some(inner) = heading {
        if space_after(text, inner.start) < at {
            return None;
        }
        let changes = set_body(text, at)?;
        let caret = map_offset(&changes, at);
        return Some(SourceEdit { changes, caret });
    }
    if at_item_start(text, &frames, at) {
        return outdent(text, at);
    }
    None
}
