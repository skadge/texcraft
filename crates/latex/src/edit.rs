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
                    // A command shown as several characters (`\LaTeX`) stands for its source whole:
                    // no stop inside it (its characters but the last stand for nothing).
                    for (k, s) in offsets.iter().enumerate() {
                        if k == 0 || offsets.get(k - 1) != Some(s) || k + 1 == offsets.len() {
                            push(at + k, *s);
                        }
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

    /// The source of the character of text that ends at `v` (of a command shown as several
    /// characters, such as `\LaTeX`, all of them).
    pub fn char_before(&self, v: usize) -> Option<Range<usize>> {
        self.chars_between(self.prev_stop(v)?, v)
    }

    /// The source of the character of text that starts at `v` (of a command shown as several
    /// characters, all of them).
    pub fn char_after(&self, v: usize) -> Option<Range<usize>> {
        self.chars_between(v, self.next_stop(v)?)
    }

    /// How many characters [`Self::char_before`] (`back`) or [`Self::char_after`] at `v` stands
    /// for in the view.
    pub fn char_width(&self, v: usize, back: bool) -> usize {
        let other = if back { self.prev_stop(v) } else { self.next_stop(v) };
        other.map_or(0, |o| o.abs_diff(v))
    }

    /// The source of the characters of one text run from stop `a` to stop `b`.
    fn chars_between(&self, a: usize, b: usize) -> Option<Range<usize>> {
        self.placed().find_map(|(s, p)| match p {
            Part::Text(offsets) if s <= a && b <= s + p.chars() && a < b => Some(*offsets.get(a - s)?..*offsets.get(b - s)?),
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
    /// Where source offset `byte` of `text` is shown: in line `hint` if it has a stop there, else
    /// in the first line that has; else in the line that spans it (`hint` first); else in the first
    /// line it is next to with nothing shown between them (a space typed at the line's end, an
    /// empty `\textbf{}` there, the line's end); else at the start of the first line after it.
    pub fn locate(&self, text: &str, byte: usize, hint: Option<usize>) -> Option<Pos> {
        let spans = |i: usize| self.lines.get(i).and_then(Line::span).is_some_and(|s| s.start <= byte && byte <= s.end);
        let next_to = |i: usize| {
            self.lines.get(i).and_then(Line::span).is_some_and(|s| {
                // After its end, a line end too (the text typed there joins the line's).
                (s.end < byte && text.get(s.end..byte).is_some_and(|t| t.matches('\n').count() < 2 && invisible(t)))
                    || (byte < s.start && text.get(byte..s.start).is_some_and(shows_nothing))
            })
        };
        // A line with a stop at it first: a line can span others (a table's caption above it and
        // its label below), its stops are its own.
        let exact = |i: usize| self.lines.get(i).is_some_and(|l| l.stops.iter().any(|s| s.1 == byte));
        let line = hint.filter(|h| exact(*h)).or_else(|| (0..self.lines.len()).find(|i| exact(*i)));
        // A line it is just after, nothing shown between, before one that spans it (a table's
        // caption line above it and its label below span its cells).
        let line = line.or_else(|| hint.filter(|h| spans(*h)));
        let line = line.or_else(|| (0..self.lines.len()).find(|i| next_to(*i))).or_else(|| (0..self.lines.len()).find(|i| spans(*i)));
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
        let gap = (a <= b && text.get(a..b)?.trim().is_empty()).then_some(a..b)?;
        keep_comment_ends(text, vec![gap]).into_iter().next()
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
        keep_comment_ends(text, merge(out))
    }

    /// What each line shows, for finding words in the view: its characters, and the source each
    /// stands for. A character standing for several bytes of source shows as what it reads as (an
    /// accent's letter, a space); an atom (a chip, a formula) as U+FFFC; what the view adds alone
    /// (a gap, a mark) is left out.
    pub fn shown_text(&self, text: &str) -> Vec<(String, Vec<Range<usize>>)> {
        self.lines
            .iter()
            .map(|line| {
                let (mut shown, mut sources) = (String::new(), Vec::new());
                for part in line.parts() {
                    match part {
                        Part::Text(offsets) => {
                            for w in offsets.windows(2) {
                                let [a, b] = w else { continue };
                                let src = text.get(*a..*b).unwrap_or_default();
                                let mut one = src.chars();
                                let c = match (one.next(), one.next()) {
                                    (Some('~'), None) => ' ',
                                    (Some(c), None) => c,
                                    _ if src == "--" => '–',
                                    _ if src == "---" => '—',
                                    _ if src == "``" => '“',
                                    _ if src == "''" => '”',
                                    _ if !src.is_empty() && src.trim().is_empty() => ' ',
                                    _ => view::accented(src).map_or('\u{fffc}', |(c, _)| c),
                                };
                                shown.push(c);
                                sources.push(*a..*b);
                            }
                        }
                        Part::Atom { range, .. } => {
                            shown.push('\u{fffc}');
                            sources.push(range.clone());
                        }
                        Part::Decoration(_) => {}
                    }
                }
                (shown, sources)
            })
            .collect()
    }

    /// The source deleting the selection from `a` to `b` removes: [`Self::pieces`], and across
    /// lines the structure of the lists it spans ([`deletion`]).
    pub fn deleted(&self, text: &str, a: Pos, b: Pos) -> Vec<Range<usize>> {
        let (a, b) = (a.min(b), a.max(b));
        let pieces = self.pieces(text, a, b);
        let sel = match (self.source(a), self.source(b)) {
            (Some(sa), Some(sb)) if sa < sb => Some(sa..sb),
            _ => None,
        };
        let out = match &sel {
            Some(sel) if a.line != b.line => deletion(text, pieces, sel.clone()),
            _ => pieces,
        };
        keep_comment_ends(text, emptied(text, out, sel))
    }
}

/// `ranges` (to delete) without the line end that ends a comment their start is in: the comment
/// would run on over what follows it (a closing brace).
fn keep_comment_ends(text: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges
        .into_iter()
        .filter_map(|r| {
            if out_of_comment(text, r.start) == r.start {
                return Some(r);
            }
            let end = text.get(r.clone())?.find('\n').map_or(r.start, |n| r.start + n + 1);
            let r = end.min(r.end)..r.end;
            (!r.is_empty()).then_some(r)
        })
        .collect()
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

/// `ranges` (to delete, sorted and merged) with the style commands and headings whose whole text
/// they hold: their commands and braces go with their text, not left empty where the caret is not
/// (the one the selection starts in keeps its command, for the text typed next).
fn emptied(text: &str, ranges: Vec<Range<usize>>, sel: Option<Range<usize>>) -> Vec<Range<usize>> {
    fn walk(text: &str, nodes: &[Node], (first, sel): (usize, &Option<Range<usize>>), out: &mut Vec<Range<usize>>) {
        for n in nodes {
            match &n.kind {
                Kind::Command { name, args, .. } => {
                    args.iter().for_each(|a| walk(text, &a.body, (first, sel), out));
                    // An e-mail (`\ead`) too, which the view has no place for when empty.
                    let field = name == "ead";
                    let wraps = view::styled(name, Style::default()).is_some() || view::heading_level(name).is_some() || field;
                    let Some(arg) = args.iter().rev().find(|a| !a.optional) else { continue };
                    let inner = trimmed(text, arg.inner.clone());
                    // Its text deleted, or empty already and inside the selection.
                    let held = if inner.is_empty() {
                        sel.as_ref().is_some_and(|s| s.start <= inner.start && inner.start <= s.end)
                    } else {
                        // Whole, but for white space between what goes (a macro's call and a word).
                        let mut left = String::new();
                        let mut at = inner.start;
                        for r in out.iter().filter(|r| r.end > inner.start && r.start < inner.end) {
                            left.push_str(text.get(at..r.start.max(at)).unwrap_or_default());
                            at = at.max(r.end);
                        }
                        left.push_str(text.get(at.min(inner.end)..inner.end).unwrap_or_default());
                        left.trim().is_empty()
                    };
                    // The one the selection starts in keeps its command (for the text typed next);
                    // the others go.
                    let kept = !field
                        && match sel {
                            Some(s) => n.range.start <= s.start && s.start <= arg.inner.end,
                            None => n.range.start <= first,
                        };
                    if wraps && arg.closed && held && !kept {
                        out.push(n.range.clone());
                        *out = merge(std::mem::take(out));
                    }
                }
                Kind::Group { body, .. } | Kind::Environment { body, .. } => walk(text, body, (first, sel), out),
                _ => {}
            }
        }
    }
    let Some(first) = ranges.first().map(|r| r.start) else { return ranges };
    let mut out = merge(ranges);
    let tree = tree::parse(text);
    whole_calls(text, &tree, &view::macros(text), &mut out);
    walk(text, &tree, (first, &sel), &mut out);
    out
}

/// `out` (to delete, merged) with the calls of macros shown as text whose chip and whole text it
/// holds: their arguments' brackets and braces go with them.
fn whole_calls(text: &str, nodes: &[Node], macros: &[view::Macro], out: &mut Vec<Range<usize>>) {
    for (i, n) in nodes.iter().enumerate() {
        match &n.kind {
            Kind::Command { name, args, .. } => {
                args.iter().for_each(|a| whole_calls(text, &a.body, macros, out));
                let Some(m) = macros.iter().find(|m| &m.name == name && m.shown()) else { continue };
                let Some(call) = view::macro_call(text, nodes, i, m) else { continue };
                let held = |r: &Range<usize>| out.iter().any(|o| o.start <= r.start && r.end <= o.end);
                let end = call.args.last().map_or(call.name.end, |a| a.range.end);
                if held(&call.name)
                    && call.args.iter().all(|a| {
                        let t = trimmed(text, a.inner.clone());
                        t.is_empty() || held(&t)
                    })
                {
                    out.push(call.name.start..end);
                    *out = merge(std::mem::take(out));
                }
            }
            Kind::Group { body, .. } | Kind::Environment { body, .. } => whole_calls(text, body, macros, out),
            _ => {}
        }
    }
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

/// Whether source `s`, on one line, shows nothing: spaces, braces and the openings of style
/// commands (`\textbf{`).
fn shows_nothing(s: &str) -> bool {
    !s.contains('\n') && invisible(s)
}

/// Whether source `s` shows nothing: white space, braces and the openings of style commands
/// (`\textbf{`), as an empty paragraph just made holds.
pub fn invisible(s: &str) -> bool {
    invisible_len(s) == s.len()
}

/// How many bytes `s` starts with that show nothing ([`invisible`]).
pub fn invisible_len(s: &str) -> usize {
    let mut rest = s;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '{' || c == '}');
        if let Some(comment) = rest.strip_prefix('%') {
            rest = comment.find('\n').map_or("", |n| &comment[n..]);
            continue;
        }
        let Some(after) = rest.strip_prefix('\\') else { return s.len() - rest.len() };
        let name_len = after.len() - after.trim_start_matches(|c: char| c.is_ascii_alphabetic()).len();
        let (name, open) = after.split_at(name_len);
        match open.strip_prefix('{') {
            Some(r) if view::styled(name, Style::default()).is_some() => rest = r,
            _ => return s.len() - rest.len(),
        }
    }
}

/// Whether source `s` shows nothing, declarations such as `\bfseries`, `\par` and the spacing
/// the view does not show (`\vspace{1em}`) included ([`invisible`]).
pub(crate) fn declares_only(s: &str) -> bool {
    declared_len(s) == s.len()
}

/// How many bytes `s` starts with that show nothing, declarations and spacing included
/// ([`declares_only`]).
pub fn declared_len(s: &str) -> usize {
    let mut rest = s;
    loop {
        rest = &rest[invisible_len(rest)..];
        let done = s.len() - rest.len();
        let Some(after) = rest.strip_prefix('\\') else { return done };
        // A control space, which a paragraph's end drops as it drops a space.
        if let Some(r) = after.strip_prefix([' ', '\n', '\t']) {
            rest = r;
            continue;
        }
        let name_len = after.len() - after.trim_start_matches(|c: char| c.is_ascii_alphabetic()).len();
        let (name, tail) = after.split_at(name_len);
        let hidden = matches!(name, "par" | "smallskip" | "medskip" | "bigskip" | "noindent" | "vfill" | "centering" | "raggedright" | "raggedleft");
        if matches!(name, "vspace" | "hspace") {
            // Its length, in braces, shows nothing either.
            let arg = tail.strip_prefix('*').unwrap_or(tail).trim_start();
            let Some(inner) = arg.strip_prefix('{') else { return done };
            let Some(close) = inner.find('}') else { return done };
            rest = inner.get(close + 1..).unwrap_or_default();
            continue;
        }
        if name.is_empty() || (!hidden && view::declared(name, Style::default()).is_none()) {
            return done;
        }
        rest = tail;
    }
}

/// Where the caret stands in a view with no line at all (everything deleted, or only what is
/// shown as source): at `caret` when it is in the document's body (or there is no
/// `\begin{document}`), else at the body's start.
pub fn empty_view_caret(text: &str, caret: usize) -> usize {
    const BEGIN: &str = "\\begin{document}";
    let Some(start) = text.find(BEGIN).map(|i| i + BEGIN.len()) else { return caret.min(text.len()) };
    let end = text.get(start..).and_then(|s| s.find("\\end{document}")).map_or(text.len(), |i| start + i);
    if (start..=end).contains(&caret) { caret } else { start }
}

/// Where what shows nothing before source offset `at` starts ([`declares_only`]: white space,
/// empty groups and styles, declarations, `\par`), looking back a few dozen bytes.
pub fn invisible_before(text: &str, at: usize) -> usize {
    let mut start = at;
    let mut p = at;
    while p > 0 && at - p < 64 {
        p -= 1;
        while !text.is_char_boundary(p) {
            p -= 1;
        }
        if text.get(p..at).is_some_and(declares_only) {
            start = p;
        }
    }
    start
}

/// Where the caret goes after the selection from `a` to `b` is deleted by `changes` (of the text
/// before them): after an e-mail it takes whole, at the end of the name before it; when it held
/// its first line whole and went on into another, where what is left
/// of its last line starts (the first line's block is gone, whatever is between them stays: the
/// start of a theorem, an empty group); `None` otherwise (where the deletion starts).
pub fn deleted_caret(text: &str, doc: &Doc, (a, b): (Pos, Pos), changes: &[Change]) -> Option<usize> {
    // An e-mail the deletion takes whole (its command too, [`emptied`]): at the end of the name
    // before it.
    if let Some(first) = changes.first().filter(|c| text.get(c.range.clone()).is_some_and(|s| s.starts_with("\\ead{")))
        && let Some(before) = text.get(..first.range.start).map(str::trim_end).filter(|s| s.ends_with('}'))
    {
        return Some(before.len() - 1);
    }
    let (lo, hi) = (a.min(b), a.max(b));
    if lo.line == hi.line || doc.lines.get(lo.line)?.first_stop() != Some(lo.at) {
        return None;
    }
    // A paragraph of the body: in a heading, a style, a cell or an item, the structure stays for
    // the caret (Select All then Delete keeps the first heading, emptied).
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, changes.first()?.range.start, &mut frames);
    if !frames.iter().all(|f| matches!(f, Frame::Environment { name: "document", .. })) {
        return None;
    }
    Some(map_offset(changes, doc.source(hi)?))
}

/// Where text replacing the selection from `a` to `b` goes: where the first run it deletes starts
/// when the selection's first line has one, else where the selection starts (an empty heading).
pub fn insertion(text: &str, doc: &Doc, (a, b): (Pos, Pos)) -> Option<usize> {
    let (lo, hi) = (a.min(b), a.max(b));
    let line = doc.lines.get(lo.line)?;
    let to = if hi.line == lo.line { hi.at } else { line.len() };
    let at = match line.pieces(lo.at, to).first() {
        Some(r) => Some(r.start),
        None => doc.source(lo).or_else(|| doc.pieces(text, lo, hi).first().map(|r| r.start)),
    };
    // Next to a label: in the heading's or the caption's text.
    at.map(|at| typing_point(text, at))
}

/// The selection from `a` to `b` replaced by `source` when deleting it takes structure (whole
/// list items, [`Doc::deleted`]): the deletion, then `source` where it leaves the caret, as one
/// change; the caret at byte `caret` of `source`.
pub fn replace_structure(text: &str, doc: &Doc, (a, b): (Pos, Pos), (source, caret): (&str, usize)) -> Option<SourceEdit> {
    let deleted: Vec<Change> = doc.deleted(text, a, b).into_iter().map(|r| change(r, "")).collect();
    let at = into_item(&apply(text, &deleted), deleted.first()?.range.start);
    let without = apply(text, &deleted);
    let joined = joined(&without, at, source);
    let after = apply(&without, &[change(at..at, joined.clone())]);
    let one = minimal_change(0, text, &after)?;
    Some(SourceEdit { changes: vec![one], caret: at + caret.min(source.len()) + (joined.len() - source.len()) })
}

/// Where text typed at source offset `at` goes: at the end of a heading's text when `at` is just
/// before or after the label shown with it (End in a heading puts the caret after its label), at
/// the end of a float's caption when `at` is next to its label, else `at` (before a comment it is
/// in: [`out_of_comment`]).
pub fn typing_point(text: &str, at: usize) -> usize {
    let blocks = view::view(text);
    let mut found = None;
    view::walk(&blocks, &mut |b| {
        let label = match &b.kind {
            BlockKind::Heading { label: Some(label), .. } => {
                if label.range.end == at || label.range.start == at {
                    found = view::heading_text(text, &b.range).map(|r| r.end);
                }
                return;
            }
            BlockKind::Figure { label: Some(label), .. } => label,
            BlockKind::Table(t) => match t.float.as_ref().and_then(|f| f.label.as_ref()) {
                Some(label) => label,
                None => return,
            },
            _ => return,
        };
        // A float's caption line: at the end of its caption (typed text after the label would
        // break the float).
        if label.range.end == at || label.range.start == at {
            found = caption_end(text, &b.range);
        }
    });
    out_of_comment(text, found.or_else(|| into_arguments(text, at)).unwrap_or(at))
}

/// Where text typed at source offset `at` goes when `at` is right after a command whose arguments
/// the view shows as text (a macro's chip, beamer's `\only<2>`: at the start of its first
/// argument's text), or before an overlay specification (`\item|<2->`: after it).
pub fn into_arguments(text: &str, at: usize) -> Option<usize> {
    fn find(nodes: &[Node], at: usize) -> Option<(&[Node], usize)> {
        for (i, n) in nodes.iter().enumerate() {
            if at < n.range.start || at > n.range.end {
                continue;
            }
            let inner = match &n.kind {
                Kind::Group { body, .. } => find(body, at),
                Kind::Environment { args, body, .. } => args.iter().find_map(|a| find(&a.body, at)).or_else(|| find(body, at)),
                Kind::Command { args, .. } => args.iter().find_map(|a| find(&a.body, at)),
                _ => None,
            };
            if inner.is_some() {
                return inner;
            }
            if matches!(n.kind, Kind::Command { .. }) && (n.range.start < at || matches!(&n.kind, Kind::Command { name, .. } if name == "bibitem")) {
                return Some((nodes, i));
            }
        }
        None
    }
    let tree = tree::parse(text);
    let (nodes, i) = find(&tree, at)?;
    let n = nodes.get(i)?;
    let Kind::Command { name, .. } = &n.kind else { return None };
    if let Some(spec) = tree::overlay(text, n)
        && spec.start == at
    {
        return Some(spec.end);
    }
    // Before an entry's key (its chip): in its text.
    if name == "bibitem" && n.range.start == at {
        return Some(n.range.end);
    }
    if let Some(m) = view::macros(text).into_iter().find(|m| &m.name == name && m.shown()) {
        let call = view::macro_call(text, nodes, i, &m)?;
        let first = call.args.first().filter(|_| call.name.end == at)?;
        return Some(first.inner.start);
    }
    if n.range.end != at {
        return None;
    }
    match nodes.get(i + 1) {
        Some(Node { kind: Kind::Group { .. }, range }) if view::OVERLAY_WRAPPERS.contains(&name.as_str()) && range.start == at => Some(at + 1),
        _ => None,
    }
}

/// Source offset `at` moved before the comment it is in (`%` to the line's end), where what is
/// typed shows instead of commenting out what follows it on the line (a closing brace).
pub fn out_of_comment(text: &str, at: usize) -> usize {
    let Some(before) = text.get(..at) else { return at };
    let line = before.rfind('\n').map_or(0, |n| n + 1);
    let mut slashes = 0;
    for (i, c) in before.get(line..).unwrap_or_default().char_indices() {
        match c {
            '\\' => slashes += 1,
            '%' if slashes % 2 == 0 => return line + i,
            _ => slashes = 0,
        }
    }
    at
}

/// Where the text of the `\caption` of the float whose source is `text[range]` ends.
fn caption_end(text: &str, range: &Range<usize>) -> Option<usize> {
    let nodes = tree::parse(text.get(range.clone())?);
    let Some(Node { kind: Kind::Environment { body, .. }, .. }) = nodes.first() else { return None };
    body.iter().find_map(|n| match &n.kind {
        Kind::Command { name, args, .. } if name == "caption" => args.iter().rev().find(|a| !a.optional).map(|a| range.start + a.inner.end),
        _ => None,
    })
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
        if first.is_ascii_whitespace() {
            let run = rest.len() - rest.trim_ascii_start().len();
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
            // An accent command shown as its letter (`\'e` as é): edited as one character.
            if let Some((letter, len)) = view::accented(rest) {
                if c != letter {
                    return None;
                }
                out.push(base + i);
                i += len;
                k += 1;
                continue;
            }
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
                // The empty group that ends its name.
                if src.get(i..).is_some_and(|r| r.starts_with("{}")) {
                    i += 2;
                }
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
        Frame::Group(_) | Frame::Argument | Frame::Environment { .. } => true,
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

/// `source` to insert at source offset `at`, kept apart from a control word just before it
/// (`\item`, `\dots`) whose name a letter would lengthen and after which TeX skips a space: a
/// space after `\item`, else an empty group, in front of it.
pub fn joined(text: &str, at: usize, source: &str) -> String {
    if !source.starts_with(|c: char| c.is_alphabetic() || c.is_whitespace()) || !after_control_word(text, at) {
        return source.to_owned();
    }
    let before = text.get(..at).unwrap_or_default();
    let name = &before[before.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len()..];
    if name == "item" && !source.starts_with(char::is_whitespace) { format!(" {source}") } else { format!("{{}}{source}") }
}

/// Source offset `at` moved out of a run of dashes or quotes the view shows as fewer characters
/// (`---` as an em dash, ``` `` ``` as a curly quote) when it stands inside one of them: to the end
/// of the character it is in, where the view shows the caret after typing `-` before `--`; and out
/// of a command shown as text with its empty group (`\dots{}`).
pub fn out_of_ligature(text: &str, at: usize) -> usize {
    let (Some(before), Some(after)) = (text.get(..at), text.get(at..)) else { return at };
    // Between a command shown as text and the empty group that ends its name (`\dots|{}`): after
    // the group, where the view shows the caret after it.
    let name = &before[before.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len()..];
    if after.starts_with("{}") && after_control_word(text, at) && view::logo(name).is_some() {
        return at + 2;
    }
    let Some(c) = before.chars().next_back().filter(|c| matches!(c, '-' | '`' | '\'')) else { return at };
    if !after.starts_with(c) {
        return at;
    }
    let mut start = at - (before.len() - before.trim_end_matches(c).len());
    // `\-` is a discretionary hyphen, not part of the run.
    let slashes = text.get(..start).map_or(0, |s| s.len() - s.trim_end_matches('\\').len());
    if slashes % 2 == 1 {
        start += 1;
    }
    let end = at + (after.len() - after.trim_start_matches(c).len());
    // The view reads a run from its start: `---`, then `--`, for dashes; pairs for quotes.
    let mut p = start;
    while p < at {
        let left = end - p;
        p += match c {
            '-' if left >= 3 => 3,
            _ if left >= 2 => 2,
            _ => 1,
        };
    }
    p
}

/// `changes` (in order, not overlapping) of `text`, with what keeps a control word apart from the
/// letters an edit puts after it (`\item` and `\textbf{o}` unbolded made `\itemo`, `\dots`
/// typed before a word): a space after `\item`, else an empty group. Also where (in the text
/// after the changes given) something was added, and how many bytes, for [`kept_apart_offset`].
pub fn kept_apart(text: &str, changes: &[Change]) -> (Vec<Change>, Vec<(usize, usize)>) {
    let mut out = changes.to_vec();
    let mut added = Vec::new();
    let mut shift = 0usize;
    for k in 0..out.len() {
        let after = apply(text, &out);
        let Some(c) = out.get(k) else { break };
        if c.text.is_empty() && c.range.is_empty() {
            continue;
        }
        // Where this change's text ends in the text after them all.
        let delta: isize = out.iter().take(k).map(|d| d.text.len() as isize - (d.range.end - d.range.start) as isize).sum();
        let end = c.range.start.saturating_add_signed(delta) + c.text.len();
        let letter = after.get(end..).is_some_and(|r| r.starts_with(|ch: char| ch.is_ascii_alphabetic()));
        if !letter || !after_control_word(&after, end) {
            continue;
        }
        let before = after.get(..end).unwrap_or_default();
        let name = before.get(before.trim_end_matches(|ch: char| ch.is_ascii_alphabetic()).len()..).unwrap_or_default();
        let sep = if name == "item" { " " } else { "{}" };
        added.push((end - shift, sep.len()));
        shift += sep.len();
        if let Some(c) = out.get_mut(k) {
            c.text.push_str(sep);
        }
    }
    (out, added)
}

/// Byte `at` of the text after the changes [`kept_apart`] was given, in the text after those it
/// returned: after what it added at it or before it.
pub fn kept_apart_offset(added: &[(usize, usize)], at: usize) -> usize {
    at + added.iter().filter(|(pos, _)| *pos <= at).map(|(_, len)| len).sum::<usize>()
}

/// The empty style command the caret at source offset `at` stands in (`\textbf{|}`, Bold pressed
/// and nothing typed): Backspace and Delete take it with what they delete.
pub fn empty_style_at(text: &str, at: usize) -> Option<Range<usize>> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    match frames.last() {
        Some(Frame::Command { node, name, arg })
            if view::styled(name, Style::default()).is_some() && trimmed(text, arg.inner.clone()).is_empty() && arg.closed =>
        {
            Some(node.range.clone())
        }
        _ => None,
    }
}

/// The empty style or heading the caret at source offset `at` stands in (`\textbf{|}`,
/// `\section{|}`, `\section{\textbf{|}}` whole): what a block inserted from there does not
/// leave behind.
pub fn empty_markup_at(text: &str, at: usize) -> Option<Range<usize>> {
    empty_wrappers_at(text, at, true)
}

/// The empty styles the caret at source offset `at` stands in, nested ones whole
/// (`\textbf{\textit{|}}`, Bold then Italic pressed): Backspace and Delete take them with what
/// they delete.
pub fn empty_styles_at(text: &str, at: usize) -> Option<Range<usize>> {
    empty_wrappers_at(text, at, false)
}

/// The outermost of the empty styles (and headings, with `headings`) nested around `at`.
fn empty_wrappers_at(text: &str, at: usize, headings: bool) -> Option<Range<usize>> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let mut found: Option<Range<usize>> = None;
    for f in frames.iter().rev() {
        let Frame::Command { node, name, arg } = f else { break };
        let wraps = view::styled(name, Style::default()).is_some() || (headings && view::heading_level(name).is_some());
        let inner = trimmed(text, arg.inner.clone());
        if !wraps || !arg.closed || !(inner.is_empty() || found.as_ref() == Some(&inner)) {
            break;
        }
        found = Some(node.range.clone());
    }
    found
}

/// Where the caret goes in pasted `source` ([`paste`]): at its end, or at the end of its last
/// item when it ends with a list (not after the list's `\end`, which the view does not show).
pub fn paste_caret(source: &str) -> usize {
    for name in ["itemize", "enumerate"] {
        let end = format!("\n\\end{{{name}}}");
        if source.ends_with(&end) {
            return source.len() - end.len();
        }
    }
    source.len()
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
    /// The group just before is an argument of a command the parser does not know (a macro's
    /// `\cventry{…}`, `\only<2>{…}`): a line break cannot go in it.
    Argument,
    Environment {
        node: &'a Node,
        name: &'a str,
        body: &'a [Node],
    },
    Math,
}

/// The frames around `at` in `nodes`.
fn path<'a>(nodes: &'a [Node], at: usize, out: &mut Vec<Frame<'a>>) {
    for (k, n) in nodes.iter().enumerate() {
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
                if argument_group(nodes, k) {
                    out.push(Frame::Argument);
                }
                path(body, at, out);
            }
            Kind::Environment { name, begin, args, body, end } => {
                let body_end = end.as_ref().map_or(n.range.end, |e| e.start);
                // In its arguments (a frame's title, a letter's recipient).
                if let Some(arg) = args.iter().find(|a| a.inner.start <= at && at <= a.inner.end) {
                    out.push(Frame::Command { node: n, name, arg });
                    path(&arg.body, at, out);
                } else if begin.end <= at && at <= body_end {
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

/// Whether `nodes[k]`, a group, is an argument of a command the parser does not know: right after
/// it, or after groups right after it.
fn argument_group(nodes: &[Node], k: usize) -> bool {
    let mut j = k;
    while j > 0 {
        let (Some(prev), Some(cur)) = (nodes.get(j - 1), nodes.get(j)) else { return false };
        if prev.range.end != cur.range.start {
            return false;
        }
        match &prev.kind {
            Kind::Group { .. } => j -= 1,
            Kind::Command { name, args, .. } => return args.is_empty() && tree::signature(name).is_empty() && !view::takes_nothing(name),
            _ => return false,
        }
    }
    false
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
            Frame::Argument => return None,
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

/// Whether a block (a displayed formula) can go at source offset `at`: in body text or a list
/// item, not in a heading, a caption, a footnote or a formula.
pub fn takes_block(text: &str, at: usize) -> bool {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    matches!(context(text, &frames), Some((Context::Paragraph | Context::Item(_), _)))
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
    let at = into_arguments(text, at).unwrap_or(at);
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let (found, styles) = context(text, &frames)?;
    let closes = "}".repeat(styles.len());
    let opens: String = styles.iter().map(|(_, open)| open.as_str()).collect();
    // On an empty style (Bold pressed, nothing typed): it goes, and the line breaks there.
    if let Some((outer, _)) = styles.first()
        && text.get(outer.range.clone()) == Some(format!("{opens}{closes}").as_str())
    {
        let start = outer.range.start;
        let without = apply(text, &[change(outer.range.clone(), "")]);
        let e = enter(&without, start)?;
        let after = apply(&without, &e.changes);
        let one = minimal_change(0, text, &after)?;
        return Some(SourceEdit { changes: vec![one], caret: e.caret });
    }
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
            // At the very start of a styled run: the new item goes before its command, the caret
            // staying in it.
            let start = styles.first().map(|(n, _)| n.range.start).filter(|s| text.get(*s..at) == Some(opens.as_str()));
            let (point, closes, opens) = match start {
                Some(start) => (start, String::new(), String::new()),
                None => (at, closes, opens),
            };
            let line_start = text.get(..item.range.start)?.rfind('\n').map_or(0, |n| n + 1);
            let indent = text.get(line_start..item.range.start).filter(|s| s.trim().is_empty()).unwrap_or_default();
            let space = space_around(text, point, false);
            let inserted = format!("{closes}\n{indent}\\item {opens}");
            let caret = space.start + inserted.len() + (at - point);
            Some(SourceEdit { changes: vec![change(space, inserted)], caret })
        }
        Context::Paragraph => {
            let blocks = view::build(text, &tree);
            let Some(para) = paragraph_at(&blocks, at) else {
                // An empty paragraph (only the caret shows it): another after it.
                return Some(SourceEdit { changes: vec![change(at..at, "\n\n")], caret: at + 2 });
            };
            let outer = styles.first().map(|(n, _)| n.range.clone());
            // At the end of what the paragraph shows (a declaration and a brace after it show
            // nothing): a paragraph after it, not one that only the declaration's group makes.
            if at >= para.range.end || text.get(at..para.range.end).is_some_and(declares_only) {
                return Some(open_paragraph(text, outer.map_or(at.max(para.range.end), |r| r.end)));
            }
            if at <= para.range.start {
                return Some(open_paragraph(text, outer.map_or(at, |r| r.start)));
            }
            // At the very start of a styled run, the break goes before its command: none is left
            // empty before it.
            if let Some(start) = outer.as_ref().map(|r| r.start).filter(|s| text.get(*s..at) == Some(opens.as_str())) {
                let space = space_around(text, start, true);
                let changes = vec![change(space.clone(), "\n\n")];
                return Some(SourceEdit { caret: map_offset(&changes, at), changes });
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
    // A space between runs needs no style (it would show as an empty one).
    // Nor does the name of a command whose arguments the view shows as text (a macro's chip):
    // wrapped, it would lose them.
    let pieces: Vec<&Range<usize>> = pieces
        .iter()
        .filter(|r| {
            text.get((*r).clone()).is_some_and(|s| {
                let command = s.starts_with('\\') && into_arguments(text, r.end).is_some();
                // Nor an overlay's specification (`\item<2->`): it is the command's.
                let overlay = s.starts_with('<') && into_arguments(text, r.start) == Some(r.end);
                !(s.trim().is_empty() || command || overlay)
            })
        })
        .collect();
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
    // A paragraph ending in a lone backslash would escape the closing brace; one that is a
    // command's argument (a letter's opening) is not body text.
    if ends_in_backslash(text, para.end) || in_argument(&tree, para.start) || !text.get(para.clone()).is_some_and(braces_balanced) {
        return None;
    }
    Some(vec![change(para.start..para.start, format!("\\{name}{{")), change(para.end..para.end, "}")])
}

/// What the toolbar's heading levels do at source offset `at` ([`set_heading`]), the caret kept in
/// the text: a list item is taken out of its lists first (a heading is not a list's item, as in a
/// word processor), then made a heading, as one change.
pub fn heading(text: &str, at: usize, name: &str) -> Option<SourceEdit> {
    let mut out = text.to_owned();
    let mut caret = at;
    let mut lifted = false;
    for _ in 0..32 {
        let h = here(&out, caret);
        if h.list.is_none() || h.heading.is_some() {
            break;
        }
        let e = outdent(&out, caret)?;
        out = apply(&out, &e.changes);
        caret = e.caret;
        lifted = true;
    }
    let changes = set_heading(&out, caret, name)?;
    let caret = map_inside(&changes, caret);
    if !lifted {
        return Some(SourceEdit { changes, caret });
    }
    let after = apply(&out, &changes);
    Some(SourceEdit { changes: vec![minimal_change(0, text, &after)?], caret })
}

/// A list environment (`itemize`, `enumerate`) at source offset `at`: a list of another kind there
/// is renamed; a paragraph there becomes the list's one item; on an empty line between paragraphs
/// (an empty paragraph only the caret shows), a new list with one empty item, the caret in it.
/// `None` in a list of that kind already or outside a paragraph.
pub fn set_list(text: &str, at: usize, name: &str) -> Option<SourceEdit> {
    let changes = list_changes(text, at, name);
    match changes {
        Some(changes) => {
            let caret = map_inside(&changes, at);
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
    if ends_in_backslash(text, para.end) || in_argument(&tree, para.start) {
        return None;
    }
    Some(vec![change(para.start..para.start, format!("\\begin{{{name}}}\n  \\item ")), change(para.end..para.end, format!("\n\\end{{{name}}}"))])
}

/// Whether `text` up to `end` ends in an odd run of backslashes (one that escapes what follows).
fn ends_in_backslash(text: &str, end: usize) -> bool {
    let before = text.get(..end).unwrap_or_default();
    (before.len() - before.trim_end_matches('\\').len()) % 2 == 1
}

/// Whether the braces of `s` (not escaped ones) close as they open: wrapped in a command's
/// braces, it keeps them whole (a `$}$` would close the heading made of it).
fn braces_balanced(s: &str) -> bool {
    let mut depth = 0i64;
    let mut slashes = 0;
    for c in s.chars() {
        match c {
            '\\' => {
                slashes += 1;
                continue;
            }
            '{' if slashes % 2 == 0 => depth += 1,
            '}' if slashes % 2 == 0 => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
        slashes = 0;
    }
    depth == 0
}

/// Whether source offset `at` is in a command's argument.
fn in_argument(tree: &[Node], at: usize) -> bool {
    let mut frames = Vec::new();
    path(tree, at, &mut frames);
    frames.iter().any(|f| matches!(f, Frame::Command { .. }))
}

/// A character's source `r` taken with the line end after it when it is alone on its line
/// (between two line ends, spaces aside): deleting it leaves no blank line, which would end the
/// paragraph.
pub fn alone_on_line(text: &str, r: Range<usize>) -> Range<usize> {
    let before = text.get(..r.start).unwrap_or_default().trim_end_matches([' ', '\t']);
    let after = text.get(r.end..).unwrap_or_default();
    let rest = after.trim_start_matches([' ', '\t']);
    if before.ends_with('\n') && rest.starts_with('\n') && !before.ends_with("\n\n") && !rest.starts_with("\n\n") {
        return r.start..r.end + (after.len() - rest.len()) + 1;
    }
    r
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
            Frame::Math | Frame::Argument => {}
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

/// What Body Text does at source offset `at` ([`set_body`]), the caret kept in the text: an empty
/// heading at the very start of the text leaves a line end before what follows, so that the
/// empty paragraph it becomes has a line for the caret (the view shows none before the first
/// block). `None` outside a heading.
pub fn body(text: &str, at: usize) -> Option<SourceEdit> {
    let mut changes = set_body(text, at)?;
    let caret = map_offset(&changes, at);
    let emptied = apply(text, &changes);
    if caret == 0 && !emptied.trim().is_empty() && changes.first().is_some_and(|c| c.range.start == 0) {
        if let Some(first) = changes.first_mut() {
            first.text.push_str("\n\n");
        }
        return Some(SourceEdit { changes, caret: 1 });
    }
    Some(SourceEdit { changes, caret })
}

/// The source of the command a deletion of the source `r` (a character of the view) leaves with
/// no text where the view has no place for an empty one (an e-mail, `\ead{}`, on an author's line):
/// the command goes with its last character. Also where the caret goes: at the end of the text of
/// the command just before it (the author's name), else where it was.
pub fn emptied_field(text: &str, r: &Range<usize>) -> Option<(Range<usize>, usize)> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, r.start, &mut frames);
    match frames.last() {
        Some(Frame::Command { node, name: "ead", arg }) if trimmed(text, arg.inner.clone()) == *r => {
            let before = text.get(..node.range.start)?.trim_end();
            let caret = if before.ends_with('}') { before.len() - 1 } else { node.range.start };
            Some((node.range.clone(), caret))
        }
        _ => None,
    }
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

/// A place between two blocks of the view (or before the first, or after the last, of a
/// sequence): where the block before it ends and the next starts, and whether they are paragraphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Between {
    pub prev: usize,
    pub next: usize,
    /// The block before is a paragraph (false at the start of a sequence).
    pub prev_paragraph: bool,
    /// The block after is a paragraph (false after the last block).
    pub next_paragraph: bool,
}

/// Whether the caret at source offset `c` stands in an empty paragraph of its own at `b`: what is
/// typed there makes a paragraph between the blocks (Enter just made it, or a deletion left it).
/// Only white space and empty styles are around it, and nothing joins it to a paragraph next to
/// it: a blank line, or a block that is not a paragraph, on each side.
pub fn empty_paragraph_at(text: &str, c: usize, b: Between) -> bool {
    // Just after a block that is not a paragraph (a list, a heading), or at the start of a
    // sequence, is a place of its own.
    let inside = (b.prev < c || (b.prev == c && !b.prev_paragraph && c > 0)) && c <= b.next;
    if !inside || (c == b.next && b.next_paragraph) {
        return false;
    }
    let (Some(before), Some(after)) = (text.get(b.prev..c), text.get(c..b.next)) else { return false };
    // A letter's closing (its text is the paragraph after): what opens it shows nothing.
    let after = ["\\opening{", "\\closing{", "\\ps{", "\\encl{", "\\cc{"]
        .iter()
        .find_map(|p| after.trim_end().strip_suffix(p).filter(|_| b.next_paragraph))
        .unwrap_or(after);
    let apart_before = before.matches('\n').count() >= 2 || !b.prev_paragraph || has_par(before);
    let apart_after = after.matches('\n').count() >= 2 || !b.next_paragraph || has_par(after);
    declares_only(before) && declares_only(after) && apart_before && apart_after
}

/// Whether source `s` holds a `\par` (which ends a paragraph as a blank line does).
fn has_par(s: &str) -> bool {
    s.match_indices("\\par").any(|(i, m)| !s.get(i + m.len()..).is_some_and(|r| r.starts_with(|c: char| c.is_ascii_alphabetic())))
}

/// The caret at source offset `at` moved into the list item that follows it when it stands in a
/// list's body before its first `\item` (where a deletion of whole items can leave it), so that
/// what is typed next goes in that item.
pub fn into_item(text: &str, at: usize) -> usize {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let Some(Frame::Environment { name, body, .. }) = frames.last() else { return at };
    if !is_list(name) || list_at(&frames, at).is_some() {
        return at;
    }
    let is_item = |n: &&Node| matches!(&n.kind, Kind::Command { name, .. } if name == "item");
    let Some(item) = body.iter().find(is_item).filter(|n| n.range.start >= at) else { return at };
    if !text.get(at..item.range.start).is_some_and(|s| s.trim().is_empty()) {
        return at;
    }
    item.range.end + usize::from(text.get(item.range.end..item.range.end + 1) == Some(" "))
}

/// Like [`map_offset`] for changes that wrap text (an opening and a closing inserted around it):
/// byte `at` stays inside, after the opening inserted there and before the closing.
pub fn map_inside(changes: &[Change], at: usize) -> usize {
    let mut delta: isize = 0;
    for (k, c) in changes.iter().enumerate() {
        if c.range.start > at || (c.range.start == at && (c.range.end > at || k > 0)) {
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
        // A label (`\item[Term]`) is the item's text too.
        let label = match self.item(k).map(|n| &n.kind) {
            Some(Kind::Command { args, .. }) => args.iter().find(|a| a.optional).map(|a| trimmed(text, a.inner.clone())).filter(|r| !r.is_empty()),
            _ => None,
        };
        if let Some(l) = label
            && !(sel.start <= l.start || text.get(l.start..sel.start).is_some_and(invisible))
        {
            return false;
        }
        if c.is_empty() {
            let at = c.start + usize::from(text.get(c.start..c.start + 1) == Some(" "));
            return sel.start <= at && at <= sel.end;
        }
        // What shows nothing at its ends (`\textbf{`, `}`) is not left out by a selection of its
        // text.
        // `a` is before `b`, or only what shows nothing is between them.
        let reaches = |a: usize, b: usize| a <= b || text.get(b..a).is_some_and(invisible);
        reaches(sel.start, c.start) && reaches(c.end, sel.end)
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

    /// Its `\begin{…}` with its options (`[label=(\alph*)]`), which a part of it split off keeps.
    fn begin(&self) -> Option<Range<usize>> {
        match &self.node.kind {
            Kind::Environment { begin, args, .. } => Some(begin.start..args.last().map_or(begin.end, |a| a.range.end.max(begin.end))),
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
    // In the item, before the inner list's `\end` (an empty item's caret is where both go).
    let caret = map_inside(&changes, at.clamp(content.start, content.end));
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
        // A paragraph between the parts of the list, starting with the item's label if it had
        // one (`\item[Term]`), its text kept.
        let label = match &head.kind {
            Kind::Command { args, .. } => args.iter().find(|a| a.optional).and_then(|a| text.get(a.inner.clone())).filter(|l| !l.trim().is_empty()),
            _ => None,
        };
        let label = label.map(|l| format!("{} ", l.trim())).unwrap_or_default();
        if first {
            changes.push(change(space_before(text, begin.start)..content.start, format!("\n\n{label}")));
        } else {
            changes.push(change(space_before(text, head.range.start)..content.start, format!("\n{at_begin}\\end{{{name}}}\n\n{label}")));
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
    // Between what goes before the item and what goes after it.
    let caret = map_inside(&changes, at.clamp(content.start, content.end));
    // After the space an empty item keeps.
    let caret = caret + usize::from(list.outer && content.is_empty());
    Some(SourceEdit { changes, caret })
}

/// Where a block inserted from the caret at source offset `at` goes (a table, an equation, a
/// figure): at the end of the list item holding it, else after the block of the view holding it
/// (not in the preamble, where the caret in the title stands: after the title block), else in the
/// document's body.
pub fn block_point(text: &str, at: usize) -> usize {
    let blocks = view::view(text);
    let shown = |b: &&Block| !matches!(b.kind, BlockKind::Preamble);
    let after = blocks.iter().filter(shown).find(|b| b.range.end >= at).map_or_else(|| empty_view_caret(text, at), |b| b.range.end);
    item_end(text, at).unwrap_or(after)
}

/// `insert` (a block put in after the caret at source offset `caret`) with the empty style or
/// heading the caret stands in taken out (Bold or Heading 1 pressed and nothing typed is not left
/// behind), and `sel` (bytes of the text after `insert` alone) moved for it.
pub fn dropping_empty_style(text: &str, caret: usize, insert: Change, sel: (usize, usize)) -> (Vec<Change>, (usize, usize)) {
    match empty_markup_at(text, caret).filter(|e| e.end <= insert.range.start) {
        Some(empty) => {
            let gone = empty.len();
            (vec![change(empty, ""), insert], (sel.0.saturating_sub(gone), sel.1.saturating_sub(gone)))
        }
        None => (vec![insert], sel),
    }
}

/// Source offset `at` in a float (a `figure`, a `table`) where the view has no place for it (where
/// its label was, before its `\end`): at the end of its caption, where the view shows the caret.
pub fn in_float(text: &str, at: usize) -> usize {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    match frames.last() {
        Some(Frame::Environment { node, name, .. }) if matches!(*name, "figure" | "figure*" | "table" | "table*") => {
            caption_end(text, &node.range).unwrap_or(at)
        }
        _ => at,
    }
}

/// `changes` (in order, not overlapping) with the empty style or heading the caret at `caret` stood
/// in taken out when they leave it untouched (Bold pressed, then a reference inserted elsewhere,
/// or a selection typed over: the empty `\textbf{}` is not left behind), and `after` (bytes of
/// the text after `changes`) moved for it.
pub fn leaving_empty_markup(text: &str, caret: usize, changes: &[Change], after: (usize, usize)) -> (Vec<Change>, (usize, usize)) {
    let Some(r) = empty_markup_at(text, caret) else { return (changes.to_vec(), after) };
    let touches =
        |c: &Change| (c.range.start < r.end && r.start < c.range.end) || (c.range.is_empty() && r.start < c.range.start && c.range.start < r.end);
    if changes.is_empty() || changes.iter().any(touches) {
        return (changes.to_vec(), after);
    }
    let start = map_offset(changes, r.start);
    let len = r.len();
    let moved = |b: usize| {
        if b >= start + len { b - len } else { b.min(start) }
    };
    let mut out = changes.to_vec();
    out.push(change(r, ""));
    out.sort_by_key(|c| (c.range.start, c.range.end));
    (out, (moved(after.0), moved(after.1)))
}

/// Where the text of the list item holding source offset `at` ends (where a block inserted from
/// it goes, in the item); `None` outside a list.
pub fn item_end(text: &str, at: usize) -> Option<usize> {
    let tree = tree::parse(text);
    let mut frames = Vec::new();
    path(&tree, at, &mut frames);
    let list = list_at(&frames, at)?;
    let c = list.content(text, list.k)?;
    Some(c.end.max(at.min(c.start.max(c.end))))
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
