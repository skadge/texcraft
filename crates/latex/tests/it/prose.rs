//! E11: the prose spell checking reads — commands, their keys, math and verbatim text left out —
//! and the document's language from babel or polyglossia.

use texcraft_latex::prose::{Options, document_language, ranges};

/// The words of the prose of `text`, in order.
fn words(text: &str, opts: Options) -> Vec<String> {
    ranges(text, opts)
        .into_iter()
        .flat_map(|r| text[r].split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

fn prose(text: &str) -> Vec<String> {
    words(text, Options::default())
}

#[test]
fn text_and_the_arguments_of_text_commands_are_prose_their_keys_are_not() {
    let text = "\\section[Short]{Intro}\nSee \\ref{sec:alpha} and \\cite[p.~3]{knuth84}, \\emph{really}.\\label{key}\n\
                \\textcolor{red}{warm} \\setlength{\\parindent}{zero} \\href{https://x.org/path}{site} \\texttt{codeword}\n";
    assert_eq!(prose(text), ["Short", "Intro", "See", "and", "really", "warm", "site"]);
}

#[test]
fn math_verbatim_and_code_environments_are_skipped() {
    let text = "Let $alpha + betta$ be \\(gamma\\) and\n\\[delta\\]\n\\begin{equation}epsilon\\end{equation}\n\
                \\verb|zeta| \\begin{verbatim}eta\\end{verbatim}\n\\begin{minted}{python}theta\\end{minted}\n\
                \\begin{lstlisting}iota\\end{lstlisting}\n\\begin{tikzpicture}\\node{kappa};\\end{tikzpicture} done";
    assert_eq!(prose(text), ["Let", "be", "and", "done"]);
}

#[test]
fn environment_names_and_arguments_are_skipped_their_bodies_are_prose() {
    let text = "\\begin{figure}[htbp]\n\\caption{A cat}\n\\end{figure}\n\\begin{tabular}{lcr}cell\\end{tabular}\n\
                \\begin{itemize}[nosep]\\item[first] one\\end{itemize}\n\\begin{multicols}{two}words\\end{multicols}";
    assert_eq!(prose(text), ["A", "cat", "cell", "first", "one", "words"]);
}

#[test]
fn comments_are_prose_only_when_asked_for() {
    let text = "Seen % unseen remark\nagain";
    assert_eq!(prose(text), ["Seen", "again"]);
    assert_eq!(words(text, Options { comments: true }), ["Seen", "unseen", "remark", "again"]);
}

#[test]
fn a_word_touching_an_accent_command_is_not_checked() {
    let text = "un caf\\'e na\\\"{\\i}ve \\\"{u}ber stra\\ss{}e fine";
    assert_eq!(prose(text), ["un", "fine"]);
}

#[test]
fn only_the_title_block_of_the_preamble_is_prose() {
    let text = "\\documentclass[a4paper]{article}\n\\usepackage[utf8]{inputenc}\n\\hypersetup{pdftitle=Words}\n\
                \\title{The Title}\n\\author{Ada}\n\\newcommand{\\foo}{bar}\n\\begin{document}\nBody\n\\end{document}\n";
    assert_eq!(prose(text), ["The", "Title", "Ada", "Body"]);
}

#[test]
fn half_typed_and_hostile_input_gives_ranges_inside_the_text_in_order() {
    let inputs = [
        "\\section{",
        "$$",
        "\\begin{itemize}\\item",
        "\\textcolor{",
        "}}}{{{",
        "\\\\[",
        "\\begin{multicols}",
        "caf\\'",
        "% only a comment",
        "\\verb",
        "é\\\"{",
        "\\begin{document}\\end{document}",
        "\\setlength{\\x}[",
    ];
    for text in inputs {
        for comments in [false, true] {
            let mut at = 0;
            for r in ranges(text, Options { comments }) {
                assert!(at <= r.start && r.start < r.end && r.end <= text.len(), "{text:?}: {r:?} after {at}");
                assert!(text.get(r.clone()).is_some(), "{text:?}: {r:?} is not on character boundaries");
                at = r.end;
            }
        }
    }
}

#[test]
fn the_language_comes_from_babel_or_polyglossia() {
    assert_eq!(document_language("\\documentclass{article}\n\\usepackage[french]{babel}\n"), Some("fr"));
    // babel's main language is the last one named, or its `main=`.
    assert_eq!(document_language("\\usepackage[ngerman,british]{babel}"), Some("en-GB"));
    assert_eq!(document_language("\\usepackage[main=spanish,english]{babel}"), Some("es"));
    // Languages given to the class reach babel.
    assert_eq!(document_language("\\documentclass[12pt,polish]{article}\n\\usepackage{babel}"), Some("pl"));
    assert_eq!(document_language("\\usepackage{polyglossia}\n\\setdefaultlanguage[variant=british]{italian}"), Some("it"));
    // Issue #36: polyglossia's English variants spell as their country does.
    assert_eq!(document_language("\\usepackage{polyglossia}\n\\setdefaultlanguage[variant=british]{english}"), Some("en-GB"));
    assert_eq!(document_language("\\setmainlanguage[variant=uk, ordinalmonthday=false]{english}"), Some("en-GB"));
    assert_eq!(document_language("\\setmainlanguage[variant=american]{english}"), Some("en-US"));
    assert_eq!(document_language("\\setmainlanguage{english}"), Some("en"));
    assert_eq!(document_language("\\documentclass{article}\n\\begin{document}\\usepackage[french]{babel}"), None);
    assert_eq!(document_language("\\usepackage[french]{csquotes}"), None);
}
