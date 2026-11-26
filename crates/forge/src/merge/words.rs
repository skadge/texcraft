//! The tokens a passage is merged by: words, whitespace, line endings, punctuation, and LaTeX's
//! markup kept whole (a control sequence `\cite`, a control symbol `\%`, a comment to the end of
//! its line), so that a merge never splits a command from its name.

/// Where each token of `text` starts, then `text.len()`: the tokens are the text between two
/// neighbours, and together they are the whole text.
pub(super) fn bounds(text: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.len() / 3 + 2);
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        out.push(i);
        let rest = &text[i + c.len_utf8()..];
        let len = match c {
            '\r' if rest.starts_with('\n') => 2,
            '\\' => {
                let letters = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
                1 + if letters > 0 { letters } else { rest.chars().next().map_or(0, char::len_utf8) }
            }
            '%' => {
                let line = rest.find('\n').unwrap_or(rest.len());
                1 + rest[..line].strip_suffix('\r').map_or(line, str::len)
            }
            c if c.is_alphanumeric() => c.len_utf8() + run(rest, char::is_alphanumeric),
            c if is_space(c) => c.len_utf8() + run(rest, is_space),
            c => c.len_utf8(),
        };
        i += len;
    }
    out.push(text.len());
    out
}

/// Whitespace within a line: line endings are tokens of their own.
fn is_space(c: char) -> bool {
    c.is_whitespace() && c != '\n' && c != '\r'
}

/// The length of the start of `text` made of `class`.
fn run(text: &str, class: fn(char) -> bool) -> usize {
    text.find(|c: char| !class(c)).unwrap_or(text.len())
}

/// A line ending, whichever it is (`\n` or `\r\n`).
pub(super) fn is_line_end(token: &str) -> bool {
    token == "\n" || token == "\r\n"
}
