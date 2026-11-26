# Test fixtures

- `subfolder.synctex`: the SyncTeX data retex 2.1.0 wrote for a two-file project of our own,
  `paper/main.tex` (`\documentclass{article}`, a paragraph, `\input{sections/one}`) and
  `paper/sections/one.tex` (two paragraphs), built by TeXCraft's retex backend. Our own work,
  under the repository's licence. It names TeX Live files by their path on the machine it was
  built on, and the build folder under `/tmp`. The last paragraph of `one.tex` ends in `main.tex`,
  so its line box is tagged with `main.tex`'s line: the case of an included file's last paragraph
  (retex-synctex 2.3.0 sends a click in it to `one.tex`).
- `pdflatex-only.tex`: a document of our own written for pdfLaTeX (`\pdfcompresslevel`,
  `\pdfmapline`, `\pdfstrcmp`, which LuaLaTeX's format does not define), under the repository's
  licence: the build chooses pdfLaTeX for it (#52).
- `gitlab-release-latest.json` and `github-release-latest.json`: answers written by hand in the
  shape of GitLab's releases API (`GET /projects/:id/releases/permalink/latest`) and GitHub's
  (`GET /repos/:owner/:repo/releases/latest`), for a made-up TeXCraft 0.20.0, with a link to
  another host and an asset of another repository that the readers must leave out. Our own work,
  under the repository's licence; the checksums, ids and project number are invented.
