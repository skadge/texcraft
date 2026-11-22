//! Motions: where a key moves the caret, as functions of the text. Positions are chars.
//!
//! Words follow Vim's documentation (`:help word`): a word is a run of letters, digits and
//! underscores, or a run of other non-blank characters; a WORD is a run of non-blank characters;
//! an empty line is also a word.

use texcraft_buffer::Buffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Blank,
    Word,
    Punct,
}

pub(crate) fn class(c: char, big: bool) -> Class {
    if c.is_whitespace() {
        Class::Blank
    } else if big || c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

fn class_at(b: &Buffer, pos: usize, big: bool) -> Class {
    b.char_at(pos).map_or(Class::Blank, |c| class(c, big))
}

/// `pos` starts an empty line.
fn empty_line(b: &Buffer, pos: usize) -> bool {
    b.char_at(pos) == Some('\n') && (pos == 0 || b.char_at(pos - 1) == Some('\n'))
}

/// `w` and `W`.
pub(crate) fn word_start(b: &Buffer, pos: usize, big: bool) -> usize {
    let len = b.len_chars();
    let mut p = pos;
    let c0 = class_at(b, p, big);
    if c0 != Class::Blank {
        while p < len && class_at(b, p, big) == c0 {
            p += 1;
        }
    } else if empty_line(b, p) {
        p += 1;
    }
    while p < len {
        let c = b.char_at(p).unwrap_or(' ');
        if c == '\n' {
            p += 1;
            if empty_line(b, p) {
                return p;
            }
        } else if c.is_whitespace() {
            p += 1;
        } else {
            break;
        }
    }
    p
}

/// `e` and `E` (inclusive).
pub(crate) fn word_end(b: &Buffer, pos: usize, big: bool) -> usize {
    let len = b.len_chars();
    let mut p = pos + 1;
    while p < len && class_at(b, p, big) == Class::Blank {
        p += 1;
    }
    if p >= len {
        return len.saturating_sub(1);
    }
    let c = class_at(b, p, big);
    while p + 1 < len && class_at(b, p + 1, big) == c {
        p += 1;
    }
    p
}

/// `b` and `B`.
pub(crate) fn word_back(b: &Buffer, pos: usize, big: bool) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut p = pos - 1;
    while p > 0 && class_at(b, p, big) == Class::Blank && !empty_line(b, p) {
        p -= 1;
    }
    if empty_line(b, p) {
        return p;
    }
    let c = class_at(b, p, big);
    while p > 0 && class_at(b, p - 1, big) == c {
        p -= 1;
    }
    p
}

/// `ge` and `gE` (inclusive).
pub(crate) fn word_end_back(b: &Buffer, pos: usize, big: bool) -> usize {
    let mut p = pos;
    let c0 = class_at(b, p, big);
    if c0 != Class::Blank {
        while p > 0 && class_at(b, p - 1, big) == c0 {
            p -= 1;
        }
    }
    if p == 0 {
        return 0;
    }
    p -= 1;
    while p > 0 && class_at(b, p, big) == Class::Blank && !empty_line(b, p) {
        p -= 1;
    }
    p
}

/// The first non-blank of `line` (its end when it is blank).
pub(crate) fn first_non_blank(b: &Buffer, line: usize) -> usize {
    let start = b.line_to_char(line);
    start + b.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// The last character of `line` (its start when it is empty).
pub(crate) fn line_last(b: &Buffer, line: usize) -> usize {
    let start = b.line_to_char(line);
    b.line_end(line).saturating_sub(1).max(start)
}

/// Column `col` of `line`, at most its last character (`past_end` allows the end itself).
pub(crate) fn at_column(b: &Buffer, line: usize, col: usize, past_end: bool) -> usize {
    let start = b.line_to_char(line);
    let end = b.line_end(line);
    let last = if past_end { end } else { line_last(b, line) };
    start.saturating_add(col).min(last).max(start)
}

pub(crate) fn column(b: &Buffer, pos: usize) -> usize {
    pos - b.line_to_char(b.char_to_line(pos))
}

/// `f`, `F`, `t` and `T`: the `count`th `target` on the line, or none. `repeat` is for `;` and
/// `,`, which after `t` go past a target right next to the caret.
pub(crate) fn find_in_line(b: &Buffer, pos: usize, target: char, forward: bool, till: bool, count: usize, repeat: bool) -> Option<usize> {
    let line = b.char_to_line(pos);
    let (start, end) = (b.line_to_char(line), b.line_end(line));
    let mut p = pos;
    let mut found = 0;
    // `;` after `t` must not stay on the character just before the target.
    let skip = usize::from(till && repeat);
    loop {
        if forward {
            p += 1;
            if p >= end {
                return None;
            }
        } else {
            if p <= start {
                return None;
            }
            p -= 1;
        }
        if b.char_at(p) == Some(target) && (found > 0 || p.abs_diff(pos) > skip) {
            found += 1;
            if found == count {
                return Some(match (till, forward) {
                    (false, _) => p,
                    (true, true) => p - 1,
                    (true, false) => p + 1,
                });
            }
        }
    }
}

const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// The bracket matching the one at `pos`.
pub(crate) fn matching(b: &Buffer, pos: usize) -> Option<usize> {
    let c = b.char_at(pos)?;
    let (open, close, forward) = PAIRS.iter().find_map(|(o, cl)| {
        if c == *o {
            Some((*o, *cl, true))
        } else if c == *cl {
            Some((*o, *cl, false))
        } else {
            None
        }
    })?;
    let mut depth = 0usize;
    let mut p = pos;
    loop {
        let ch = b.char_at(p)?;
        if ch == open {
            if forward { depth += 1 } else { depth = depth.checked_sub(1)? }
        } else if ch == close {
            if forward { depth = depth.checked_sub(1)? } else { depth += 1 }
        }
        if depth == 0 {
            return Some(p);
        }
        if forward {
            p += 1;
        } else {
            p = p.checked_sub(1)?;
        }
    }
}

/// `%`: from the first bracket at or after `pos` on the line to its match.
pub(crate) fn percent(b: &Buffer, pos: usize) -> Option<usize> {
    let end = b.line_end(b.char_to_line(pos));
    (pos..end).find(|p| b.char_at(*p).is_some_and(|c| PAIRS.iter().any(|(o, cl)| c == *o || c == *cl))).and_then(|p| matching(b, p))
}

fn blank_line(b: &Buffer, line: usize) -> bool {
    b.line(line).is_empty()
}

/// `}` and `{`: the next or previous empty line, or the end or start of the text.
pub(crate) fn paragraph(b: &Buffer, pos: usize, forward: bool, count: usize) -> usize {
    let mut line = b.char_to_line(pos);
    let last = b.len_lines().saturating_sub(1);
    for _ in 0..count {
        if forward {
            while line < last && blank_line(b, line) {
                line += 1;
            }
            while line < last && !blank_line(b, line) {
                line += 1;
            }
            if line == last && !blank_line(b, line) {
                return b.len_chars().saturating_sub(1).max(b.line_to_char(last));
            }
        } else {
            while line > 0 && blank_line(b, line) {
                line -= 1;
            }
            while line > 0 && !blank_line(b, line) {
                line -= 1;
            }
        }
    }
    b.line_to_char(line)
}
