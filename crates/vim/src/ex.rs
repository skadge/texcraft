//! The command line: Vim's search patterns (`:help pattern`, the default "magic" syntax) turned
//! into `regex` patterns, `:s` replacements, line ranges and commands.

use regex::Regex;

/// A Vim pattern as a regex. `\c` anywhere ignores case and `\C` keeps it; otherwise `ignorecase`
/// and `smartcase` decide.
pub(crate) fn pattern(vim: &str, ignorecase: bool, smartcase: bool) -> Result<Regex, String> {
    let mut out = String::new();
    let mut fold = None;
    let mut chars = vim.chars().peekable();
    let mut has_upper = false;
    let mut in_class = false;
    while let Some(c) = chars.next() {
        if in_class {
            if c == ']' {
                in_class = false;
            }
            if c == '\\' {
                out.push('\\');
                if let Some(n) = chars.next() {
                    out.push(n);
                }
                continue;
            }
            out.push(c);
            continue;
        }
        match c {
            '\\' => {
                let Some(n) = chars.next() else {
                    out.push_str("\\\\");
                    break;
                };
                match n {
                    '(' | ')' | '|' | '+' | '?' | '{' | '}' => out.push(n),
                    '=' => out.push('?'),
                    '<' | '>' => out.push_str("\\b"),
                    'c' => fold = Some(true),
                    'C' => fold = Some(false),
                    's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'n' | 't' => {
                        out.push('\\');
                        out.push(n);
                    }
                    'a' => out.push_str("[A-Za-z]"),
                    'l' => out.push_str("[a-z]"),
                    'u' => out.push_str("[A-Z]"),
                    'x' => out.push_str("[0-9A-Fa-f]"),
                    'h' => out.push_str("[A-Za-z_]"),
                    '1'..='9' => return Err("back-references in search patterns are not supported yet".to_owned()),
                    _ => out.push_str(&regex::escape(&n.to_string())),
                }
            }
            '(' | ')' | '|' | '+' | '?' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '[' => {
                in_class = true;
                out.push(c);
                if chars.peek() == Some(&'^') {
                    out.push('^');
                    chars.next();
                }
                if chars.peek() == Some(&']') {
                    out.push_str("\\]");
                    chars.next();
                }
            }
            '~' => out.push_str("\\~"),
            _ => {
                has_upper |= c.is_uppercase();
                out.push(c);
            }
        }
    }
    let fold = fold.unwrap_or(ignorecase && !(smartcase && has_upper));
    let source = if fold { format!("(?mi){out}") } else { format!("(?m){out}") };
    Regex::new(&source).map_err(|e| format!("bad pattern {vim}: {e}"))
}

/// A `:s` replacement in `regex`'s syntax: `&` and `\0` the match, `\1`… groups, `\r` a line
/// break, `\t` a tab, `\&` and `\\` themselves.
pub(crate) fn replacement(vim: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = vim.chars();
    while let Some(c) = chars.next() {
        match c {
            '&' => out.push_str("${0}"),
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                Some(d @ '0'..='9') => out.push_str(&format!("${{{d}}}")),
                Some('r' | 'n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('u' | 'U' | 'l' | 'L' | 'e' | 'E') => return Err("case changes in replacements are not supported yet".to_owned()),
                Some(o) => out.push(o),
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// A line address before it is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Address {
    Current,
    Last,
    Line(usize),
    Mark(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Range {
    pub from: (Address, isize),
    pub to: Option<(Address, isize)>,
    pub whole: bool,
}

/// A parsed command line: its range and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub range: Option<Range>,
    pub name: String,
    pub bang: bool,
    pub args: String,
}

fn address(s: &str) -> (Option<(Address, isize)>, &str) {
    let mut rest = s.trim_start();
    let base = if let Some(r) = rest.strip_prefix('.') {
        rest = r;
        Some(Address::Current)
    } else if let Some(r) = rest.strip_prefix('$') {
        rest = r;
        Some(Address::Last)
    } else if let Some(r) = rest.strip_prefix('\'') {
        let mut it = r.chars();
        let Some(m) = it.next() else { return (None, s) };
        rest = it.as_str();
        Some(Address::Mark(m))
    } else {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            None
        } else {
            rest = &rest[digits.len()..];
            digits.parse().ok().map(Address::Line)
        }
    };
    let mut offset = 0isize;
    let mut any_offset = false;
    loop {
        let sign = match rest.chars().next() {
            Some('+') => 1,
            Some('-') => -1,
            _ => break,
        };
        rest = &rest[1..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        rest = &rest[digits.len()..];
        offset += sign * digits.parse::<isize>().unwrap_or(1);
        any_offset = true;
    }
    match (base, any_offset) {
        (Some(b), _) => (Some((b, offset)), rest),
        (None, true) => (Some((Address::Current, offset)), rest),
        (None, false) => (None, s),
    }
}

pub(crate) fn parse(text: &str) -> Line {
    let mut rest = text.trim_start_matches([' ', ':']);
    let range = if let Some(r) = rest.strip_prefix('%') {
        rest = r;
        Some(Range { from: (Address::Line(1), 0), to: Some((Address::Last, 0)), whole: true })
    } else {
        let (from, r) = address(rest);
        rest = r;
        from.map(|from| {
            let to = if let Some(r) = rest.strip_prefix([',', ';']) {
                let (to, r) = address(r);
                rest = r;
                to
            } else {
                None
            };
            Range { from, to, whole: false }
        })
    };
    let rest = rest.trim_start();
    let name: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    // `:s/a/b/` and `:s#a#b#`: the name stops at the delimiter.
    let (name, after) = if name.is_empty() {
        (rest.chars().take(1).collect(), &rest[rest.chars().next().map_or(0, char::len_utf8)..])
    } else {
        (name.clone(), &rest[name.len()..])
    };
    let bang = after.starts_with('!');
    let args = after.strip_prefix('!').unwrap_or(after).trim_start_matches(' ').to_owned();
    Line { range, name, bang, args }
}

/// The pattern, replacement and flags of `:s/pattern/replacement/flags`.
pub(crate) fn substitute_args(args: &str) -> Option<(String, String, String)> {
    let mut chars = args.chars();
    let delim = chars.next()?;
    if delim.is_alphanumeric() || delim == '\\' || delim == '"' || delim == '|' {
        return None;
    }
    let mut parts = vec![String::new()];
    let mut escaped = false;
    for c in chars {
        if escaped {
            if c != delim
                && let Some(p) = parts.last_mut()
            {
                p.push('\\');
            }
            if let Some(p) = parts.last_mut() {
                p.push(c);
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delim && parts.len() < 3 {
            parts.push(String::new());
        } else if let Some(p) = parts.last_mut() {
            p.push(c);
        }
    }
    if escaped && let Some(p) = parts.last_mut() {
        p.push('\\');
    }
    let mut it = parts.into_iter();
    Some((it.next().unwrap_or_default(), it.next().unwrap_or_default(), it.next().unwrap_or_default()))
}
