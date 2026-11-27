# Test fixtures

- `frontmatter.tex`: an elsarticle-style front matter (title note, authors with affiliations,
  e-mails, a corresponding author and a footnote, keyed and plain affiliations, abstract,
  keywords) written for TeXCraft's tests. Our own work, under the repository's licence; the
  title, people, places and addresses are invented (`example.org`).
- `formatted-mixed.tex`, `formatted-lists.tex`, `formatted-fragment.tex`: documents written for
  the formatted view's random-edit runs (`tests/it/formatted_fuzz.rs`, and the window's in
  `crates/ui-egui/tests/it/formatted_fuzz.rs`), holding what the view shows: headings with labels,
  styles and declarations, formulas, references and citations, nested, empty and labelled list
  items, a theorem, a proof, a quote, a figure, a table with a merged cell, links, verbatim text,
  and an included file's kind of text (no document environment). Our own work, under the
  repository's licence.
- `formatted-structures.tex`, `formatted-beamer.tex`: documents written for the same runs, of
  what papers and talks hold that the others do not: theorem-like environments of several styles,
  macros of `\newcommand`, comments everywhere, lists four deep, a table with `\multicolumn` and
  `\multirow`, verbatim and `lstlisting`, `\input` and `\include`, a bibliography environment,
  and a beamer talk's frames, columns and blocks. Our own work, under the repository's licence;
  the names are invented.
- `formatted-talk.tex`, `formatted-report.tex`: documents written for the same runs and for the
  view's tests, of what the templates hold that the others do not: a beamer talk with a title
  page, overlays (`\item<2->`, `\pause`, `\only<3>{…}`), frames titled by argument and by
  `\frametitle`, columns and the three kinds of block; and a report with a `titlepage`, the
  generated lists (`\tableofcontents`), user macros (`\newcommand`, `\NewDocumentCommand`,
  `\def`) whose arguments are text or not, and a `thebibliography`. Our own work, under the
  repository's licence; the names, places and titles are invented.
