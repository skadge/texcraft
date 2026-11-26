//! The numbers the formatted view shows, from a real build's `.aux` (retex in process).

use serde_json::{Value, json};
use texcraft_engine::Session;

const MAIN: &str = r"\documentclass{article}
\usepackage{amsmath,amsthm}
\usepackage{hyperref}
\usepackage{cleveref}
\newtheorem{theorem}{Theorem}[section]
\newtheorem{lemma}[theorem]{Lemma}
\begin{document}
\section{Intro}\label{sec:intro}
\subsection{Background}\label{sec:back}
\input{sections/one}
\begin{equation}\label{eq:one} a = b \end{equation}
\begin{align}
  x &= 1 \label{eq:two}\\
  y &= 2 \nonumber\\
  z &= 3 \label{eq:three}
\end{align}
\begin{equation} c = d \end{equation}
\begin{figure}\centering \caption{A picture}\label{fig:pic}\end{figure}
\begin{theorem}[Big]\label{thm:big} True. \end{theorem}
\begin{lemma} Also true. \end{lemma}
See \ref{eq:one}, \eqref{eq:two}, \cref{fig:pic}, \Cref{thm:big}, \autoref{sec:back}, \ref{eq:none} and \cite{knuth,lamport}.
\bibliographystyle{plain}
\bibliography{refs}
\end{document}
";

const BIB: &str = "@book{knuth, author={Donald E. Knuth}, title={The TeXbook}, year={1984}, publisher={AW}}\n@book{lamport, author={Leslie Lamport}, title={LaTeX}, year={1994}, publisher={AW}}\n";

fn session(name: &str) -> Result<(Session, std::path::PathBuf), String> {
    let dir = std::env::temp_dir().join(format!("texcraft-numbers-{name}-{}", std::process::id()));
    let mut s = Session::with_backend(Box::new(texcraft_compile::retex_backend::Retex::new(Some(dir.clone()))));
    for (path, text) in [("main.tex", MAIN), ("sections/one.tex", "Included. \\begin{equation}\\label{eq:in} e \\end{equation}\n"), ("refs.bib", BIB)]
    {
        if path != "main.tex" {
            s.execute("file.new", &json!({"path": path}))?;
        }
        s.execute("file.write", &json!({"path": path, "text": text}))?;
    }
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    Ok((s, dir))
}

/// What `\command{key}` shows, or why it shows no number; null when the file has no such reference.
fn shown<'v>(report: &'v Value, command: &str, key: &str) -> &'v Value {
    let found = report["references"].as_array().and_then(|refs| refs.iter().find(|r| r["command"] == command && r["key"] == key));
    match found {
        Some(r) if r["shown"].is_null() => &r["missing"],
        Some(r) => &r["shown"],
        None => &Value::Null,
    }
}

#[test]
fn references_show_the_numbers_of_the_last_build_and_say_why_before_it() {
    let (mut s, dir) = session("refs").unwrap();
    let before = s.execute("language.numbers", &json!({"path": "main.tex"})).unwrap();
    assert_eq!(before["built"], false);
    assert_eq!(shown(&before, "ref", "eq:one"), "not built yet");

    let built = s.execute("build.compile", &json!({"wait": true})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(s.aux().is_some(), "the build brought no .aux: {built}");
    let after = s.execute("language.numbers", &json!({"path": "main.tex"})).unwrap();
    // The included file's equation is the first: the main file's go on from it.
    assert_eq!(shown(&after, "ref", "eq:one"), "2");
    assert_eq!(shown(&after, "eqref", "eq:two"), "(3)");
    assert_eq!(shown(&after, "cref", "fig:pic"), "fig.\u{a0}1");
    assert_eq!(shown(&after, "Cref", "thm:big"), "Theorem\u{a0}1.1");
    assert_eq!(shown(&after, "autoref", "sec:back"), "subsection\u{a0}1.1");
    assert!(shown(&after, "ref", "eq:none").as_str().is_some_and(|m| m.contains("eq:none")), "{after}");
    assert_eq!(shown(&after, "cite", "knuth,lamport"), "[1, 2]");

    // The numbers the view gives the parts agree with the build's, labelled or not.
    let numbered = after["numbered"].as_array().unwrap();
    let numbers = |kind: &str| -> Vec<Value> { numbered.iter().filter(|n| n["kind"] == kind).map(|n| n["number"].clone()).collect() };
    assert_eq!(numbers("heading"), [json!("1"), json!("1.1")]);
    assert_eq!(
        numbers("formula"),
        [json!(["(2)"]), json!(["(3)", null, "(4)"]), json!(["(5)"])],
        "an unlabelled equation goes on from the labelled ones"
    );
    assert_eq!(numbers("theorem"), [json!("1.1"), json!("1.2")], "the lemma shares the theorem's counter");
    assert_eq!(numbers("figure"), [json!("1")]);
    let included = s.execute("language.numbers", &json!({"path": "sections/one.tex"})).unwrap();
    assert_eq!(included["numbered"][0]["number"], json!(["(1)"]), "{included}");
}
