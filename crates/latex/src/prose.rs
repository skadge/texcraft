//! The prose of a LaTeX file, for spell checking (docs/ROADMAP.md, E11): the byte ranges of the
//! text a reader reads, without commands, their keys and options, math, verbatim text and (unless
//! asked for) comments; and the document's language as babel or polyglossia set it.
//!
//! The ranges come from the [`tree`]: text nodes, and the arguments of the commands whose
//! arguments are text (`\section{…}`, `\emph{…}`, `\caption{…}`). The arguments of other commands
//! are names and keys (`\label{…}`, `\cite{…}`, `\usepackage{…}`, `\setlength{…}{…}`); environment
//! names and their arguments too. Before `\begin{document}` only the title, author, date and
//! thanks are prose. A word joined to an accent command (`caf\'e`, `\"{u}ber`) is left out: its
//! letters are not the word as written.

use std::ops::Range;

use crate::tree::{self, Kind, Node};

/// What the prose includes besides the text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The text of comments (after `%`).
    pub comments: bool,
}

/// The prose of `text`, in order, as byte ranges that do not overlap.
pub fn ranges(text: &str, opts: Options) -> Vec<Range<usize>> {
    let nodes = tree::parse(text);
    let mut w = Walker { text, opts, out: Vec::new() };
    // In a file with a document environment, what comes before it is the preamble.
    let document = nodes.iter().position(|n| matches!(&n.kind, Kind::Environment { name, .. } if name == "document"));
    match document {
        Some(at) => {
            for n in &nodes[..at] {
                w.preamble(n);
            }
            w.seq(&nodes[at..]);
        }
        None => w.seq(&nodes),
    }
    w.out
}

/// Commands whose arguments, all of them, are text.
fn prose_command(name: &str) -> bool {
    matches!(
        name,
        "part"
            | "chapter"
            | "section"
            | "subsection"
            | "subsubsection"
            | "paragraph"
            | "subparagraph"
            | "textbf"
            | "textit"
            | "emph"
            | "underline"
            | "textsc"
            | "textsf"
            | "textrm"
            | "textsl"
            | "textup"
            | "textmd"
            | "textnormal"
            | "mbox"
            | "footnote"
            | "thanks"
            | "caption"
            | "title"
            | "author"
            | "date"
            | "item"
    )
}

/// Commands the tree does not give arguments to whose first `n` brace groups are names, keys,
/// lengths or code, not text.
fn key_groups(name: &str) -> usize {
    match name {
        "definecolor" | "newenvironment" | "renewenvironment" | "NewDocumentCommand" | "RenewDocumentCommand" | "newacronym" => 3,
        "setlength"
        | "addtolength"
        | "setcounter"
        | "addtocounter"
        | "fcolorbox"
        | "newtheorem"
        | "DeclareMathOperator"
        | "inputminted"
        | "mintinline"
        | "newfontfamily"
        | "foreignlanguage"
        | "DeclareRobustCommand"
        | "SI"
        | "qty"
        | "crefrange"
        | "import"
        | "subimport"
        | "multicolumn"
        | "multirow"
        | "rule"
        | "resizebox"
        | "fontsize" => 2,
        "stepcounter" | "refstepcounter" | "color" | "pagecolor" | "textcolor" | "colorbox" | "pagestyle" | "thispagestyle" | "pagenumbering"
        | "geometry" | "hypersetup" | "usetikzlibrary" | "usepgfplotslibrary" | "bibitem" | "lstinputlisting" | "lstinline" | "graphicspath"
        | "RequirePackage" | "LoadClass" | "ProvidesPackage" | "ProvidesClass" | "NeedsTeXFormat" | "setmainfont" | "setsansfont" | "setmonofont"
        | "setmainlanguage" | "setdefaultlanguage" | "setotherlanguage" | "setotherlanguages" | "selectlanguage" | "captionsetup" | "tikzset"
        | "pgfplotsset" | "newlength" | "newcounter" | "si" | "num" | "unit" | "ang" | "path" | "nolinkurl" | "vref" | "Vref" | "cpageref"
        | "labelcref" | "citealp" | "citealt" | "citenum" | "fullcite" | "footfullcite" | "smartcite" | "supercite" | "Cite" | "Textcite"
        | "Parencite" | "Autocite" | "citetitle" | "citeurl" | "citedate" | "gls" | "Gls" | "glspl" | "Glspl" | "glsadd" | "acrshort" | "acrlong"
        | "acrfull" | "ac" | "acp" | "acs" | "acl" | "includepdf" | "includesvg" | "subfile" | "cline" | "hhline" | "raisebox" | "scalebox"
        | "rotatebox" | "parbox" | "fontfamily" | "linespread" | "enlargethispage" | "phantom" | "hphantom" | "vphantom" | "index" | "texttt"
        | "verbatiminput" | "hyphenation" | "documentclass" | "usepackage" => 1,
        _ => 0,
    }
}

/// Environments whose body is drawing or code, not text.
pub(crate) fn code_environment(name: &str) -> bool {
    matches!(
        name,
        "tikzpicture"
            | "pgfpicture"
            | "axis"
            | "circuitikz"
            | "pspicture"
            | "forest"
            | "filecontents"
            | "filecontents*"
            | "asy"
            | "dot2tex"
            | "gnuplot"
            | "sagesilent"
            | "sageblock"
            | "pycode"
            | "luacode"
            | "luacode*"
    )
}

/// Accents and letters written as commands (`\'`, `\"`, `\c`, `\ss`): a word they touch is not
/// written out.
fn accent(text: &str, node: &Node) -> bool {
    match &node.kind {
        Kind::ControlSymbol => matches!(text.get(node.range.start + 1..node.range.end), Some("'" | "`" | "^" | "\"" | "~" | "=" | ".")),
        Kind::Command { name, args, .. } => {
            args.is_empty()
                && matches!(
                    name.as_str(),
                    "c" | "v"
                        | "u"
                        | "H"
                        | "r"
                        | "k"
                        | "b"
                        | "d"
                        | "t"
                        | "ss"
                        | "o"
                        | "O"
                        | "ae"
                        | "AE"
                        | "oe"
                        | "OE"
                        | "aa"
                        | "AA"
                        | "l"
                        | "L"
                        | "i"
                        | "j"
                )
        }
        _ => false,
    }
}

struct Walker<'a> {
    text: &'a str,
    opts: Options,
    out: Vec<Range<usize>>,
}

impl Walker<'_> {
    fn push(&mut self, r: Range<usize>) {
        if r.start < r.end {
            self.out.push(r);
        }
    }

    /// A node of the preamble: only the title's commands are prose.
    fn preamble(&mut self, n: &Node) {
        if let Kind::Command { name, args, .. } = &n.kind
            && matches!(name.as_str(), "title" | "author" | "date" | "thanks")
        {
            for a in args {
                self.seq(&a.body);
            }
        }
    }

    fn seq(&mut self, nodes: &[Node]) {
        // Brace groups after a key command still to skip, and whether an option list may follow.
        let mut skip = 0usize;
        // The end of the last command, where a `[…]` option list may start.
        let mut options_at: Option<usize> = None;
        for (i, n) in nodes.iter().enumerate() {
            let options = options_at.take();
            match &n.kind {
                Kind::Text => {
                    let mut r = n.range.clone();
                    let s = self.text.get(r.clone()).unwrap_or_default();
                    // An option list right after a command (`\hyperref[sec:a]`, `\\[2pt]`).
                    if (options == Some(r.start) || skip > 0) && s.starts_with('[') {
                        match s.find(']') {
                            Some(close) => r.start += close + 1,
                            None => continue,
                        }
                    }
                    let rest = self.text.get(r.clone()).unwrap_or_default();
                    if skip > 0 {
                        if rest.trim().is_empty() {
                            continue;
                        }
                        skip = 0;
                    }
                    self.text_node(nodes, i, r);
                }
                Kind::Group { body, .. } => {
                    // An accented letter (`\"{u}`) is part of a word that is not written out.
                    let accented = i.checked_sub(1).and_then(|p| nodes.get(p)).is_some_and(|p| p.range.end == n.range.start && accent(self.text, p));
                    if accented {
                        skip = 0;
                        continue;
                    }
                    if skip > 0 {
                        skip -= 1;
                        options_at = (skip > 0).then_some(n.range.end);
                        continue;
                    }
                    self.seq(body);
                }
                Kind::Comment => {
                    skip = 0;
                    if self.opts.comments {
                        self.push(n.range.start + 1..n.range.end);
                    }
                }
                Kind::Command { name, args, .. } => {
                    skip = 0;
                    if prose_command(name) {
                        for a in args {
                            self.seq(&a.body);
                        }
                    } else if name == "href" {
                        if let Some(a) = args.get(1) {
                            self.seq(&a.body);
                        }
                    } else if args.is_empty() {
                        skip = key_groups(name);
                    }
                    options_at = Some(n.range.end);
                }
                Kind::ControlSymbol => {
                    skip = 0;
                    options_at = Some(n.range.end);
                }
                Kind::Environment { name, begin, body, .. } => {
                    skip = 0;
                    if !code_environment(name) {
                        // An option list or a key group right after `\begin{…}` is the
                        // environment's argument.
                        let mut w = Walker { text: self.text, opts: self.opts, out: Vec::new() };
                        let skip_first = body.first().is_some_and(|f| f.range.start == begin.end && matches!(f.kind, Kind::Group { .. }));
                        let body = if skip_first { body.get(1..).unwrap_or_default() } else { body.as_slice() };
                        w.seq(body);
                        let bracket = |r: &Range<usize>| r.start == begin.end && self.text.get(r.clone()).is_some_and(|s| s.starts_with('['));
                        for mut r in w.out {
                            if bracket(&r) {
                                let s = self.text.get(r.clone()).unwrap_or_default();
                                r.start += s.find(']').map_or(s.len(), |c| c + 1);
                            }
                            self.push(r);
                        }
                    }
                }
                Kind::Math { .. } | Kind::Verbatim { .. } | Kind::Special | Kind::ParBreak | Kind::Error => skip = 0,
            }
        }
    }

    /// A text node's range `r`, less a word at either end that an accent command touches.
    fn text_node(&mut self, nodes: &[Node], i: usize, mut r: Range<usize>) {
        let text = self.text;
        let prev = i.checked_sub(1).and_then(|p| nodes.get(p));
        let glued_before = prev.is_some_and(|p| {
            p.range.end == r.start
                && (accent(text, p)
                    || (matches!(p.kind, Kind::Group { .. }) && i.checked_sub(2).and_then(|q| nodes.get(q)).is_some_and(|q| accent(text, q))))
        });
        let glued_after = nodes.get(i + 1).is_some_and(|n| n.range.start == r.end && accent(text, n));
        let s = text.get(r.clone()).unwrap_or_default();
        if glued_before && !s.starts_with(char::is_whitespace) {
            r.start += s.find(char::is_whitespace).unwrap_or(s.len());
        }
        let s = text.get(r.clone()).unwrap_or_default();
        if glued_after && !s.ends_with(char::is_whitespace) {
            r.end = r.start + s.rfind(char::is_whitespace).map_or(0, |w| w + 1);
        }
        self.push(r);
    }
}

/// The document's language as a BCP 47 tag, from babel's options (its `main=`, or the last
/// language named, also among the class options) or polyglossia's `\setdefaultlanguage` /
/// `\setmainlanguage`. `None` when the preamble names none this knows.
pub fn document_language(text: &str) -> Option<&'static str> {
    let nodes = tree::parse(text);
    let mut class_options: Vec<String> = Vec::new();
    let mut found: Option<&'static str> = None;
    let arg_text = |a: &tree::Arg| text.get(a.inner.clone()).unwrap_or_default().to_owned();
    for (i, n) in nodes.iter().enumerate() {
        if matches!(&n.kind, Kind::Environment { name, .. } if name == "document") {
            break;
        }
        let Kind::Command { name, args, .. } = &n.kind else { continue };
        match name.as_str() {
            "documentclass" => {
                class_options = args.iter().filter(|a| a.optional).map(arg_text).collect();
            }
            "usepackage" => {
                let packages = args.iter().find(|a| !a.optional).map(arg_text).unwrap_or_default();
                if !packages.split(',').any(|p| p.trim() == "babel") {
                    continue;
                }
                let options: Vec<String> = args.iter().filter(|a| a.optional).map(arg_text).collect();
                let all: Vec<&str> = class_options.iter().chain(options.iter()).flat_map(|o| o.split(',')).map(str::trim).collect();
                let main = all.iter().find_map(|o| o.strip_prefix("main=")).and_then(|m| babel_tag(m.trim()));
                if let Some(tag) = main.or_else(|| all.iter().rev().find_map(|o| babel_tag(o))) {
                    found = Some(tag);
                }
            }
            "setdefaultlanguage" | "setmainlanguage" => {
                // The tree gives these no arguments: the next group is the language, after
                // options such as `[variant=british]`.
                let after = nodes.get(i + 1..).unwrap_or_default();
                let group = after.iter().position(|g| !matches!(g.kind, Kind::Text) || !is_blank_or_option(text, g));
                let options: String = after.get(..group.unwrap_or(0)).unwrap_or_default().iter().filter_map(|n| text.get(n.range.clone())).collect();
                if let Some(Node { kind: Kind::Group { .. }, range }) = group.and_then(|g| after.get(g))
                    && let Some(name) = text.get(range.start + 1..range.end.saturating_sub(1)).map(str::trim)
                    && let Some(tag) = variant_tag(name, &options).or_else(|| babel_tag(name))
                {
                    found = Some(tag);
                }
            }
            _ => {}
        }
    }
    found
}

/// The tag of polyglossia's English with a `variant` option (`[variant=british]`, `[variant=uk]`,
/// `[variant=us]`), when `options` (the text before the language's group) gives one.
fn variant_tag(name: &str, options: &str) -> Option<&'static str> {
    if name != "english" {
        return None;
    }
    let inside = options.trim().strip_prefix('[')?.strip_suffix(']')?;
    let variant = inside.split(',').find_map(|o| o.trim().strip_prefix("variant").map(str::trim)?.strip_prefix('=')).map(str::trim)?;
    Some(match variant {
        "british" | "uk" => "en-GB",
        "american" | "us" | "usmax" => "en-US",
        "australian" => "en-AU",
        "canadian" => "en-CA",
        "newzealand" => "en-NZ",
        _ => return None,
    })
}

/// Whether text node `n` is blank or an option list (`[variant=british]`).
fn is_blank_or_option(text: &str, n: &Node) -> bool {
    let s = text.get(n.range.clone()).unwrap_or_default().trim();
    s.is_empty() || (s.starts_with('[') && s.ends_with(']'))
}

/// The BCP 47 tag of a babel or polyglossia language name.
pub fn babel_tag(name: &str) -> Option<&'static str> {
    Some(match name {
        "english" => "en",
        "american" | "USenglish" | "usenglish" => "en-US",
        "british" | "UKenglish" | "ukenglish" => "en-GB",
        "australian" => "en-AU",
        "canadian" => "en-CA",
        "newzealand" => "en-NZ",
        "french" | "francais" | "frenchb" => "fr",
        "acadian" | "canadien" => "fr-CA",
        "german" | "ngerman" => "de",
        "austrian" | "naustrian" => "de-AT",
        "swissgerman" | "nswissgerman" => "de-CH",
        "spanish" => "es",
        "mexican" => "es-MX",
        "italian" => "it",
        "portuguese" | "portuges" => "pt",
        "brazil" | "brazilian" => "pt-BR",
        "polish" => "pl",
        "russian" => "ru",
        "dutch" => "nl",
        "czech" => "cs",
        "turkish" => "tr",
        "croatian" => "hr",
        "indonesian" | "bahasa" | "bahasai" => "id",
        "swedish" => "sv",
        "danish" => "da",
        "norsk" | "norwegian" | "bokmal" => "nb",
        "nynorsk" => "nn",
        "finnish" => "fi",
        "greek" => "el",
        "catalan" => "ca",
        "romanian" => "ro",
        "magyar" | "hungarian" => "hu",
        "slovak" => "sk",
        "slovene" | "slovenian" => "sl",
        "ukrainian" => "uk",
        "bulgarian" => "bg",
        "serbian" => "sr",
        "estonian" => "et",
        "latvian" => "lv",
        "lithuanian" => "lt",
        "icelandic" => "is",
        "irish" => "ga",
        "welsh" => "cy",
        "basque" => "eu",
        "galician" => "gl",
        "vietnamese" => "vi",
        "hebrew" => "he",
        "arabic" => "ar",
        _ => return None,
    })
}
