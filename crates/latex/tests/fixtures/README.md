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
