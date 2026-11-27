//! What must hold after every step of a run.

use texcraft_latex::edit::{self, Part};
use texcraft_latex::tree::{self, Kind, Node};
use texcraft_latex::view::{self, Block, BlockKind, InlineKind};

use super::lines::{self, Shown, ViewLine};
use super::sim::{Done, Editor, Op};

/// A property that failed: its name, and what was seen.
#[derive(Debug, Clone)]
pub struct Failure {
    pub rule: &'static str,
    pub detail: String,
}

fn fail<T>(rule: &'static str, detail: String) -> Result<T, Failure> {
    Err(Failure { rule, detail })
}

/// The state before a step: the editor and its view.
pub struct Before<'a> {
    pub editor: &'a Editor,
    pub shown: &'a Shown,
}

pub fn after_step(before: &Before<'_>, op: &Op, done: &Done, after: &Editor) -> Result<(), Failure> {
    // Enter makes an empty item on purpose (before the caret at an item's start).
    // Tab and Shift+Tab move items, empty ones too (Shift+Tab gives back what Tab nested).
    let enter = matches!(op, Op::Enter | Op::Tab | Op::ShiftTab);
    let nodes = tree::parse(&after.text);
    if !tree::tiles(&nodes, 0..after.text.len()) {
        return fail("tree-tiles", "the tree does not tile the text".to_owned());
    }
    let shown = after.shown();
    if done.steps == 0 {
        return Ok(());
    }
    caret_shown(&shown, after)?;
    local(before, op, done)?;
    balanced_stays(before.editor, after)?;
    no_leftovers(before.editor, after, enter)?;
    no_new_source(before.shown, &shown, op)?;
    selection_kept(before, op, &shown, after)?;
    expected_view(before, op, done, &shown)
}

/// The view's text from the selection's start to its end (a line end between lines).
fn selected(shown: &Shown, e: &Editor) -> Option<String> {
    let doc = shown.doc();
    let (a, h) = e.selection(&doc)?;
    let (lo, hi) = (a.min(h), a.max(h));
    let mut out = String::new();
    for i in lo.line..=hi.line {
        let chars = &shown.lines.get(i)?.chars;
        let from = if i == lo.line { lo.at } else { 0 };
        let to = if i == hi.line { hi.at } else { chars.len() };
        out.extend(chars.get(from..to.max(from)).unwrap_or_default());
        if i < hi.line {
            out.push('\n');
        }
    }
    Some(out)
}

/// A style button pressed on a selection keeps that text selected (for the next button).
fn selection_kept(before: &Before<'_>, op: &Op, shown: &Shown, after: &Editor) -> Result<(), Failure> {
    let e = before.editor;
    if !matches!(op, Op::Bar("bold" | "italic" | "code")) || e.anchor == e.head {
        return Ok(());
    }
    // A selection the view shows as nothing (from an empty line to the start of the next) is a caret.
    let (Some(was), Some(now)) = (selected(before.shown, e).filter(|w| !w.trim().is_empty()), selected(shown, after)) else { return Ok(()) };
    if words(&was) != words(&now) {
        return fail("selection-lost", format!("{:?} was selected, {:?} is after the button", was, now));
    }
    Ok(())
}

/// The caret stands where the view shows it: a key typed next shows where the view shows the
/// caret, as if typed at the stop the view shows it at.
fn caret_shown(shown: &Shown, e: &Editor) -> Result<(), Failure> {
    let doc = shown.doc();
    if doc.lines.is_empty() {
        return Ok(());
    }
    let Some(at) = doc.locate(&e.text, e.head, None).and_then(|p| doc.source(p)) else { return Ok(()) };
    if at == e.head {
        return Ok(());
    }
    let typed = |b: usize| {
        let mut t = e.text.clone();
        // A digit: it lengthens no command's name.
        t.insert(b, '0');
        normal(&lines::shown(&t, Some(b + 1)).plain())
    };
    // Spaces at a paragraph's ends do not show: where they go does not matter.
    let line_words = |s: String| s.lines().map(words).collect::<Vec<_>>();
    if line_words(typed(e.head)) == line_words(typed(at)) {
        return Ok(());
    }
    fail(
        "caret-hidden",
        format!("the caret is at byte {} ({}), the view shows it at byte {at} ({})", e.head, context(&e.text, e.head), context(&e.text, at)),
    )
}

/// The source around byte `at`, the caret marked.
pub fn context(text: &str, at: usize) -> String {
    let mut s = at.saturating_sub(30);
    while !text.is_char_boundary(s) {
        s -= 1;
    }
    let mut t = (at + 30).min(text.len());
    while !text.is_char_boundary(t) {
        t += 1;
    }
    format!("{:?}", format!("{}|{}", text.get(s..at).unwrap_or_default(), text.get(at..t).unwrap_or_default()))
}

/// The changes stay in the blocks the selection touches and the space around them.
fn local(before: &Before<'_>, op: &Op, done: &Done) -> Result<(), Failure> {
    if matches!(op, Op::Bar("table") | Op::Math("equation", _) | Op::Figure) {
        return Ok(());
    }
    let e = before.editor;
    let (lo, hi) = (e.anchor.min(e.head), e.anchor.max(e.head));
    let blocks = &before.shown.blocks;
    let touched: Vec<&Block> = blocks.iter().filter(|b| b.range.start <= hi && lo <= b.range.end).collect();
    let (first, last) = match (touched.first(), touched.last()) {
        (Some(f), Some(l)) => (f.range.start, l.range.end),
        _ => (lo, hi),
    };
    // And the blocks next to them (the caret may stand in empty markup just after a block).
    let start = blocks.iter().filter(|b| b.range.end <= first).map(|b| b.range.start).max().unwrap_or(0);
    let end = blocks.iter().filter(|b| b.range.start >= last).map(|b| b.range.end).min().unwrap_or(e.text.len());
    // The white space around them too.
    let start = e.text.get(..start).map_or(start, |s| s.trim_end().len());
    let end = e.text.get(end..).map_or(end, |s| end + s.len() - s.trim_start().len());
    for c in &done.changes {
        if c.range.start < start || c.range.end > end {
            return fail("not-local", format!("a change at {:?} outside the blocks of the selection, {start}..{end}", c.range));
        }
    }
    Ok(())
}

/// Whether the text has nothing unclosed and nothing closing what is not open.
pub fn balanced(text: &str) -> bool {
    fn ok(nodes: &[Node]) -> bool {
        nodes.iter().all(|n| match &n.kind {
            Kind::Error => false,
            Kind::Group { body, closed } => *closed && ok(body),
            Kind::Environment { body, end, args, .. } => end.is_some() && args.iter().all(|a| a.closed && ok(&a.body)) && ok(body),
            Kind::Command { args, .. } => args.iter().all(|a| a.closed && ok(&a.body)),
            Kind::Math { closed, .. } | Kind::Verbatim { closed, .. } => *closed,
            Kind::Text | Kind::ParBreak | Kind::Comment | Kind::Special | Kind::ControlSymbol => true,
        })
    }
    ok(&tree::parse(text))
}

fn balanced_stays(before: &Editor, after: &Editor) -> Result<(), Failure> {
    if balanced(&before.text) && !balanced(&after.text) {
        return fail("unbalanced", format!("the edit left something unclosed: {}", context(&after.text, after.head)));
    }
    Ok(())
}

/// What an edit should not leave behind where the caret is not: list items with no text, lists
/// with no items, styles and headings with nothing in them.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
struct Leftovers {
    items: usize,
    lists: usize,
    styles: usize,
    headings: usize,
}

fn leftovers(text: &str, caret: usize) -> Leftovers {
    fn walk(text: &str, nodes: &[Node], caret: usize, out: &mut Leftovers) {
        for (i, n) in nodes.iter().enumerate() {
            match &n.kind {
                Kind::Environment { name, body, .. } => {
                    if matches!(name.as_str(), "itemize" | "enumerate" | "description") {
                        list(text, body, caret, out);
                    }
                    walk(text, body, caret, out);
                }
                Kind::Command { name, args, .. } => {
                    let empty =
                        args.iter().rev().find(|a| !a.optional).filter(|a| text.get(a.inner.clone()).is_some_and(|s| s.trim_ascii().is_empty()));
                    if let Some(a) = empty
                        && !(a.inner.start <= caret && caret <= a.inner.end)
                    {
                        if view::heading_level(name).is_some() {
                            out.headings += 1;
                        } else if STYLES.contains(&name.as_str()) && !followed_by_letter(text, nodes, i) {
                            out.styles += 1;
                        }
                    }
                    args.iter().for_each(|a| walk(text, &a.body, caret, out));
                }
                Kind::Group { body, .. } => walk(text, body, caret, out),
                _ => {}
            }
        }
    }
    let mut out = Leftovers::default();
    walk(text, &tree::parse(text), caret, &mut out);
    out
}

/// The style commands the toolbar and the view know.
const STYLES: &[&str] = &["textbf", "textit", "textsl", "emph", "underline", "texttt", "textsf", "textrm", "textsc", "textup", "textmd"];

/// `\textrm{}` and kin as a separator before a letter (`\LaTeX\textrm{}s`) are meant.
fn followed_by_letter(text: &str, nodes: &[Node], i: usize) -> bool {
    nodes.get(i + 1).and_then(|n| text.get(n.range.clone())).is_some_and(|s| s.starts_with(char::is_alphabetic))
}

fn list(text: &str, body: &[Node], caret: usize, out: &mut Leftovers) {
    let items: Vec<usize> =
        body.iter().enumerate().filter(|(_, n)| matches!(&n.kind, Kind::Command { name, .. } if name == "item")).map(|(i, _)| i).collect();
    if items.is_empty() {
        let shows = body.iter().any(|n| {
            !matches!(n.kind, Kind::Text | Kind::ParBreak | Kind::Comment) || text.get(n.range.clone()).is_some_and(|s| !s.trim().is_empty())
        });
        if !shows {
            out.lists += 1;
        }
        return;
    }
    for (k, &i) in items.iter().enumerate() {
        let next = items.get(k + 1).copied().unwrap_or(body.len());
        let Some(head) = body.get(i) else { continue };
        let rest = body.get(i + 1..next).unwrap_or_default();
        let empty = rest.iter().all(|n| {
            matches!(n.kind, Kind::Text | Kind::ParBreak | Kind::Comment)
                && text.get(n.range.clone()).is_some_and(|s| s.trim().is_empty() || s.trim_start().starts_with('%'))
        });
        let labelled = matches!(&head.kind, Kind::Command { args, .. } if args.iter().any(|a| a.optional));
        let end = rest.last().map_or(head.range.end, |n| n.range.end);
        let holds_caret = head.range.start < caret && caret <= end;
        if empty && !labelled && !holds_caret {
            out.items += 1;
        }
    }
}

fn no_leftovers(before: &Editor, after: &Editor, enter: bool) -> Result<(), Failure> {
    if !balanced(&before.text) {
        return Ok(());
    }
    let (b, a) = (leftovers(&before.text, before.head), leftovers(&after.text, after.head));
    let more = |x: usize, y: usize| y > x;
    // Enter on an empty heading, as in a word processor, leaves it.
    if (more(b.items, a.items) && !enter) || more(b.lists, a.lists) || more(b.styles, a.styles) || (more(b.headings, a.headings) && !enter) {
        return fail("leftover", format!("empty structure left behind: before {b:?}, after {a:?}"));
    }
    Ok(())
}

/// How much of the view is shown as its source.
fn source_count(blocks: &[Block]) -> usize {
    let mut n = 0;
    view::walk(blocks, &mut |b| {
        n += usize::from(matches!(b.kind, BlockKind::Source | BlockKind::UnsupportedTable(_)));
        n += view::inlines_of(b).iter().filter(|i| matches!(i.kind, InlineKind::Source)).count();
    });
    n
}

fn no_new_source(before: &Shown, after: &Shown, op: &Op) -> Result<(), Failure> {
    let (b, a) = (source_count(&before.blocks), source_count(&after.blocks));
    // A formula typed in the math field is the user's source.
    if a > b && !matches!(op, Op::Math(..)) {
        return fail("became-source", format!("{} more parts of the view are shown as their source", a - b));
    }
    Ok(())
}

/// Which characters of a line stand for source (not what the view adds).
fn sourced(line: &ViewLine) -> Vec<bool> {
    line.line.parts().iter().flat_map(|p| std::iter::repeat_n(!matches!(p, Part::Decoration(_)), p.chars())).collect()
}

/// The view's text as words: white space taken out.
fn words(s: &str) -> String {
    let mut out = String::new();
    // An empty group after a command shown as source is the edit's (it keeps typed text apart from
    // the command's name); a title block with no title and no author shows `\maketitle`.
    let s = s.replace("{}", "").replace("\\maketitle", "");
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        match c {
            '’' => out.push('\''),
            '‘' => out.push('`'),
            '“' => out.push_str("``"),
            '”' => out.push_str("''"),
            '–' => out.push_str("--"),
            '—' => out.push_str("---"),
            '…' => out.push_str("..."),
            _ => out.push(c),
        }
    }
    out
}

/// Pasted text as the view shows it: the bullets and numbers of lines that become list items
/// left out.
fn pasted(s: &str) -> String {
    let lines: Vec<&str> = s
        .lines()
        .map(|l| {
            let l = l.trim_start();
            for bullet in ["- ", "* ", "• ", "◦ ", "‣ ", "– "] {
                if let Some(rest) = l.strip_prefix(bullet) {
                    return rest;
                }
            }
            let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            match l.get(digits..).and_then(|r| r.strip_prefix(". ").or_else(|| r.strip_prefix(") "))) {
                Some(rest) if digits > 0 => rest,
                _ => l,
            }
        })
        .collect();
    lines.join("\n")
}

/// Each line's spaces collapsed and trimmed, empty lines left out.
fn normal(s: &str) -> String {
    let lines: Vec<String> =
        s.lines().map(|l| l.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

/// The view's text with what is between `from` and `to` (line, char) taken out and `put` in its
/// place; the characters the view adds stay.
fn edited(shown: &Shown, from: (usize, usize), to: (usize, usize), put: &str) -> String {
    let mut out = Vec::new();
    for (i, l) in shown.lines.iter().enumerate() {
        if l.phantom {
            if i == from.0 {
                out.push(put.to_owned());
            }
            continue;
        }
        let keep = sourced(l);
        let mut s = String::new();
        for (k, c) in l.chars.iter().enumerate() {
            if (i, k) == from {
                s.push_str(put);
            }
            let inside = (i, k) >= from && (i, k) < to;
            if !inside && keep.get(k).copied().unwrap_or(true) {
                s.push(*c);
            }
        }
        if (i, l.chars.len()) == from {
            s.push_str(put);
        }
        out.push(s);
    }
    out.join("\n")
}

fn expected_view(before: &Before<'_>, op: &Op, done: &Done, after: &Shown) -> Result<(), Failure> {
    let doc = before.shown.doc();
    let Some((_, h)) = before.editor.selection(&doc) else { return Ok(()) };
    let got = after.plain();
    let changes_typography = |s: &str| s.contains(['-', '`', '\'', '.', '\n', '\t', '\r']) || s.contains("  ");
    let expected = if let Some((p, width)) = done.char_deleted {
        Some((edited(before.shown, (p.line, p.at), (p.line, p.at + width), ""), true))
    } else if let Some(s) = &done.typed {
        // Typed after a heading's label, it goes in the heading ([`edit::typing_point`]).
        let e = before.editor;
        let typed_at = doc.locate(&e.text, edit::typing_point(&e.text, e.head), e.hint).unwrap_or(h);
        let (lo, hi) = done.selection_deleted.unwrap_or((typed_at, typed_at));
        let strict = lo.line == hi.line && !changes_typography(s) && matches!(op, Op::Type(_)) && done.steps == 1;
        let at = edit::insertion(&before.editor.text, &doc, (lo, hi)).filter(|_| lo != hi).unwrap_or(before.editor.head);
        let listed = edit::paste(&before.editor.text, at, s).contains("\\item ");
        let s = if matches!(op, Op::Paste(_)) && listed { pasted(s) } else { s.clone() };
        Some((edited(before.shown, (lo.line, lo.at), (hi.line, hi.at), &s), strict))
    } else if let Some((lo, hi)) = done.selection_deleted {
        Some((edited(before.shown, (lo.line, lo.at), (hi.line, hi.at), ""), lo.line == hi.line && !done.keeps_words))
    } else if done.keeps_words {
        Some((before.shown.plain(), false))
    } else {
        None
    };
    let Some((want, strict)) = expected else { return Ok(()) };
    // Strict: the same lines with the same words; else the same words.
    let lines = |s: &str| normal(s).lines().map(words).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    let (w, g) = if strict { (lines(&want), lines(&got)) } else { (words(&want), words(&got)) };
    // Small capitals show typed letters as capitals.
    let (w, g) = (w.to_lowercase(), g.to_lowercase());
    if w == g {
        return Ok(());
    }
    let at = w.chars().zip(g.chars()).take_while(|(x, y)| x == y).count();
    let near = |s: &str| s.chars().skip(at.saturating_sub(20)).take(50).collect::<String>();
    let rule = if strict { "view-text" } else { "view-words" };
    fail(rule, format!("the view shows {:?}, expected {:?}", near(&g), near(&w)))
}
