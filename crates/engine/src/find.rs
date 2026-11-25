//! Finding and replacing in a file's text: what the editor's find bar and the `file.find` and
//! `file.replace` commands share, so an agent finds what a person does.

use std::ops::Range;

use regex::{NoExpand, Regex};

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub pattern: String,
    /// `pattern` is a regular expression (the `regex` crate's syntax; `$1` in replacements).
    pub regex: bool,
    /// Upper and lower case differ.
    pub case: bool,
    /// Only whole words match.
    pub word: bool,
}

/// `text` as a regular expression that matches it literally.
pub fn escape(text: &str) -> String {
    regex::escape(text)
}

/// At most this many matches are listed.
pub const MAX_MATCHES: usize = 10_000;

impl Query {
    pub fn compile(&self) -> Result<Regex, String> {
        if self.pattern.is_empty() {
            return Err("Type something to find.".to_owned());
        }
        let core = if self.regex { self.pattern.clone() } else { regex::escape(&self.pattern) };
        let core = if self.word { format!(r"\b(?:{core})\b") } else { core };
        let flags = if self.case { "(?m)" } else { "(?mi)" };
        regex::RegexBuilder::new(&format!("{flags}{core}"))
            .size_limit(1 << 22)
            .build()
            .map_err(|e| format!("That is not a valid regular expression: {e}"))
    }

    /// The byte ranges of the matches in `text`, non-empty ones only, at most [`MAX_MATCHES`].
    pub fn matches(&self, text: &str) -> Result<Vec<Range<usize>>, String> {
        let re = self.compile()?;
        Ok(re.find_iter(text).filter(|m| !m.is_empty()).take(MAX_MATCHES).map(|m| m.range()).collect())
    }

    /// `text` with every match inside `within` (bytes; all of it when none) replaced, and how many.
    pub fn replace_all(&self, text: &str, replacement: &str, within: Option<Range<usize>>) -> Result<(String, usize), String> {
        let re = self.compile()?;
        let within = within.unwrap_or(0..text.len());
        let (head, rest) = text.split_at(within.start.min(text.len()));
        let (middle, tail) = rest.split_at((within.end - within.start).min(rest.len()));
        let count = re.find_iter(middle).filter(|m| !m.is_empty()).count();
        let replaced = if self.regex { re.replace_all(middle, replacement) } else { re.replace_all(middle, NoExpand(replacement)) };
        Ok((format!("{head}{replaced}{tail}"), count))
    }

    /// The replacement for the match at `range` (a regular expression's `$1` expanded).
    pub fn replacement_for(&self, text: &str, range: Range<usize>, replacement: &str) -> Result<String, String> {
        if !self.regex {
            return Ok(replacement.to_owned());
        }
        let re = self.compile()?;
        let caps = re.captures_at(text, range.start).filter(|c| c.get(0).is_some_and(|m| m.range() == range));
        let mut out = String::new();
        if let Some(caps) = caps {
            caps.expand(replacement, &mut out);
        }
        Ok(out)
    }
}

/// Where a match is: its line and column (from 1, in characters), and the rest of its line around
/// it, clipped when the line is long (an ellipsis marks what was cut).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub line: usize,
    pub column: usize,
    /// The line before the match.
    pub before: String,
    /// The line after the match (after its end, on the line where it ends).
    pub after: String,
}

/// At most this many characters of a line are kept before a match, and after it.
const CONTEXT_BEFORE: usize = 40;
const CONTEXT_AFTER: usize = 120;

/// The places of `ranges` (byte ranges in `text`, in order).
pub fn places(text: &str, ranges: &[Range<usize>]) -> Vec<Place> {
    let mut line = 1;
    let mut line_start = 0;
    let mut scanned = 0;
    ranges
        .iter()
        .map(|r| {
            for (i, c) in text[scanned..r.start].char_indices() {
                if c == '\n' {
                    line += 1;
                    line_start = scanned + i + 1;
                }
            }
            scanned = r.start;
            let head = &text[line_start..r.start];
            let column = head.chars().count() + 1;
            let before = match head.char_indices().rev().nth(CONTEXT_BEFORE - 1) {
                Some((i, _)) if i > 0 => format!("…{}", &head[i..]),
                _ => head.to_owned(),
            };
            let tail = &text[r.end..];
            let tail = tail.split_once('\n').map_or(tail, |(l, _)| l).trim_end_matches('\r');
            let after = match tail.char_indices().nth(CONTEXT_AFTER) {
                Some((i, _)) => format!("{}…", &tail[..i]),
                None => tail.to_owned(),
            };
            Place { line, column, before, after }
        })
        .collect()
}
