//! Foldable regions: environments, sections up to the next heading of their level, the preamble.

use texcraft_latex::fold::{FoldKind, FoldRegion, regions};

const DOC: &str = "\\documentclass{article}\n\
\\usepackage{amsmath}\n\
\n\
\\begin{document}\n\
\\section{Intro}\n\
Text.\n\
\\begin{itemize}\n\
  \\item One\n\
\\end{itemize}\n\
\\subsection{Detail}\n\
More.\n\
\n\
\\section{Next}\n\
Last.\n\
\\begin{equation}\n\
  x = 1\n\
\\end{equation}\n\
\\end{document}\n";

fn at(rs: &[FoldRegion], line: usize) -> Option<&FoldRegion> {
    rs.iter().find(|r| r.start == line)
}

#[test]
fn a_section_runs_to_the_next_heading_of_its_level_without_trailing_blank_lines() {
    let rs = regions(DOC);
    let intro = at(&rs, 4).expect("\\section{Intro} is foldable");
    assert!(matches!(&intro.kind, FoldKind::Section { title, .. } if title == "Intro"));
    assert_eq!(intro.end, 10, "Intro swallows its subsection but not the blank line before Next");
    let detail = at(&rs, 9).expect("the subsection is foldable");
    assert!(detail.end < intro.end + 1 && detail.end > detail.start);
    let next = at(&rs, 12).expect("the last section is foldable");
    assert_eq!(next.end, 16, "the last section stops before \\end{{document}}");
}

#[test]
fn environments_spanning_lines_fold_and_the_document_environment_does_not() {
    let rs = regions(DOC);
    assert!(matches!(&at(&rs, 6).map(|r| (&r.kind, r.end)), Some((FoldKind::Environment(n), 8)) if n == "itemize"));
    assert!(matches!(&at(&rs, 14).map(|r| (&r.kind, r.end)), Some((FoldKind::Environment(n), 16)) if n == "equation"));
    assert!(at(&rs, 3).is_none(), "\\begin{{document}} would hide the whole text");
    // An environment on one line, or one never closed, has nothing to fold.
    let rs = regions("\\begin{center}x\\end{center}\n\\begin{itemize}\n\\item a\n");
    assert!(rs.is_empty(), "{rs:?}");
}

#[test]
fn the_preamble_folds_from_documentclass_to_before_begin_document() {
    let rs = regions(DOC);
    let p = at(&rs, 0).expect("the preamble is foldable");
    assert_eq!((p.kind.clone(), p.end), (FoldKind::Preamble, 1));
    // Without \begin{document} (an included file) there is no preamble.
    assert!(regions("\\documentclass{article}\n\\usepackage{x}\n\\usepackage{y}\n").iter().all(|r| r.kind != FoldKind::Preamble));
}

#[test]
fn half_typed_and_hostile_input_gives_regions_inside_the_text() {
    for text in ["", "\\begin{", "\\section{", "\\end{document}\n\\section{A}\nx\n", "}\\end{x}\n\\begin{y}\n\\end{y}", "\\section{A}\n\n\n"] {
        let lines = text.split('\n').count();
        for r in regions(text) {
            assert!(r.start < r.end && r.end < lines, "{text:?}: {r:?}");
        }
    }
}
