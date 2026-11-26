//! What cross-references and citations point at.

use texcraft_latex::refs;

#[test]
fn a_label_says_what_it_labels() {
    let text = "\\section{Methods}\\label{sec:m}\n\\begin{figure}\\caption{A  cat}\\label{fig:cat}\\end{figure}\n\\begin{equation}x\\label{eq:x}\\end{equation}";
    let found: Vec<(String, String)> = refs::labels(text).into_iter().map(|l| (l.key, l.what)).collect();
    let want = [("sec:m", "Methods"), ("fig:cat", "A cat"), ("eq:x", "equation")];
    assert_eq!(found, want.map(|(k, w)| (k.to_owned(), w.to_owned())));
}

#[test]
fn an_entry_reads_as_author_and_year() {
    let bib = "@article{one, author = {Lemaignan, S{\\'e}verin}, year = 2017, title = {T}}\n\
               @book{two, author = {Ada Lovelace and Charles Babbage}, year = {1843}}\n\
               @misc{many, author = {A, B and C, D and E, F}, date = {2020-01-02}}\n\
               @misc{org, author = {{World Health Organization}}}";
    let shown: Vec<String> = refs::entries(bib).iter().map(refs::author_year).collect();
    assert_eq!(shown.len(), 4, "{shown:?}");
    assert!(shown[0].starts_with("Lemaignan") && shown[0].ends_with("(2017)"), "{}", shown[0]);
    assert_eq!(shown[1], "Lovelace and Babbage (1843)");
    assert_eq!(shown[2], "A et al. (2020)");
    assert_eq!(shown[3], "World Health Organization");
}

#[test]
fn a_package_is_loaded_from_any_usepackage_list() {
    let main = "\\documentclass{article}\n\\usepackage[capitalise]{hyperref, cleveref}\n";
    assert!(refs::loads(main, "cleveref"));
    assert!(!refs::loads(main, "booktabs"));
}
