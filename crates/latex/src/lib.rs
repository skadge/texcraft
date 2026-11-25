//! LaTeX source analysis the editor needs on every keystroke: highlighting tokens ([`lex`]), the
//! outline of sections ([`outline`]), the tolerant syntax tree of the formatted view ([`tree`])
//! and what that view shows of it ([`view`]). All are pure functions of the text, with no I/O, so
//! they run the same natively and in the browser.
//!
//! The lexer follows TeX's reading rules for the default category codes (The TeXbook, chapters 7
//! and 8): a control word is `\` followed by letters, a control symbol is `\` followed by one other
//! character, `%` starts a comment that runs to the end of the line. Math is recognised from the
//! usual LaTeX delimiters (`$`, `$$`, `\(`, `\)`, `\[`, `\]`) and the math environments.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::ops::Range;

pub mod edit;
pub mod fold;
pub mod table;
pub mod tree;
pub mod view;

/// What a run of source text is, for colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Text,
    /// `\section`, `\\`, `\%`.
    Command,
    /// From `%` to the end of the line.
    Comment,
    /// `{ } [ ]`.
    Group,
    /// `& ~ ^ _ #`.
    Special,
    /// `$`, `\[`, `\begin{equation}`'s delimiters.
    MathDelimiter,
    /// Text inside math.
    Math,
    /// The name in `\begin{name}` and `\end{name}`.
    Environment,
}

/// A run of `kind` over the byte range `range` of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
}

/// Environments whose body is math.
const MATH_ENVIRONMENTS: &[&str] = &[
    "equation",
    "equation*",
    "align",
    "align*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "flalign",
    "flalign*",
    "displaymath",
    "math",
    "eqnarray",
    "eqnarray*",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    /// Inside math that ends with this delimiter kind.
    Math(Close),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Close {
    Dollar,
    DoubleDollar,
    Paren,
    Bracket,
    Environment,
}

/// Tokens covering all of `text`, in order and without gaps. Adjacent runs of the same kind are
/// merged.
pub fn lex(text: &str) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut push = |kind: Kind, range: Range<usize>| {
        if range.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut()
            && last.kind == kind
            && last.range.end == range.start
        {
            last.range.end = range.end;
            return;
        }
        out.push(Token { kind, range });
    };
    let bytes = text.as_bytes();
    let mut mode = Mode::Text;
    let mut i = 0;
    while i < bytes.len() {
        let body = if mode == Mode::Text { Kind::Text } else { Kind::Math };
        match bytes[i] {
            b'%' => {
                let end = text[i..].find('\n').map_or(text.len(), |n| i + n);
                push(Kind::Comment, i..end);
                i = end;
            }
            b'\\' => {
                let end = control_sequence_end(text, i);
                let name = &text[i + 1..end];
                match (mode, name) {
                    (Mode::Text, "(") => {
                        push(Kind::MathDelimiter, i..end);
                        mode = Mode::Math(Close::Paren);
                    }
                    (Mode::Text, "[") => {
                        push(Kind::MathDelimiter, i..end);
                        mode = Mode::Math(Close::Bracket);
                    }
                    (Mode::Math(Close::Paren), ")") | (Mode::Math(Close::Bracket), "]") => {
                        push(Kind::MathDelimiter, i..end);
                        mode = Mode::Text;
                    }
                    (_, "begin" | "end") => {
                        let is_begin = name == "begin";
                        if let Some((env, after)) = environment_name(text, end) {
                            let is_math = MATH_ENVIRONMENTS.contains(&&text[env.clone()]);
                            let kind = if is_math { Kind::MathDelimiter } else { Kind::Command };
                            push(kind, i..end);
                            push(Kind::Group, end..env.start);
                            push(Kind::Environment, env.clone());
                            push(Kind::Group, env.end..after);
                            if is_math {
                                mode = if is_begin { Mode::Math(Close::Environment) } else { Mode::Text };
                            }
                            i = after;
                            continue;
                        }
                        push(Kind::Command, i..end);
                    }
                    _ => push(Kind::Command, i..end),
                }
                i = end;
            }
            b'$' => {
                let double = bytes.get(i + 1) == Some(&b'$');
                let len = if double { 2 } else { 1 };
                push(Kind::MathDelimiter, i..i + len);
                mode = match (mode, double) {
                    (Mode::Text, true) => Mode::Math(Close::DoubleDollar),
                    (Mode::Text, false) => Mode::Math(Close::Dollar),
                    (Mode::Math(Close::DoubleDollar), true) | (Mode::Math(Close::Dollar), false) => Mode::Text,
                    // `$` inside `$$…$$` or `\[…\]`: not a delimiter TeX would accept there; keep
                    // the current mode so one stray dollar does not recolour the rest of the file.
                    (m, _) => m,
                };
                i += len;
            }
            b'{' | b'}' | b'[' | b']' => {
                push(Kind::Group, i..i + 1);
                i += 1;
            }
            b'&' | b'~' | b'^' | b'_' | b'#' => {
                push(Kind::Special, i..i + 1);
                i += 1;
            }
            _ => {
                let end = text[i..].find(['%', '\\', '$', '{', '}', '[', ']', '&', '~', '^', '_', '#']).map_or(text.len(), |n| i + n);
                // `end > i` always holds here: the byte at `i` is none of the characters searched for.
                push(body, i..end.max(i + 1).min(text.len()));
                i = end.max(i + 1);
            }
        }
    }
    out
}

/// Where the control sequence starting with the `\` at `start` ends.
fn control_sequence_end(text: &str, start: usize) -> usize {
    let rest = &text[start + 1..];
    let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters > 0 {
        // `\begin*`-style starred commands keep their star.
        let star = usize::from(rest.as_bytes().get(letters) == Some(&b'*'));
        return start + 1 + letters + star;
    }
    match rest.chars().next() {
        Some(c) => start + 1 + c.len_utf8(),
        None => start + 1,
    }
}

/// `{name}` right after `\begin` or `\end`: the range of `name` and the index after `}`.
fn environment_name(text: &str, at: usize) -> Option<(Range<usize>, usize)> {
    let rest = text.get(at..)?;
    let open = rest.bytes().take_while(|b| *b == b' ').count();
    if rest.as_bytes().get(open) != Some(&b'{') {
        return None;
    }
    let name_start = at + open + 1;
    let close = text[name_start..].find(['}', '\n'])?;
    if text.as_bytes().get(name_start + close) != Some(&b'}') {
        return None;
    }
    Some((name_start..name_start + close, name_start + close + 1))
}

/// One heading of the outline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Heading {
    /// 0 for `\part` down to 6 for `\subparagraph`.
    pub level: u8,
    pub title: String,
    /// The line, from 0.
    pub line: u32,
}

/// The sectioning commands of `text`, in order, from retex's language-server scanner.
pub fn outline(text: &str) -> Vec<Heading> {
    retex_lsp::scan::places(text)
        .into_iter()
        .filter(|p| p.kind == retex_lsp::scan::Kind::Section)
        .map(|p| Heading { level: p.level, title: p.name, line: p.line })
        .collect()
}
