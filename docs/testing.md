# Testing

The checks to run before a change are in `AGENTS.md` ("Checks"); `cargo xtask ci` runs them all.
This page is about the tests that take longer when asked to.

## Testing the formatted view

The formatted view is built from the source, and each edit in it is a small change of the source
with the caret mapped back (`texcraft_latex::view`, `texcraft_latex::edit`). Two random-edit runs
look for what goes wrong:

- **The model** (`crates/latex/tests/it/formatted_fuzz.rs`): documents of the corpus, or slices of
  them, edited by random sequences of what a user does in the view (clicks, selections, arrows,
  typing with LaTeX's special characters and other scripts, Backspace and Delete at every kind of
  edge, Enter, Tab and Shift+Tab, pastes of lines and lists, the toolbar's buttons, the table
  toolbar's operations, citations, references, footnotes, links, formulas, and a figure inserted
  for an image picked or dropped), made as the window makes them. The corpus holds the templates,
  the examples and documents written for the runs: lists, tables, figures, theorems of several
  styles, front matter, `\newcommand` macros, comments everywhere, lists four deep, `\multicolumn`
  and `\multirow`, verbatim and `lstlisting`, `\input` and `\include`, a bibliography, a beamer
  talk, a talk with overlays, columns and blocks, a report with a title page and the document's own
  macros. After every step it checks that nothing panicked; that the changes are sound (in order, on
  the text, near the selection); that nothing balanced became unbalanced; that no empty item,
  list, style or heading is left where the caret is not; that nothing became shown as source; that
  the caret stands where the view shows it (a key typed next goes there); that a style button
  pressed on a selection keeps that text selected; and that the view shows what the edit meant
  (the typed text where it was typed, the deleted character gone, the words kept by a change of
  structure). A failure is shrunk to the fewest lines and characters of its document and the
  fewest steps, and printed.
- **The window** (`crates/ui-egui/tests/it/formatted_fuzz.rs`): the real view with the build's
  fonts, driven by random keys, typed text, words typed letter by letter, text composed with an
  input method (marked text, then committed), clicks, drags, double and triple clicks, toolbar
  buttons, undo and redo. After every action: no panic, the frames settle, the keyboard stays in
  the text, the caret is in a line of the view inside the window, a typed or composed word shows
  before the caret without its line jumping, one undo takes a word typed letter by letter back
  whole (not its last letter), and undo then redo give the texts before and after the edit back.

The default run is short (a few hundred model cases, a few window cases). For a long run:

```sh
TEXCRAFT_FUZZ_CASES=100000 TEXCRAFT_FUZZ_SEED=7 nice -n 10 cargo test -p texcraft-latex --test it formatted_fuzz -- --nocapture
TEXCRAFT_FUZZ_WINDOW_CASES=200 nice -n 10 cargo test -p texcraft-ui-egui --test it formatted_fuzz
```

`TEXCRAFT_FUZZ_CORPUS=<dir>` adds the `.tex` files under a folder (your own papers, read and never
copied; `TEXCRAFT_FUZZ_FILES` caps how many). Change `TEXCRAFT_FUZZ_SEED` to explore other cases. A
failure prints its reproducer: the document and the steps. Write the fix with a regression test
named after the behaviour, in the document's simplest form and in your own words (a reproducer cut
from someone's paper is not committed); `crates/latex/tests/it/formatted_fuzz/found.rs` holds those
the runs found so far.
