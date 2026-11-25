//! The prose of a LaTeX file as passages of plain text, for a grammar checker (docs/ROADMAP.md,
//! E12): what [`prose::ranges`] keeps, joined into the paragraphs, headings, captions and
//! footnotes a reader reads, each with a map back to the source.
//!
//! Between two pieces of prose the source holds commands, math and keys. A grammar checker reads
//! sentences, so what stood there is put back as little as it takes for the sentence to read on:
//! a space where the source had one, a formula, a reference or a citation as the placeholder word
//! [`PLACEHOLDER`], an escaped character as itself, a word written with accent commands
//! (`caf\'e`) as its letters, `~` as a space, ``` `` ``` and `''` as quotation marks, `--` and
//! `---` as dashes. A blank line, `\par`, `\item`, `\\`, `&` and the start or end of an
//! environment that is not math end a passage; a heading, a caption, the title block and a
//! footnote are passages of their own (the sentence around a footnote goes on after it).
//!
//! [`Passage::source`] maps characters of a passage back to source bytes, and says whether they
//! are written there exactly as the passage has them: only then can a replacement be put in the
//! source without touching its LaTeX.

use std::ops::Range;

use crate::prose;

/// The word a formula, a reference or a citation reads as.
pub const PLACEHOLDER: &str = "X";

/// The longest passage, in bytes: a longer one is cut at a line break (hostile input, or a file
/// without blank lines).
const MAX_PASSAGE: usize = 20_000;

/// Brace groups deeper than this are not followed (hostile input).
const MAX_UNITS: usize = 32;

/// A run of a passage's text and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Piece {
    /// Bytes of the passage's text.
    at: usize,
    len: usize,
    /// Bytes of the source.
    source: Range<usize>,
    /// Copied from the source as it is (so byte `at + i` is source byte `source.start + i`).
    exact: bool,
    /// A formula, reference or citation's placeholder.
    placeholder: bool,
}

/// A paragraph, heading, caption or footnote as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub text: String,
    pieces: Vec<Piece>,
}

/// Where characters of a passage are in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    /// Bytes of the source, from the first character's start to the last one's end.
    pub range: Range<usize>,
    /// The source holds exactly these characters there, with nothing between them: a
    /// replacement can take their place.
    pub exact: bool,
    /// They include a formula's, reference's or citation's placeholder.
    pub placeholder: bool,
}

impl Passage {
    /// Where characters `chars` (from 0, of [`Passage::text`]) are in the source.
    pub fn source(&self, chars: Range<usize>) -> Option<Spot> {
        let byte = |c: usize| self.text.char_indices().nth(c).map_or(self.text.len(), |(b, _)| b);
        let (a, b) = (byte(chars.start), byte(chars.end.max(chars.start)));
        let touched: Vec<&Piece> = if a == b {
            self.pieces.iter().filter(|p| p.at <= a && a <= p.at + p.len).take(1).collect()
        } else {
            self.pieces.iter().filter(|p| p.at < b && a < p.at + p.len).collect()
        };
        let (first, last) = (touched.first()?, touched.last()?);
        let start = if first.exact { first.source.start + (a - first.at) } else { first.source.start };
        let end = if last.exact { last.source.start + (b - last.at).min(last.len) } else { last.source.end };
        let contiguous = touched.windows(2).all(|w| w[0].source.end == w[1].source.start);
        Some(Spot {
            range: start.min(end)..end,
            exact: contiguous && touched.iter().all(|p| p.exact),
            placeholder: touched.iter().any(|p| p.placeholder),
        })
    }

    /// The source bytes the passage covers.
    pub fn span(&self) -> Range<usize> {
        let start = self.pieces.first().map_or(0, |p| p.source.start);
        let end = self.pieces.last().map_or(start, |p| p.source.end);
        start..end
    }
}

/// A passage being built.
#[derive(Default)]
struct Builder {
    text: String,
    pieces: Vec<Piece>,
    /// The source had a space here (its bytes): one goes in before what comes next.
    space: Option<Range<usize>>,
}

impl Builder {
    fn last_char(&self) -> Option<char> {
        self.text.chars().next_back()
    }

    fn push_raw(&mut self, s: &str, source: Range<usize>, exact: bool, placeholder: bool) {
        if s.is_empty() {
            return;
        }
        // Exact text right after exact text that ends where it starts in the source is one piece.
        if exact
            && let Some(last) = self.pieces.last_mut()
            && last.exact
            && last.source.end == source.start
            && last.at + last.len == self.text.len()
        {
            last.len += s.len();
            last.source.end = source.end;
            self.text.push_str(s);
            return;
        }
        self.pieces.push(Piece { at: self.text.len(), len: s.len(), source, exact, placeholder });
        self.text.push_str(s);
    }

    /// The space the source had before `next`, if the sentence needs one there.
    fn settle_space(&mut self, next: &str) {
        let Some(gap) = self.space.take() else { return };
        let after_space = self.last_char().is_none_or(|c| c.is_whitespace() || "([{“‘".contains(c));
        let before_space_or_closing = next.chars().next().is_none_or(|c| c.is_whitespace() || ".,;:!?)]}”’".contains(c));
        if !after_space && !before_space_or_closing {
            self.push_raw(" ", gap, false, false);
        }
    }

    fn push(&mut self, s: &str, source: Range<usize>, exact: bool, placeholder: bool) {
        self.settle_space(s);
        self.push_raw(s, source, exact, placeholder);
    }

    fn take(&mut self) -> Option<Passage> {
        let b = std::mem::take(self);
        (!b.text.trim().is_empty()).then_some(Passage { text: b.text, pieces: b.pieces })
    }
}

/// A passage being built and the brace depth that ends it (`None`: the paragraph, which only a
/// break ends).
struct Unit {
    builder: Builder,
    closes_at: Option<usize>,
}

struct Walker<'a> {
    text: &'a str,
    units: Vec<Unit>,
    out: Vec<Passage>,
    /// Braces open in the source, from its start.
    depth: usize,
}

/// Commands whose arguments are a passage of their own, and whether the text around goes on
/// after it (a footnote) or ends before it (a heading).
fn unit_command(name: &str) -> Option<bool> {
    match name {
        "footnote" | "thanks" => Some(true),
        "part" | "chapter" | "section" | "subsection" | "subsubsection" | "paragraph" | "subparagraph" | "caption" | "title" | "author" | "date" => {
            Some(false)
        }
        _ => None,
    }
}

/// Commands that read as a word in the sentence: references, citations, quantities.
fn placeholder_command(name: &str) -> bool {
    matches!(
        name,
        "ref"
            | "eqref"
            | "cref"
            | "Cref"
            | "autoref"
            | "pageref"
            | "nameref"
            | "vref"
            | "cite"
            | "citep"
            | "citet"
            | "citeauthor"
            | "citeyear"
            | "parencite"
            | "textcite"
            | "autocite"
            | "footcite"
            | "SI"
            | "si"
            | "num"
            | "qty"
            | "unit"
            | "ang"
            | "url"
            | "gls"
            | "Gls"
            | "glspl"
            | "Glspl"
            | "acrshort"
            | "acrlong"
            | "acrfull"
            | "ac"
            | "acp"
            | "acs"
            | "acl"
            | "LaTeX"
            | "TeX"
            | "LaTeXe"
            | "today"
            | "verb"
    )
}

/// Commands that end a passage.
fn break_command(name: &str) -> bool {
    matches!(name, "par" | "item" | "newline" | "linebreak" | "bibitem" | "maketitle" | "tableofcontents" | "newpage" | "clearpage")
}

/// Environments whose body is code or verbatim text, skipped to their end.
fn verbatim_environment(name: &str) -> bool {
    matches!(name, "verbatim" | "verbatim*" | "Verbatim" | "lstlisting" | "minted" | "comment" | "alltt") || prose::code_environment(name)
}

/// An accent command's letter: `\'e` is `e`, `\ss` is `ss`.
fn accent_letters(name: &str) -> Option<&'static str> {
    Some(match name {
        "ss" => "ss",
        "ae" => "ae",
        "AE" => "AE",
        "oe" => "oe",
        "OE" => "OE",
        "aa" => "a",
        "AA" => "A",
        "o" => "o",
        "O" => "O",
        "l" => "l",
        "L" => "L",
        "i" => "i",
        "j" => "j",
        _ => return None,
    })
}

/// Accent commands whose argument is the letter (`\c{c}`, `\v s`).
fn accent_with_argument(name: &str) -> bool {
    matches!(name, "c" | "v" | "u" | "H" | "r" | "k" | "b" | "d" | "t")
}

/// The prose of `text` as passages, in order.
pub fn passages(text: &str, opts: prose::Options) -> Vec<Passage> {
    let ranges = prose::ranges(text, opts);
    let mut w = Walker { text, units: vec![Unit { builder: Builder::default(), closes_at: None }], out: Vec::new(), depth: 0 };
    let mut at = 0;
    for r in ranges {
        w.gap(at..r.start);
        w.prose(r.clone());
        at = r.end;
    }
    while !w.units.is_empty() {
        w.close_top();
    }
    w.out
}

impl Walker<'_> {
    fn top(&mut self) -> &mut Builder {
        if self.units.is_empty() {
            self.units.push(Unit { builder: Builder::default(), closes_at: None });
        }
        let last = self.units.len() - 1;
        &mut self.units[last].builder
    }

    /// Ends the passage being built.
    fn flush(&mut self) {
        if let Some(p) = self.top().take() {
            self.out.push(p);
        }
    }

    /// Ends the innermost unit (the paragraph starts again, empty).
    fn close_top(&mut self) {
        if let Some(mut u) = self.units.pop()
            && let Some(p) = u.builder.take()
        {
            self.out.push(p);
        }
    }

    /// A blank line: every unit ends (a footnote's paragraph too).
    fn paragraph_break(&mut self) {
        while self.units.len() > 1 {
            self.close_top();
        }
        self.flush();
    }

    fn space(&mut self, r: Range<usize>) {
        let b = self.top();
        if b.space.is_none() {
            b.space = Some(r);
        }
    }

    /// A range of prose, copied as it is but for its quotation marks, dashes and ties; a blank
    /// line in it ends the passage.
    fn prose(&mut self, r: Range<usize>) {
        let text = self.text;
        let s = text.get(r.clone()).unwrap_or_default();
        let bytes = s.as_bytes();
        let mut i = 0;
        let mut run = 0;
        while i < bytes.len() {
            let sub: Option<(&str, usize)> = match bytes[i] {
                b'`' if bytes.get(i + 1) == Some(&b'`') => Some(("“", 2)),
                b'\'' if bytes.get(i + 1) == Some(&b'\'') => Some(("”", 2)),
                b'`' => Some(("‘", 1)),
                b'-' if bytes.get(i + 1) == Some(&b'-') && bytes.get(i + 2) == Some(&b'-') => Some(("—", 3)),
                b'-' if bytes.get(i + 1) == Some(&b'-') => Some(("–", 2)),
                b'~' => Some((" ", 1)),
                b'\n' => {
                    // A blank line: spaces and tabs between two line breaks.
                    let rest = &bytes[i + 1..];
                    let blank = rest.iter().take_while(|b| **b == b' ' || **b == b'\t' || **b == b'\r').count();
                    (rest.get(blank) == Some(&b'\n')).then_some(("\n\n", blank + 2))
                }
                _ => None,
            };
            let Some((with, len)) = sub else {
                i += 1;
                continue;
            };
            self.exact(r.start + run..r.start + i);
            if with == "\n\n" {
                self.paragraph_break();
            } else {
                self.top().push(with, r.start + i..r.start + i + len, false, false);
            }
            i += len;
            run = i;
        }
        self.exact(r.start + run..r.end);
        if self.top().text.len() > MAX_PASSAGE {
            self.flush();
        }
    }

    fn exact(&mut self, r: Range<usize>) {
        let text = self.text;
        if let Some(s) = text.get(r.clone())
            && !s.is_empty()
        {
            self.top().push(s, r, true, false);
        }
    }

    fn placeholder(&mut self, r: Range<usize>) {
        self.top().push(PLACEHOLDER, r, false, true);
    }

    /// What stands between two ranges of prose.
    fn gap(&mut self, r: Range<usize>) {
        let text = self.text;
        let base = r.start;
        let s = text.get(r).unwrap_or_default();
        let b = s.as_bytes();
        let mut i = 0;
        // A word written with accent commands: its start, its letters, and whether it has one.
        let mut word: Option<(usize, String, bool)> = None;
        // After a command: a `[…]` here is its options.
        let mut options_at: Option<usize> = None;
        while i < b.len() {
            let c = b[i];
            let letter = c.is_ascii_alphabetic();
            // Punctuation right after a word written with accents is its (`caf\\'e,`).
            let trailing = c.is_ascii_punctuation() && !b"\\{}$%&~[]".contains(&c) && word.as_ref().is_some_and(|w| w.2);
            if !letter && !trailing && c != b'\\' && c != b'{' && c != b'}' {
                self.end_word(&mut word, base + i);
            }
            if c == b'[' && options_at == Some(i) {
                i = s[i..].find(']').map_or(b.len(), |e| i + e + 1);
                options_at = Some(i);
                continue;
            }
            match c {
                b'%' => {
                    i = s[i..].find('\n').map_or(b.len(), |e| i + e + 1);
                }
                b'{' => {
                    self.depth += 1;
                    i += 1;
                    if word.is_none() {
                        options_at = None;
                    }
                }
                b'}' => {
                    if self.units.last().and_then(|u| u.closes_at) == Some(self.depth) {
                        self.end_word(&mut word, base + i);
                        self.close_top();
                    }
                    self.depth = self.depth.saturating_sub(1);
                    i += 1;
                    options_at = None;
                }
                b'$' => {
                    self.end_word(&mut word, base + i);
                    let double = b.get(i + 1) == Some(&b'$');
                    let close = if double { "$$" } else { "$" };
                    let from = i + close.len();
                    let end = find_unescaped(&s[from..], close).map_or(b.len(), |e| from + e + close.len());
                    self.placeholder(base + i..base + end);
                    i = end;
                    options_at = None;
                }
                b'&' => {
                    self.flush();
                    i += 1;
                }
                // A tie, and the end of an option list that was prose (`\\section[Short]{Long}`).
                b'~' | b']' => {
                    self.space(base + i..base + i + 1);
                    i += 1;
                }
                b'\n' if s[i + 1..].trim_start_matches([' ', '\t', '\r']).starts_with('\n') => {
                    self.paragraph_break();
                    i += 1;
                }
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.space(base + i..base + i + 1);
                    i += 1;
                }
                b'\\' => {
                    let name_len = s[i + 1..].bytes().take_while(u8::is_ascii_alphabetic).count();
                    if name_len == 0 {
                        i = self.control_symbol(s, base, i, &mut word);
                        options_at = Some(i);
                        continue;
                    }
                    let (next, options) = self.command(s, base, i, name_len, &mut word);
                    i = next;
                    options_at = options;
                }
                _ if letter => {
                    // Letters in a gap are a key's or the rest of a word written with accents.
                    match &mut word {
                        Some(w) => w.1.push(c as char),
                        None => word = Some((base + i, (c as char).to_string(), false)),
                    }
                    i += 1;
                }
                _ if trailing => {
                    if let Some(w) = &mut word {
                        w.1.push(c as char);
                    }
                    i += 1;
                }
                _ => {
                    // A character of a key, or a byte of one: skipped, whole.
                    i += s[i..].chars().next().map_or(1, char::len_utf8);
                }
            }
        }
        self.end_word(&mut word, base + b.len());
    }

    /// The command at byte `i` of gap `s` (its name `name_len` letters): where it ends, and where
    /// a `[…]` would be its options.
    fn command(&mut self, s: &str, base: usize, i: usize, name_len: usize, word: &mut Option<(usize, String, bool)>) -> (usize, Option<usize>) {
        let name = &s[i + 1..i + 1 + name_len];
        let mut end = i + 1 + name_len;
        if let Some(stop) = accented(s, i, end, name, base, word) {
            return (stop, None);
        }
        self.end_word(word, base + i);
        if s[end..].starts_with('*') {
            end += 1;
        }
        match name {
            "begin" | "end" => {
                let stop = self.environment(s, base, i, end, name == "begin");
                (stop, Some(stop))
            }
            "verb" => {
                let delim = s[end..].chars().next();
                let stop = delim.and_then(|d| s[end + d.len_utf8()..].find(d).map(|e| end + 2 * d.len_utf8() + e)).unwrap_or(s.len());
                self.placeholder(base + i..base + stop);
                (stop, None)
            }
            _ if placeholder_command(name) => {
                // Its options and arguments are part of what the placeholder stands for.
                let stop = skip_arguments(s, end);
                self.placeholder(base + i..base + stop);
                (stop, None)
            }
            "ldots" | "dots" | "textellipsis" => {
                self.top().push("…", base + i..base + end, false, false);
                (end, None)
            }
            "quad" | "qquad" | "hfill" | "enspace" | "thinspace" | "space" => {
                self.space(base + i..base + end);
                (end, None)
            }
            _ if break_command(name) => {
                self.flush();
                (end, Some(end))
            }
            _ => {
                if let Some(continues) = unit_command(name) {
                    if !continues {
                        self.flush();
                    }
                    if self.units.len() < MAX_UNITS {
                        // It ends with the group of its argument, one deeper than here.
                        self.units.push(Unit { builder: Builder::default(), closes_at: Some(self.depth + 1) });
                    }
                }
                (end, Some(end))
            }
        }
    }

    /// `\begin{env}` (`begin`) or `\end{env}` at byte `i` of gap `s`, its name from byte `end`:
    /// where it ends. A math environment is a placeholder, a verbatim one is skipped, any other
    /// ends the passage.
    fn environment(&mut self, s: &str, base: usize, i: usize, end: usize, begin: bool) -> usize {
        let Some(env) = s[end..].strip_prefix('{').and_then(|r| r.find('}').map(|c| &r[..c])) else {
            return end;
        };
        let after = end + env.len() + 2;
        let math = crate::tree::MATH_ENVIRONMENTS.contains(&env);
        let close = format!("\\end{{{env}}}");
        let to_close = || s[after..].find(&close).map_or(s.len(), |e| after + e + close.len());
        if math && begin {
            let stop = to_close();
            self.placeholder(base + i..base + stop);
            stop
        } else if verbatim_environment(env) && begin {
            self.flush();
            to_close()
        } else {
            if !math {
                self.flush();
            }
            after
        }
    }

    /// `\\`, `\%`, `\,` and the other control symbols at byte `i` of gap `s`; where they end.
    fn control_symbol(&mut self, s: &str, base: usize, i: usize, word: &mut Option<(usize, String, bool)>) -> usize {
        let Some(c) = s[i + 1..].chars().next() else {
            return s.len();
        };
        let end = i + 1 + c.len_utf8();
        match c {
            '\\' => {
                self.end_word(word, base + i);
                self.flush();
                // `\\[2pt]`, `\\*`.
                let rest = &s[end..];
                let rest_star = rest.strip_prefix('*').unwrap_or(rest);
                let star = rest.len() - rest_star.len();
                if rest_star.starts_with('[') {
                    return rest_star.find(']').map_or(s.len(), |e| end + star + e + 1);
                }
                end + star
            }
            '%' | '&' | '#' | '$' | '_' | '{' | '}' => {
                self.end_word(word, base + i);
                let mut buf = [0u8; 4];
                self.top().push(c.encode_utf8(&mut buf), base + i..base + end, false, false);
                end
            }
            ' ' | ',' | ';' | ':' | '!' => {
                self.end_word(word, base + i);
                self.space(base + i..base + end);
                end
            }
            '(' | '[' => {
                self.end_word(word, base + i);
                let close = if c == '(' { "\\)" } else { "\\]" };
                let stop = s[end..].find(close).map_or(s.len(), |e| end + e + 2);
                self.placeholder(base + i..base + stop);
                stop
            }
            '\'' | '`' | '^' | '"' | '~' | '=' | '.' => {
                // An accent on the next letter, perhaps in braces: the word goes on.
                let rest = &s[end..];
                let (letter, len) = match rest.strip_prefix('{') {
                    Some(inner) => inner.find('}').map_or((String::new(), 0), |close| (inner[..close].to_owned(), close + 2)),
                    None => rest.chars().next().filter(char::is_ascii_alphabetic).map_or((String::new(), 0), |c| (c.to_string(), 1)),
                };
                let w = word.get_or_insert_with(|| (base + i, String::new(), false));
                w.1.push_str(&letter);
                w.2 = true;
                end + len
            }
            // `\-`, `\@`, `\/`: nothing.
            _ => end,
        }
    }

    /// The word being read in a gap ends at source byte `at`: written with accents, it goes in as
    /// its letters.
    fn end_word(&mut self, word: &mut Option<(usize, String, bool)>, at: usize) {
        if let Some((start, letters, accented)) = word.take()
            && accented
        {
            self.top().push(&letters, start..at, false, false);
        }
    }
}

/// The options and brace arguments of a command whose name ends at byte `at` of `s`: where they
/// end.
fn skip_arguments(s: &str, mut at: usize) -> usize {
    loop {
        let rest = &s[at..];
        if rest.starts_with('[') {
            at = rest.find(']').map_or(s.len(), |e| at + e + 1);
        } else if rest.starts_with('{') {
            let mut depth = 0usize;
            let mut end = s.len();
            for (k, c) in rest.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = at + k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            at = end;
        } else {
            return at;
        }
    }
}

/// Where `close` is in `s`, not after a backslash.
fn find_unescaped(s: &str, close: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(k) = s[from..].find(close) {
        let at = from + k;
        if at == 0 || s.as_bytes()[at - 1] != b'\\' {
            return Some(at);
        }
        from = at + close.len();
    }
    None
}

/// An accent command at byte `i` of gap `s`, its name ending at `end`: its letters added to the
/// word being read, and where it ends; `None` when `name` is no accent.
fn accented(s: &str, i: usize, end: usize, name: &str, base: usize, word: &mut Option<(usize, String, bool)>) -> Option<usize> {
    let (letters, stop) = if let Some(letters) = accent_letters(name) {
        // `\ss{}` ends with its group.
        (letters.to_owned(), if s[end..].starts_with("{}") { end + 2 } else { end })
    } else if accent_with_argument(name) {
        let rest = &s[end..];
        let arg = rest.trim_start_matches(' ');
        let skipped = rest.len() - arg.len();
        let (letter, len) = match arg.strip_prefix('{') {
            Some(inner) => inner.find('}').map(|close| (inner[..close].to_owned(), close + 2))?,
            None => arg.chars().next().filter(char::is_ascii_alphabetic).map(|c| (c.to_string(), 1))?,
        };
        (letter, end + skipped + len)
    } else {
        return None;
    };
    let w = word.get_or_insert_with(|| (base + i, String::new(), false));
    w.1.push_str(&letters);
    w.2 = true;
    Some(stop)
}
