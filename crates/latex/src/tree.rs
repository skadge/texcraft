//! A tolerant syntax tree of LaTeX source, for the formatted view (docs/plan.md, phase 3, item 1).
//!
//! [`parse`] never fails. Every byte of the text belongs to exactly one top-level node, and the
//! nodes of each sequence (a group's body, an argument's, an environment's) follow each other with
//! no gap, so the view can map anything it shows back to the source span it stands for. What is
//! unbalanced or half-typed comes out as nodes marked unclosed, or as [`Kind::Error`] for a stray
//! `}` or `\end`, which cover their text exactly.
//!
//! The parser knows the arguments of the commands the view shows ([`signature`]); the groups after
//! any other command stay groups of their own, as TeX reads them until the command is expanded.
//!
//! Formulas end where TeX would complain: at their closing delimiter, or at the end of their
//! paragraph, a `}` that closes a group around them, or an `\end` of an environment around them.

use std::ops::Range;

/// Nesting deeper than this is kept as text: hostile input cannot exhaust the stack.
const MAX_DEPTH: usize = 96;

/// Environments whose body is math.
pub const MATH_ENVIRONMENTS: &[&str] = &[
    "equation",
    "equation*",
    "align",
    "align*",
    "alignat",
    "alignat*",
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

/// Environments whose body is read as it is.
const VERBATIM_ENVIRONMENTS: &[&str] = &["verbatim", "verbatim*", "Verbatim", "lstlisting", "minted", "comment"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    /// The bytes of the text it stands for.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Characters with no structure, line ends included (but not blank lines).
    Text,
    /// A line end followed by one or more blank lines: the end of a paragraph.
    ParBreak,
    /// From `%` to the end of its line (the line end is text).
    Comment,
    /// `& ~ ^ _ #`.
    Special,
    /// `\` and one character that is not a letter: `\%`, `\\`, `\,`.
    ControlSymbol,
    /// A control word with the arguments its [`signature`] gives (spaces between them included in
    /// the range). `name` has no backslash and no star.
    Command { name: String, star: bool, args: Vec<Arg> },
    /// `{…}`; `closed` is false when the text ends (or an enclosing group closes) first.
    Group { body: Vec<Node>, closed: bool },
    /// `\begin{name}` with its arguments (`begin`), the body, and `\end{name}` (`None` when missing).
    Environment { name: String, begin: Range<usize>, args: Vec<Arg>, body: Vec<Node>, end: Option<Range<usize>> },
    /// `$…$`, `\(…\)` inline; `$$…$$`, `\[…\]` and the math environments displayed. `body` is
    /// the math without its delimiters.
    Math { display: bool, env: Option<String>, body: Range<usize>, closed: bool },
    /// `\verb|…|` and the verbatim environments: `body` is the text as it is.
    Verbatim { body: Range<usize>, closed: bool },
    /// A `}` or an `\end{…}` that closes nothing.
    Error,
}

/// An argument of a command or an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    /// `[…]` rather than `{…}`.
    pub optional: bool,
    /// With its brackets or braces.
    pub range: Range<usize>,
    /// Without them.
    pub inner: Range<usize>,
    pub body: Vec<Node>,
    pub closed: bool,
}

/// What a command takes, as far as the view needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec {
    Optional,
    Required,
}

use Spec::{Optional as O, Required as R};

/// The arguments of the commands and environments the formatted view shows; others take none.
pub fn signature(name: &str) -> &'static [Spec] {
    match name {
        "part" | "chapter" | "section" | "subsection" | "subsubsection" | "paragraph" | "subparagraph" => &[O, R],
        "textbf" | "textit" | "emph" | "underline" | "texttt" | "textsc" | "textsf" | "textrm" | "textsl" | "textup" | "textmd" | "textnormal"
        | "mbox" | "url" | "label" | "ref" | "eqref" | "pageref" | "autoref" | "nameref" | "cref" | "Cref" | "input" | "include" | "includeonly"
        | "bibliography" | "bibliographystyle" | "addbibresource" | "hspace" | "vspace" => &[R],
        "cite" | "citep" | "citet" | "parencite" | "textcite" | "autocite" | "footcite" | "nocite" | "citeauthor" | "citeyear" => &[O, O, R],
        "footnote" | "thanks" | "caption" | "title" | "author" | "date" | "documentclass" | "usepackage" | "includegraphics" => &[O, R],
        "href" => &[R, R],
        "item" => &[O],
        "newcommand" | "renewcommand" | "providecommand" => &[R, O, O, R],
        // Environments.
        "figure" | "figure*" | "table" | "table*" => &[O],
        "tabular" | "array" => &[O, R],
        "tabular*" | "tabularx" => &[R, O, R],
        "minipage" => &[O, O, O, R],
        "thebibliography" => &[R],
        _ => &[],
    }
}

/// The tree of `text`: nodes that cover it in order, with no gap.
pub fn parse(text: &str) -> Vec<Node> {
    let mut p = Parser { text, i: 0, envs: Vec::new(), braces: 0 };
    let mut out = p.seq(&Until::Eof, 0);
    // The top level stops only at the text's end; this is a guard, not a case.
    if p.i < text.len() {
        out.push(Node { kind: Kind::Error, range: p.i..text.len() });
    }
    out
}

#[derive(PartialEq, Eq)]
enum Until {
    Eof,
    Brace,
    Bracket,
    End(String),
}

struct Parser<'a> {
    text: &'a str,
    i: usize,
    /// The environments open around the position, innermost last.
    envs: Vec<String>,
    /// How many `{` inside the innermost environment wait for their `}`.
    braces: usize,
}

/// Appends `node`, merging text with the text before it.
fn push(out: &mut Vec<Node>, node: Node) {
    if node.range.is_empty() {
        return;
    }
    if node.kind == Kind::Text
        && let Some(last) = out.last_mut()
        && last.kind == Kind::Text
        && last.range.end == node.range.start
    {
        last.range.end = node.range.end;
        return;
    }
    out.push(node);
}

/// Characters that end a run of plain text.
const TEXT_STOPS: &[char] = &['%', '\\', '$', '{', '}', '&', '~', '^', '_', '#', '\n', ']'];

impl Parser<'_> {
    fn byte(&self, at: usize) -> Option<u8> {
        self.text.as_bytes().get(at).copied()
    }

    /// If the line end at `at` is followed by blank lines, the end of the last of them.
    fn blank_line_end(&self, at: usize) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let mut j = at + 1;
        let mut blank_end = None;
        loop {
            while matches!(bytes.get(j), Some(b' ' | b'\t' | b'\r')) {
                j += 1;
            }
            if bytes.get(j) != Some(&b'\n') {
                return blank_end;
            }
            j += 1;
            blank_end = Some(j);
        }
    }

    /// `\end{name}` at `at`: its name and the index after it.
    fn end_at(&self, at: usize) -> Option<(&str, usize)> {
        let after = self.text.get(at..)?.strip_prefix("\\end")?;
        if after.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
            return None;
        }
        let (name, end) = env_name(self.text, at + 4)?;
        Some((&self.text[name], end))
    }

    /// Whether `at` is the `\end` of an environment open around it.
    fn ends_enclosing(&self, at: usize) -> bool {
        self.end_at(at).is_some_and(|(name, _)| self.envs.iter().any(|e| e == name))
    }

    fn seq(&mut self, until: &Until, depth: usize) -> Vec<Node> {
        let mut out = Vec::new();
        let len = self.text.len();
        while let Some(b) = self.byte(self.i) {
            let start = self.i;
            let node = match b {
                b'}' if *until == Until::Brace || self.braces > 0 => break,
                b']' if *until == Until::Bracket => break,
                b'}' => {
                    self.i += 1;
                    Node { kind: Kind::Error, range: start..self.i }
                }
                b'%' => {
                    self.i = self.text[start..].find('\n').map_or(len, |n| start + n);
                    Node { kind: Kind::Comment, range: start..self.i }
                }
                b'\n' => match self.blank_line_end(start) {
                    Some(end) => {
                        self.i = end;
                        Node { kind: Kind::ParBreak, range: start..end }
                    }
                    None => {
                        self.i += 1;
                        Node { kind: Kind::Text, range: start..self.i }
                    }
                },
                b'&' | b'~' | b'^' | b'_' | b'#' => {
                    self.i += 1;
                    Node { kind: Kind::Special, range: start..self.i }
                }
                b'$' => self.dollar_math(),
                b'{' => self.group(depth),
                b'\\' => match self.end_at(start) {
                    Some((name, end)) => {
                        if *until == Until::End(name.to_owned()) || self.envs.iter().any(|e| e == name) {
                            break;
                        }
                        self.i = end;
                        Node { kind: Kind::Error, range: start..end }
                    }
                    None => self.control(depth),
                },
                _ => {
                    // A `]` that ends nothing here is text; so is everything up to the next stop.
                    let from = next_char_end(self.text, start);
                    let end = self.text[from..].find(TEXT_STOPS).map_or(len, |n| from + n);
                    self.i = end;
                    Node { kind: Kind::Text, range: start..end }
                }
            };
            push(&mut out, node);
        }
        out
    }

    /// `{…}` at the position.
    fn group(&mut self, depth: usize) -> Node {
        let start = self.i;
        if depth >= MAX_DEPTH {
            self.i = raw_group_end(self.text, start);
            return Node { kind: Kind::Text, range: start..self.i };
        }
        self.i += 1;
        self.braces += 1;
        let body = self.seq(&Until::Brace, depth + 1);
        self.braces -= 1;
        let closed = self.byte(self.i) == Some(b'}');
        if closed {
            self.i += 1;
        }
        Node { kind: Kind::Group { body, closed }, range: start..self.i }
    }

    /// The arguments `specs` after the position, skipping the spaces (and one line end) before each.
    /// A missing optional argument is skipped; a missing required one ends the list.
    fn args(&mut self, specs: &[Spec], depth: usize) -> Vec<Arg> {
        let mut out = Vec::new();
        if depth >= MAX_DEPTH {
            return out;
        }
        for spec in specs {
            let at = self.skip_space(self.i);
            let optional = *spec == Spec::Optional;
            let (open, close, until) = if optional { (b'[', b']', Until::Bracket) } else { (b'{', b'}', Until::Brace) };
            if self.byte(at) != Some(open) {
                if optional {
                    continue;
                }
                break;
            }
            self.i = at + 1;
            if !optional {
                self.braces += 1;
            }
            let body = self.seq(&until, depth + 1);
            if !optional {
                self.braces -= 1;
            }
            let inner = at + 1..self.i;
            let closed = self.byte(self.i) == Some(close);
            if closed {
                self.i += 1;
            }
            out.push(Arg { optional, range: at..self.i, inner, body, closed });
        }
        out
    }

    /// After spaces and tabs, and at most one line end (a blank line ends the arguments).
    fn skip_space(&self, from: usize) -> usize {
        let bytes = self.text.as_bytes();
        let mut j = from;
        let mut newline = false;
        while let Some(b) = bytes.get(j) {
            match b {
                b' ' | b'\t' | b'\r' => j += 1,
                b'\n' if !newline => {
                    newline = true;
                    j += 1;
                }
                _ => break,
            }
        }
        j
    }

    /// A control sequence at the position: a command, math, an environment or `\verb`.
    fn control(&mut self, depth: usize) -> Node {
        let start = self.i;
        let end = control_end(self.text, start);
        let word = &self.text[start + 1..end];
        self.i = end;
        if !word.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return match word {
                "(" => self.delimited_math(start, "\\)", false),
                "[" => self.delimited_math(start, "\\]", true),
                _ => Node { kind: Kind::ControlSymbol, range: start..end },
            };
        }
        let (name, star) = match word.strip_suffix('*') {
            Some(n) => (n, true),
            None => (word, false),
        };
        match name {
            "begin" => self.environment(start, depth),
            "verb" => self.verb(start),
            _ => {
                let args = self.args(signature(name), depth);
                Node { kind: Kind::Command { name: name.to_owned(), star, args }, range: start..self.i }
            }
        }
    }

    /// Where the formula starting at `from` ends: the start of `close` and the index after it, or,
    /// when it is not closed, where it stops. `delimited` formulas (not environments) also stop at
    /// the end of their paragraph and at a `}` that closes a group around them.
    fn math_end(&self, from: usize, close: &str, delimited: bool) -> (usize, Option<usize>) {
        let bytes = self.text.as_bytes();
        let mut depth = 0usize;
        let mut j = from;
        while j < bytes.len() {
            if bytes[j..].starts_with(close.as_bytes()) {
                return (j, Some(j + close.len()));
            }
            match bytes[j] {
                b'\\' if self.ends_enclosing(j) => return (j, None),
                b'\\' => j = control_end(self.text, j),
                b'%' => j = self.text[j..].find('\n').map_or(bytes.len(), |n| j + n),
                b'{' => {
                    depth += 1;
                    j += 1;
                }
                b'}' if delimited && depth == 0 && self.braces > 0 => return (j, None),
                b'}' => {
                    depth = depth.saturating_sub(1);
                    j += 1;
                }
                b'\n' if delimited && self.blank_line_end(j).is_some() => return (j, None),
                _ => j += 1,
            }
        }
        (bytes.len(), None)
    }

    /// `\(…\)` or `\[…\]`; the position is after the opening delimiter at `start`.
    fn delimited_math(&mut self, start: usize, close: &str, display: bool) -> Node {
        let body_start = self.i;
        let (body_end, end) = self.math_end(body_start, close, true);
        self.i = end.unwrap_or(body_end);
        Node { kind: Kind::Math { display, env: None, body: body_start..body_end, closed: end.is_some() }, range: start..self.i }
    }

    /// `$…$` or `$$…$$` at the position.
    fn dollar_math(&mut self) -> Node {
        let start = self.i;
        let display = self.byte(start + 1) == Some(b'$');
        let close = if display { "$$" } else { "$" };
        let body_start = start + close.len();
        let (body_end, end) = self.math_end(body_start, close, true);
        self.i = end.unwrap_or(body_end);
        Node { kind: Kind::Math { display, env: None, body: body_start..body_end, closed: end.is_some() }, range: start..self.i }
    }

    /// `\verb` and its delimited text, on one line.
    fn verb(&mut self, start: usize) -> Node {
        let Some(delim) = self.text[self.i..].chars().next().filter(|c| !c.is_whitespace() && !c.is_ascii_alphabetic()) else {
            return Node { kind: Kind::Command { name: "verb".to_owned(), star: false, args: Vec::new() }, range: start..self.i };
        };
        let body_start = self.i + delim.len_utf8();
        let line_end = self.text[body_start..].find('\n').map_or(self.text.len(), |n| body_start + n);
        let (body_end, end) = match self.text[body_start..line_end].find(delim) {
            Some(n) => (body_start + n, body_start + n + delim.len_utf8()),
            None => (line_end, line_end),
        };
        self.i = end;
        Node { kind: Kind::Verbatim { body: body_start..body_end, closed: end > body_end }, range: start..end }
    }

    /// `\begin{name}…\end{name}`; the position is after `\begin`.
    fn environment(&mut self, start: usize, depth: usize) -> Node {
        let Some((name_range, after)) = env_name(self.text, self.i) else {
            return Node { kind: Kind::Command { name: "begin".to_owned(), star: false, args: Vec::new() }, range: start..self.i };
        };
        let name = self.text[name_range].to_owned();
        self.i = after;
        let end_tag = format!("\\end{{{name}}}");
        if VERBATIM_ENVIRONMENTS.contains(&name.as_str()) {
            let (body_end, end) = match self.text[after..].find(&end_tag) {
                Some(n) => (after + n, after + n + end_tag.len()),
                None => (self.text.len(), self.text.len()),
            };
            self.i = end;
            return Node { kind: Kind::Verbatim { body: after..body_end, closed: end > body_end }, range: start..end };
        }
        if MATH_ENVIRONMENTS.contains(&name.as_str()) {
            let (body_end, end) = self.math_end(after, &end_tag, false);
            self.i = end.unwrap_or(body_end);
            let closed = end.is_some();
            return Node { kind: Kind::Math { display: true, env: Some(name), body: after..body_end, closed }, range: start..self.i };
        }
        if depth >= MAX_DEPTH {
            return Node { kind: Kind::Text, range: start..self.i };
        }
        let args = self.args(signature(&name), depth);
        let begin = start..self.i;
        self.envs.push(name.clone());
        // An environment's body is its own: the braces around it do not end it.
        let braces = std::mem::take(&mut self.braces);
        let body = self.seq(&Until::End(name.clone()), depth + 1);
        self.braces = braces;
        self.envs.pop();
        let end = match self.end_at(self.i) {
            Some((found, end)) if found == name => {
                let range = self.i..end;
                self.i = end;
                Some(range)
            }
            _ => None,
        };
        Node { kind: Kind::Environment { name, begin, args, body, end }, range: start..self.i }
    }
}

/// The end of the character at `at`.
fn next_char_end(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(text.len(), |c| at + c.len_utf8())
}

/// Where the control sequence at `start` (a `\`) ends: a word with an optional star, or one character.
fn control_end(text: &str, start: usize) -> usize {
    let rest = &text[start + 1..];
    let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters > 0 {
        let star = usize::from(rest.as_bytes().get(letters) == Some(&b'*'));
        return start + 1 + letters + star;
    }
    match rest.chars().next() {
        Some(c) => start + 1 + c.len_utf8(),
        None => start + 1,
    }
}

/// `{name}` at `at` (after spaces): the name's range and the index after `}`, on one line.
fn env_name(text: &str, at: usize) -> Option<(Range<usize>, usize)> {
    let rest = text.get(at..)?;
    let open = rest.bytes().take_while(|b| *b == b' ').count();
    if rest.as_bytes().get(open) != Some(&b'{') {
        return None;
    }
    let name_start = at + open + 1;
    let close = text[name_start..].find(['}', '\n', '{', '\\', '%'])?;
    if text.as_bytes().get(name_start + close) != Some(&b'}') || close == 0 {
        return None;
    }
    Some((name_start..name_start + close, name_start + close + 1))
}

/// The end of the group opened at `start`, counting braces without parsing (too deep to parse).
fn raw_group_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut j = start;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => {
                j = control_end(text, j);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    text.len()
}

/// Whether `inner` lies inside `outer`.
fn within(inner: &Range<usize>, outer: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.start <= inner.end && inner.end <= outer.end
}

/// Whether `args` follow each other inside `range`, each tiled by its body.
fn args_tile(args: &[Arg], range: &Range<usize>) -> bool {
    let mut at = range.start;
    args.iter().all(|a| {
        let ok = at <= a.range.start
            && within(&a.range, range)
            && within(&a.inner, &a.range)
            && a.inner.start == a.range.start + 1
            && a.inner.end + usize::from(a.closed) == a.range.end
            && tiles(&a.body, a.inner.clone());
        at = a.range.end;
        ok
    })
}

/// Checks that `nodes` tile `range` with no gap, and that every sequence inside them tiles its own
/// range: a group's body between its braces, an argument's between its brackets, an environment's
/// from after its `\begin` and arguments to its `\end` (or its own end). The tests and the
/// hostile-input sweep use it.
pub fn tiles(nodes: &[Node], range: Range<usize>) -> bool {
    let mut at = range.start;
    for n in nodes {
        if n.range.start != at || n.range.end < n.range.start || n.range.is_empty() {
            return false;
        }
        at = n.range.end;
        let r = &n.range;
        let inner_ok = match &n.kind {
            Kind::Group { body, closed } => tiles(body, r.start + 1..r.end - usize::from(*closed)),
            Kind::Environment { begin, body, args, end, .. } => {
                let body_end = end.as_ref().map_or(r.end, |e| e.start);
                begin.start == r.start
                    && end.as_ref().is_none_or(|e| e.end == r.end)
                    && args_tile(args, begin)
                    && begin.end <= body_end
                    && tiles(body, begin.end..body_end)
            }
            Kind::Command { args, .. } => args_tile(args, r),
            Kind::Math { body, .. } | Kind::Verbatim { body, .. } => within(body, r),
            Kind::Text | Kind::ParBreak | Kind::Comment | Kind::Special | Kind::ControlSymbol | Kind::Error => true,
        };
        if !inner_ok {
            return false;
        }
    }
    at == range.end
}
