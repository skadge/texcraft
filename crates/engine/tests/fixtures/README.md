# Test fixtures

- `subfolder.synctex`: the SyncTeX data retex 2.1.0 wrote for a two-file project of our own,
  `paper/main.tex` (`\documentclass{article}`, a paragraph, `\input{sections/one}`) and
  `paper/sections/one.tex` (two paragraphs), built by TeXCraft's retex backend. Our own work,
  under the repository's licence. It names TeX Live files by their path on the machine it was
  built on, and the build folder under `/tmp`. The last paragraph of `one.tex` ends in `main.tex`,
  so its line box is tagged with `main.tex`'s line: the case of an included file's last paragraph
  (retex-synctex 2.3.0 sends a click in it to `one.tex`).
