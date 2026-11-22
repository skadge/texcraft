//! Text objects: what `i` and `a` select after an operator or in Visual mode. Besides Vim's own
//! (`:help text-objects`), LaTeX's: `ie`/`ae` an environment, `i$`/`a$` inline math, and
//! `ic`/`ac` a command's argument (`ac` is the whole command with its arguments).

use texcraft_buffer::Buffer;

use crate::motion::{Class, class, matching};

/// `start..end` (chars), whole lines when `linewise`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub linewise: bool,
}

fn span(start: usize, end: usize) -> Span {
    Span { start, end, linewise: false }
}

/// The text object `kind` (the key after `i` or `a`) around `pos`.
pub(crate) fn select(b: &Buffer, pos: usize, kind: char, around: bool, count: usize) -> Option<Span> {
    match kind {
        'w' => word(b, pos, around, false, count),
        'W' => word(b, pos, around, true, count),
        '(' | ')' | 'b' => bracket(b, pos, '(', ')', around, count),
        '{' | '}' | 'B' => bracket(b, pos, '{', '}', around, count),
        '[' | ']' => bracket(b, pos, '[', ']', around, count),
        '<' | '>' => bracket(b, pos, '<', '>', around, count),
        '"' | '\'' | '`' => quote(b, pos, kind, around),
        'p' => paragraph(b, pos, around),
        's' => sentence(b, pos, around),
        'e' => environment(b, pos, around, count),
        '$' => math(b, pos, around),
        'c' => command(b, pos, around, count),
        _ => None,
    }
}

fn class_at(b: &Buffer, pos: usize, big: bool) -> Option<Class> {
    b.char_at(pos).filter(|c| *c != '\n').map(|c| class(c, big))
}

/// The run of one class around `pos`, within its line.
fn run(b: &Buffer, pos: usize, big: bool) -> (usize, usize) {
    let Some(c) = class_at(b, pos, big) else { return (pos, pos) };
    let (mut start, mut end) = (pos, pos + 1);
    while start > 0 && class_at(b, start - 1, big) == Some(c) {
        start -= 1;
    }
    while class_at(b, end, big) == Some(c) {
        end += 1;
    }
    (start, end)
}

fn blanks_after(b: &Buffer, mut pos: usize) -> usize {
    while b.char_at(pos).is_some_and(|c| c == ' ' || c == '\t') {
        pos += 1;
    }
    pos
}

fn blanks_before(b: &Buffer, mut pos: usize) -> usize {
    while pos > 0 && b.char_at(pos - 1).is_some_and(|c| c == ' ' || c == '\t') {
        pos -= 1;
    }
    pos
}

fn word(b: &Buffer, pos: usize, around: bool, big: bool, count: usize) -> Option<Span> {
    class_at(b, pos, big)?;
    let (start, mut end) = run(b, pos, big);
    let on_blank = class_at(b, pos, big) == Some(Class::Blank);
    for _ in 1..count {
        if class_at(b, end, big).is_none() {
            break;
        }
        end = run(b, end, big).1;
    }
    if !around {
        return Some(span(start, end));
    }
    if on_blank {
        // The blanks and the word after them.
        let next = if class_at(b, end, big).is_some() { run(b, end, big).1 } else { end };
        return Some(span(start, next));
    }
    let after = blanks_after(b, end);
    if after > end { Some(span(start, after)) } else { Some(span(blanks_before(b, start), end)) }
}

/// The `count`th pair of `open` and `close` around `pos`.
fn bracket(b: &Buffer, pos: usize, open: char, close: char, around: bool, count: usize) -> Option<Span> {
    let mut o = enclosing_open(b, pos, open, close, true)?;
    for _ in 1..count.max(1) {
        o = enclosing_open(b, o.checked_sub(1)?, open, close, false)?;
    }
    let c = if open == '<' { forward_close(b, o, open, close)? } else { matching(b, o)? };
    if around {
        return Some(span(o, c + 1));
    }
    let mut start = o + 1;
    let mut end = c;
    // `i{` on a block whose braces are on their own lines leaves those lines' breaks.
    if b.char_at(start) == Some('\n') {
        start += 1;
        let line_start = b.line_to_char(b.char_to_line(c));
        if b.slice(line_start, c).trim().is_empty() && line_start > start {
            end = line_start;
        }
    }
    Some(span(start, end.max(start)))
}

/// The unclosed `open` at or before `pos`. With `at_cursor`, a bracket under the caret counts as
/// the pair's (as Vim does for `di(` on a parenthesis).
fn enclosing_open(b: &Buffer, pos: usize, open: char, close: char, at_cursor: bool) -> Option<usize> {
    if at_cursor && b.char_at(pos) == Some(open) {
        return Some(pos);
    }
    let mut depth = 0usize;
    let mut p = if at_cursor && b.char_at(pos) == Some(close) { pos.checked_sub(1)? } else { pos };
    loop {
        match b.char_at(p) {
            Some(c) if c == close => depth += 1,
            Some(c) if c == open => {
                if depth == 0 {
                    return Some(p);
                }
                depth -= 1;
            }
            _ => {}
        }
        p = p.checked_sub(1)?;
    }
}

fn forward_close(b: &Buffer, open_at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut p = open_at;
    loop {
        let c = b.char_at(p)?;
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(p);
            }
        }
        p += 1;
    }
}

fn quote(b: &Buffer, pos: usize, q: char, around: bool) -> Option<Span> {
    let line = b.char_to_line(pos);
    let start = b.line_to_char(line);
    let text: Vec<char> = b.line(line).chars().collect();
    let quotes: Vec<usize> = (0..text.len()).filter(|i| text[*i] == q && (*i == 0 || text[*i - 1] != '\\')).map(|i| start + i).collect();
    let pairs: Vec<(usize, usize)> = quotes.chunks(2).filter_map(|p| if let [a, b] = p { Some((*a, *b)) } else { None }).collect();
    let (o, c) = pairs.iter().copied().find(|(o, c)| *o <= pos && pos <= *c).or_else(|| pairs.iter().copied().find(|(o, _)| *o > pos))?;
    if !around {
        return Some(span(o + 1, c));
    }
    let after = blanks_after(b, c + 1);
    if after > c + 1 { Some(span(o, after)) } else { Some(span(blanks_before(b, o), c + 1)) }
}

fn paragraph(b: &Buffer, pos: usize, around: bool) -> Option<Span> {
    let line = b.char_to_line(pos);
    let blank = |l: usize| b.line(l).trim().is_empty();
    let kind = blank(line);
    let last = b.len_lines().saturating_sub(1);
    let (mut first, mut end) = (line, line);
    while first > 0 && blank(first - 1) == kind {
        first -= 1;
    }
    while end < last && blank(end + 1) == kind {
        end += 1;
    }
    if around {
        if end < last {
            end += 1;
            while end < last && blank(end + 1) != kind {
                end += 1;
            }
        } else {
            while first > 0 && blank(first - 1) != kind {
                first -= 1;
            }
        }
    }
    Some(Span { start: b.line_to_char(first), end: b.line_to_char(end + 1), linewise: true })
}

/// A sentence ends at `.`, `!` or `?` (and closing brackets or quotes) before a blank or a line
/// end; an empty line ends one too.
fn sentence(b: &Buffer, pos: usize, around: bool) -> Option<Span> {
    let ends_at = |p: usize| -> bool {
        let Some(c) = b.char_at(p) else { return false };
        if !matches!(c, '.' | '!' | '?') {
            return false;
        }
        let mut q = p + 1;
        while b.char_at(q).is_some_and(|c| matches!(c, ')' | ']' | '"' | '\'')) {
            q += 1;
        }
        b.char_at(q).is_none_or(char::is_whitespace)
    };
    let para = paragraph(b, pos, false)?;
    let mut start = pos;
    while start > para.start && !ends_at(start - 1) {
        start -= 1;
    }
    start = start.max(para.start);
    while b.char_at(start).is_some_and(char::is_whitespace) && start < pos {
        start += 1;
    }
    let mut end = pos;
    while end < para.end && !ends_at(end) {
        end += 1;
    }
    end = (end + 1).min(para.end);
    if around {
        let mut after = end;
        while after < para.end && b.char_at(after).is_some_and(|c| c == ' ' || c == '\t') {
            after += 1;
        }
        end = after;
    }
    Some(span(start, end))
}

/// `\begin{name}` … `\end{name}` pairs, innermost around `pos` first.
fn environment(b: &Buffer, pos: usize, around: bool, count: usize) -> Option<Span> {
    let text = b.text();
    let re = regex::Regex::new(r"\\(begin|end)\s*\{([^}]*)\}").ok()?;
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut around_pos = Vec::new();
    for m in re.captures_iter(&text) {
        let (Some(all), Some(kind), Some(name)) = (m.get(0), m.get(1), m.get(2)) else { continue };
        let (start, end) = (b.byte_to_char(all.start()), b.byte_to_char(all.end()));
        if kind.as_str() == "begin" {
            stack.push((name.as_str().to_owned(), start, end));
        } else if let Some(i) = stack.iter().rposition(|(n, _, _)| n == name.as_str()) {
            let (_, bstart, bend) = stack.remove(i);
            stack.truncate(i);
            if bstart <= pos && pos < end {
                around_pos.push((bstart, bend, start, end));
            }
        }
    }
    // Pairs close inner-first, so the innermost around `pos` comes first.
    let (bstart, bend, estart, eend) = *around_pos.get(count.max(1) - 1)?;
    if around {
        return Some(span(bstart, eend));
    }
    let mut start = bend;
    if b.char_at(start) == Some('\n') {
        start += 1;
    }
    let mut end = estart;
    let line_start = b.line_to_char(b.char_to_line(estart));
    if b.slice(line_start, estart).trim().is_empty() && line_start >= start {
        end = line_start;
    }
    Some(span(start, end.max(start)))
}

/// Inline math between single unescaped `$`, outside comments.
fn math(b: &Buffer, pos: usize, around: bool) -> Option<Span> {
    let mut dollars = Vec::new();
    for line in 0..b.len_lines() {
        let start = b.line_to_char(line);
        let chars: Vec<char> = b.line(line).chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\\' => i += 1,
                '%' => break,
                '$' if chars.get(i + 1) == Some(&'$') => i += 1,
                '$' => dollars.push(start + i),
                _ => {}
            }
            i += 1;
        }
    }
    let (o, c) = dollars.chunks(2).filter_map(|p| if let [a, b] = p { Some((*a, *b)) } else { None }).find(|(o, c)| *o <= pos && pos <= *c)?;
    Some(if around { span(o, c + 1) } else { span(o + 1, c) })
}

/// A command's argument: `ic` is inside the braces around `pos`; `ac` is the command, from its
/// backslash, with all its arguments.
fn command(b: &Buffer, pos: usize, around: bool, count: usize) -> Option<Span> {
    let inner = bracket(b, pos, '{', '}', false, count)?;
    if !around {
        return Some(inner);
    }
    let group = bracket(b, pos, '{', '}', true, count)?;
    let mut start = group.start;
    // Earlier arguments: `{…}` and `[…]` right before.
    loop {
        match start.checked_sub(1).and_then(|p| b.char_at(p).map(|c| (p, c))) {
            Some((p, '}')) => start = enclosing_open(b, p, '{', '}', true)?,
            Some((p, ']')) => start = enclosing_open(b, p, '[', ']', true)?,
            _ => break,
        }
    }
    let mut name = start;
    while name > 0 && b.char_at(name - 1).is_some_and(|c| c.is_alphabetic() || c == '*' || c == '@') {
        name -= 1;
    }
    if name > 0 && b.char_at(name - 1) == Some('\\') && name < start {
        start = name - 1;
    }
    let mut end = group.end;
    while let Some(c @ ('{' | '[')) = b.char_at(end) {
        let close = if c == '{' { matching(b, end)? } else { forward_close(b, end, '[', ']')? };
        end = close + 1;
    }
    Some(span(start, end))
}
