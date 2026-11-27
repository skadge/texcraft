# Changelog

What changed in each release of TeXCraft, for the people who use it. The day-by-day record is the
progress log in [docs/progress.md](docs/progress.md), what comes next in
[docs/ROADMAP.md](docs/ROADMAP.md); how a release is made is in
[docs/releasing.md](docs/releasing.md).

## 0.21.1 (2026-11-27)

### Fixes

- Web: after the first build, a build could stop with "the typesetting engine stopped:
  JsValue(WebAssembly.Exception)" (seen with slides) until the page was reloaded (#56). Such a
  build is now built again from the start, as a reload did; the engine's own message is kept, to
  find the cause.

## 0.21.0 (2026-11-27)

The formatted view reads slides, theses, reports, letters, CVs and homework as documents, and
many more of its edits are fixed.

### Documents shown as text

- Slides: each frame is a card with its title; lists, columns and blocks are text to edit;
  overlays (`\pause`, `<2->`) are small marks.
- A title page, a bibliography (numbered entries with their keys), a letter's recipient, and
  the table of contents, lists of figures, page breaks and `\today` as named marks.
- A call of the document's own simple commands (`\cventry`, `\exercise`) shows its text to edit,
  styled as the command sets it.
- Lists with options such as `[label=(\alph*)]` are lists, numbered as the label says.

### Editing

- Find and Replace open over the formatted view and search the text it shows, also across bold
  or italic; a replacement keeps the style.
- "LaTeX" and the other logos are deleted whole with one Backspace.
- A heading made from a list item leaves the list first.
- Pressing Bold and going elsewhere no longer leaves an empty `\textbf{}`; typing never lands in
  a `%` comment; a display formula in a heading or caption is refused, with the reason.
- Text being composed with an input method shows at the caret.
- On a phone, Bold, Italic and Code stay on the toolbar.
- Delete Row leaves the caret in a cell, and many more carets stay where the text is.

## 0.20.1 (2026-11-27)

0.20.0's changes, below: 0.20.0's release stopped at a test, so it was not published.

## 0.20.0 (2026-11-27, not published)

The browser downloads the LaTeX packages it lacks, and many fixes to the formatted view.

### Packages in the browser

- A package the browser's TeX tree lacks is downloaded during the build, once, from a copy of
  TeX Live of the tree's year (texlive.skadge.org by default), and kept across visits and
  TeXCraft updates. The status line names the package being downloaded, the build log lists what
  was downloaded, and says why when the server refuses or can't be reached.
- Settings › Building › Package server: TeXCraft's server, another server, or Off.

### Formatted view

Found by a new test that edits real documents at random and checks each step:

- The caret stays on its line after a trailing space, `~`, `^` or `\`, and in tables with a
  caption above and a label below.
- Text typed after a list, a heading's label or a figure's caption goes into it, not after its
  `\end{…}` or `\label` (which turned a figure into source).
- Commands no longer run into the next word (`\itemword`, `\dotsword`), and deleting a styled
  word, a heading's text or whole items leaves no empty `\textbf{}`, `\section{}` or `\item`.
- A paste goes where it lands: a list is no longer pasted into a heading.
- A new table or equation goes where the caret is (not into the preamble, after the document
  or after the list).
- Empty paragraphs, theorems, quotes, titles and captions keep a line to type on.
- Merged cells, a typed no-break space, Shift+Tab on a described item and Backspace on `~`
  behave as they should.

## 0.19.1 (2026-11-27)

0.19.0's Linux packages, a search in Settings, updates from within TeXCraft, and fixes. 0.19.0's
release stopped at its check, so its Linux packages were not published; they are in this one.

### Updates

- TeXCraft looks for a new release once a day (Settings › Appearance › Check for updates; it asks
  gitlab.com and sends nothing about you) and Help › Check for Updates…. The notice in the status
  bar opens What's New. An AppImage updates itself (the download checked against the release's
  checksums, the old file kept until the new one starts); Flatpak updates come through Flatpak;
  on Windows and macOS the new download is offered; the browser offers to reload.

### Settings

- A search across every section, by name and description, in your language or English (#54);
  each section opens at its top (#55).

### Fixes

- After a Sync from the browser, the badge no longer says a change is waiting (#53).
- A PDF page zoomed far is drawn at the largest size the graphics card takes, instead of failing.
- The tip of the day's button is named as the tip's, apart from the button of the same label.

## 0.19.0 (2026-11-26, Linux packages not published)

Merges that see words, a switch for AI, pdfLaTeX when a document needs it, and fixes from the
third round of testing (#43–#52).

### Working with others

- Sync, Pull and the GitLab and GitHub updates merge lines both sides changed word by word: two
  people editing different sentences, or different words, of one paragraph no longer conflict.
- Tokens typed when importing are kept (the system's keychain; the tab in the browser) and used for
  the next import, push, pull and Sync; Settings › Accounts lists them, with Replace… and Forget
  (#51).
- Importing shows a card on the start page with what is happening, and Cancel (#49); the
  repository list says it is being fetched, then how many there are (#50).
- Automatic versions in every repository, your own too, without filling its history: while not
  yet sent anywhere, the next automatic version replaces the last one made in the same session.

### Building

- pdfLaTeX is chosen when the document asks for it (a `% !TEX program = pdflatex` line, or a
  pdfTeX-only command such as `\pdfmapfile`), and the build log says why (#52).
- retex 2.5.2: SVG images keep a sans-serif or monospaced font when theirs is missing (#5), and
  fixes to the DOCX, ODT and HTML exports.

### The formatted view

- Deleting whole list items removes them (#48); the list buttons start a list on an empty line
  (#47); the caret sits right in empty items (#46); three dots typed become an ellipsis (#45).
- Elsevier's front matter (title, authors and affiliations, notes, abstract, keywords) as a title
  block (#44); accented letters (`\'e`, `\"o`, `\c{c}`…) shown as letters; every author shown.

### AI

- Settings › AI › Use AI features: off hides every AI feature and nothing is sent (#43).

## 0.18.0 (2026-11-26)

Import a Project: one dialog for every place a project can come from.

- File › Import a Project… (Ctrl+Shift+O) and Import… on the start page: cards for GitLab, GitHub,
  Overleaf, your own GitLab server, any git address over HTTPS, a folder and a zip file.
- On the desktop a project from GitLab, GitHub, Overleaf or a git address is cloned into a folder
  of yours, with its whole history and its remote set, so Sync works at once. It goes into your
  Documents folder, or the one you chose last.
- Overleaf: paste the project's git clone address (or the whole `git clone` line) and an Overleaf
  git token; Remember on this computer keeps the token in the system's keychain (Settings ›
  Accounts › Git servers to forget it).
- A git address is checked as you type: public or needing a sign-in, its branches, an SSH address
  offered over HTTPS.
- In the browser, GitLab and GitHub come in through their web APIs; Overleaf and other git servers
  do not answer web pages, and their cards say so and offer the desktop app or a zip.
- The cards show GitLab's, GitHub's and Overleaf's marks, their owners' trademarks, to name the
  service each connects to.
- The phone's Back button reads right in every language.

## 0.17.0 (2026-11-26)

Saving by itself and one-click Sync, and fixes from the second round of testing (#27–#42).

### Saving and Sync

- Changes are saved by themselves shortly after you stop typing (and when the window loses the
  focus or the project closes); the top bar says "All changes saved". Settings › Editor › Save
  automatically turns it off.
- Versions are made at moments that matter, not at every save: after a few quiet minutes, when the
  project closes, before a pull, a restore or Accept All, and on File › Save; each says what
  changed. History › Name This Version… gives one a name.
- ⇅ Sync beside the project's name, when it has a remote: one click makes a version, brings in the
  remote's changes, then sends yours; the counts show what waits on each side (#34).
- Overleaf projects cloned with git are recognised and synced with an Overleaf git token (#35).
- The History panel's Push and Pull are real buttons, with less text around them.

### Fixes from testing

- A folder with one very large file opens: that file is listed and left alone, and sizes read in
  MB (#28).
- British English spelling from the system's region or polyglossia's variant (#36); the spelling
  language in the status bar, to switch (#37).
- In the formatted view a double click selects a word and a triple click the paragraph;
  Ctrl+double-click shows the source (#38).
- Ctrl and the wheel change the text's size (#33).
- "Apply this fix everywhere in this file (n places)" fixes only that kind of hint (#42).
- A smaller tip of the day and a one-line newcomer hint (#27, #29); the Compile button grey with a
  spinner while it compiles by itself (#32).
- Clicking the outline also scrolls the PDF (#30); the outline folds (#31).
- The PDF bar: Back to Fit (#41), the own-window button at the end (#40); in the PDF's window,
  pages side by side and page thumbnails (#39).

## 0.16.0 (2026-11-26)

Fixes from testing (#16–#26), shorter menus with a list of all commands, a toolbar that fits,
and one outline for the whole document.

### In the browser and on a phone

- Typing works in the formatted view after clicking in it (#24).
- On a phone, a finger's drag scrolls the text, the formatted view, the PDF and the file list; a
  long press selects (#26).
- A first build fetches what its packages need before it starts, and says so ("Downloading LaTeX
  packages: 12 of 34 files…"), instead of stopping and starting again for each file (#16).
- The PDF can be downloaded (File › Download PDF, and a button beside Export) (#22).
- Unsaved changes are kept in this browser, and the question says so (#23).
- Print PDFs work there: the colour profiles are in the browser's TeX files (#20).

### Menus and commands

- Shorter menus, each command in one place (#25), and Edit › All Commands… (Ctrl+Shift+P): every
  command, searched as you type, with its shortcut.
- The source toolbar fits beside the PDF: formulas and references each in one drop-down, and the
  citation and cross-reference pickers in the source too.
- The Outline shows the whole document, through its included files, numbered as in the PDF, the
  shown file's part marked.

### Building and the PDF

- Hide Like This on a warning hides that kind of message, in this project or in all of them (#21).
- Ctrl and the wheel (or a pinch) zoom the PDF around the pointer (#19); the middle button drags
  it (#18).
- The spelling popup is small: a few spellings and Add to Dictionary (#17).

### Behind the scenes

- The interface tests no longer fail now and then (tests that started a build by themselves).

## 0.15.0 (2026-11-26)

The formatted view reads like the PDF and edits more of the document; first steps for new users;
build errors where they start, and versions that say what changed.

### The formatted view

- Cross-references and citations show what the PDF shows ("(2)", "Figure 1", "[Knuth, 1984]"),
  from the last build; before one, the key with a "?" and why. Headings, captions, theorems and
  numbered formulas carry their numbers; labels are a small tag, not text.
- An included file's text is shown in place under its name, with Edit Here.
- Parts the view shows as LaTeX are edited in place, as source; formulas open an editor with
  their typeset preview (desktop).
- Tables: a header row, merging and splitting cells, a caption and a label. Figures: resized by a
  handle, captioned in the view, placed and labelled from their menu.
- The toolbar also shows `\emph` and declarations such as `\bfseries`.

### First steps

- Try an Example: a short paper of our own that shows what TeXCraft does.
- Template cards say what each template is for, who it is from, its licence in words, and whether
  it works offline. An empty start page says what to do first.
- A tip of the day on the start page, each with a button to try it (can be turned off).
- A dismissible strip pointing to the formatted view, the build log and the user guide.

### Building and history

- An error in an included file (an environment left open, a brace never closed) is reported where
  it starts, with where TeX noticed it linked too; the build log's button says its count.
- Each saved version says what changed: the files and, for LaTeX files, the sections ("Edited
  Introduction and Results (main.tex); added figures/plot.png").

### Keyboard and screen readers

- Tab follows the order things are shown in the top bar, the panels' headers, the toolbar and the
  rail; Settings and Keyboard Shortcuts on the rail are buttons, not tabs.
- The question about unsaved changes says what is about to replace the project.

## 0.14.0 (2026-11-26)

TeXCraft on a phone, projects kept for good in the browser, and what an in-depth review of the
interface found.

### On a phone

- Every dialog fits the screen: it takes the whole window, with Close at the top and its buttons in
  a bar at the bottom that stays visible. Settings is a list of its sections, the template gallery
  one column of cards.
- The start page fits, with New Project first; the status bar keeps to the build's state.

### In the browser

- TeXCraft asks the browser to keep your projects for good, and the start page says whether it
  does and how much room is used and left.
- A save the browser refuses (its storage is full) leaves the project unsaved and says what to do;
  File › Download Project as Zip keeps a copy on your computer.

### From the interface review

- Export stays visible beside the PDF's page and zoom buttons in longer languages; the layout
  switch and the toolbars use the commands' own names; "1 errors" and other counts read right.
- Grammar no longer flags every indented line.
- The export formats are named by their files (DOCX, PPTX).
- The full review, and what it planned, is in docs/ux-review.md and the roadmap.

### Languages

- The six translations brought up to date, and the English made easier to translate (whole
  sentences instead of pieces, words told apart where they have two meanings).

## 0.13.0 (2026-11-26)

TeXCraft in six more languages, grammar checking, a user guide, and the formatted view as a real
way to write.

### Writing in the formatted view

- Its own toolbar: paragraph style (body text, headings following the document's class), bold,
  italic, code, bulleted and numbered lists with indent and outdent, link, footnote, citation
  and cross-reference (pickers of the project's bibliography and labels), formulas, table and
  figure; each with a shortcut, and the toolbar shows what is at the caret.
- Typing across structure: Enter on an empty list item ends the list, Tab and Shift+Tab nest
  items, Backspace at the start of a heading or an item makes it text again, pasted lists become
  lists.
- Spellings in the view's own menu; a citation shows its author and year, Ctrl+click on a
  reference goes to its label.

### Grammar (desktop)

- English grammar checked as you write, offline, by Harper: a double underline (spelling stays
  wavy), the explanation and replacements in the menu, Ignore and Turn Off This Rule, groups of
  rules in Settings › Editor. F7 goes to the next spelling or grammar problem.
- Check Grammar with AI, for any language, with the connected model.

### Languages

- The interface in German, Spanish, French, Italian, Dutch and Portuguese (Portugal), chosen in
  Settings › Appearance or following the system. These first translations were drafted with an
  AI model and have not been reviewed by native speakers yet: corrections are welcome.
- Times ("3 days ago"), dates, sizes and numbers written the way the language does.

### Help

- A user guide, online at texcraft-893d51.gitlab.io/guide/ and from Help › User Guide (F1).

Known limits:

- Grammar checking is not in the browser build yet (it would add about 10 MB to the page).
- The guide needs a connection; an offline copy comes with a later release.

## 0.12.2 (2026-11-26)

0.12.1 with one fix for the web editor; 0.12.1 was not published (its release was stopped for
this one).

- In the browser, a new project from a built-in template (article, thesis, slides, letter, CV,
  homework) failed with "This template has not been fetched yet".

## 0.12.1 (2026-11-25, not published)

0.12.0 with two fixes, and AI assistance that no longer asks before every request. 0.12.0's
Linux packages were not published (its release check failed on the first fix below); its
Windows and macOS builds and the web editor have the git bug.

### Fixes

- Pull, Push and Save to GitHub/GitLab failed on a computer where git has no name and email set
  ("Could not update reflog"): TeXCraft is now the fallback name, as for commits.
- Copy and paste work with other programs in every text field and in the editor: Ctrl+V into
  Settings, dialogs and the find bar; on Linux, middle-click paste and the selection offered to
  other programs. Edit › Cut, Copy, Paste and Select All act on the field that had the keyboard.

### AI assistance (desktop)

- No dialog before a request: Settings › AI lists what may be sent, with a switch for each kind
  of request, and connecting agrees to the ones switched on; another model or address asks again
  (#15).
- The instructions sent for each kind are in Settings › AI › Prompts, to change or reset.
- The token is shown as you type it (Hide the token to mask it), and every AI feature has the same
  icon.

## 0.12.0 (2026-11-25, Linux packages not published)

Save a project to GitHub or GitLab, push and pull, folders and zip files, and fixes from testing.

### Git and forges

- File › Save Project to GitHub… / to GitLab…: creates the repository (yours or a group's,
  private by default), sets it as the project's `origin` and pushes the whole history. In the
  browser the files go in as one first commit.
- Push and Pull for project folders in the History panel, with real git over HTTPS: what is to
  push and to pull, a fast-forward or a merge of text files, a refusal in words when the remote
  has commits you don't.
- The Open dialog suggests your projects as you type and lists the branches, the default chosen.
- Self-managed GitLab servers and GitHub Enterprise Server can be added for signing in (Settings ›
  Accounts), and the desktop trusts the system's certificates.

### Projects and files

- Dropping a folder imports it with its structure; dropping a zip asks whether to unzip it
  (unsafe names and zip bombs refused). On the start screen, a folder opens and a zip becomes a
  new project. File › Import a Zip File….

### AI assistance (desktop)

- Make with AI inserts at the caret, Undo takes it out (#13).
- Agreeing to send text is asked once per kind of request and kept; Settings › AI lists what was
  agreed, with Withdraw (#15).
- Ask AI to Change This…: an instruction of your own on a selection, such as "flesh out these
  bullet points" (#14).

### Fixes

- The editor gets the keyboard back after Go to Line (#3); the margin's Hide button works while a
  comment is written (#6).
- Export is a full button, and says "Exporting…" while a file is made (#8, #10); the Compile
  button says Auto-Compile when it compiles automatically (#9); `:w` in Vim saves and compiles
  (#11).
- A failed save of the preferences is said on the status line, and an idle window saves them.
- The browser has the bibliography styles of the common publishers (#4), and SVG figures show
  their text in the editor (#5).
- A narrow window or a phone gets a layout of its own: the files in a drawer, Source and PDF as
  tabs, one Menu button; text on phones should no longer be garbled (#12).

Known limits:

- Nothing of the push, pull and Save to GitHub/GitLab has been used against the real services
  yet; the narrow layout has not been tried on a phone.
- SVG figures still use a serif font in the PDF in the browser (a retex change).

## 0.11.0 (2026-11-25)

Sign in to GitLab and GitHub, and more AI assistance.

### Git forges

- Sign in with GitLab (on the desktop and in the browser) or GitHub (on the desktop) instead of
  pasting a personal access token. TeXCraft asks only to read at first, and for the right to
  commit at the first commit. On the desktop the sign-in is kept in the system's keychain; in the
  browser, for as long as the tab is open. Settings › Accounts shows who is signed in, and signs
  out. A personal access token still works, and is the way for other GitLab or GitHub servers.

### AI assistance (desktop)

- Insert › Make with AI…: a table, an equation, a TikZ diagram or a figure from a description,
  typeset and shown before it is offered as a suggestion.
- Insert › Add a Reference with AI…: a BibTeX entry drafted from a title or DOI, marked as a draft
  to check (models invent references).
- Ask AI: questions about the open file or the whole project, with the lines it mentions as links.
- Review This Section with AI: remarks left as comments on the passages they quote.

Known limits:

- GitHub sign-in is not available in the browser (GitHub's sign-in does not answer web pages); a
  token works there.
- No AI in the browser yet.

## 0.10.0 (2026-11-25)

AI assistance, spell checking, a symbol palette, and a keyboard and screen-reader pass.

### AI assistance (desktop)

- Off until you connect a model in Settings › AI: any OpenAI-compatible service, or one on your
  own computer (llama.cpp, Ollama, vLLM). The token is kept in the system's keychain.
- Before anything is sent, TeXCraft shows exactly what would go, and where, and asks once per
  kind of request. A project can turn AI off for itself.
- Explain on a build message: a plain explanation, and a proposed fix as a suggestion to accept
  or reject.
- Rewrite with AI (clearer, shorter, more formal, grammar, translated) on a selection, as a
  suggestion; a rewrite that would change labels, references, citations or formulas is refused.
- Optional next words as you type, in grey, taken with Tab.

### Writing

- Spell checking that skips commands, keys, formulas and code, in the document's language (from
  babel or polyglossia): English everywhere, other languages with an installed Hunspell
  dictionary. Spellings in the text's menu, Add to Dictionary, F7 for the next one.
- A symbol palette (Insert › Symbols…, or Ω on the toolbar), searchable, inserting inside a
  formula or as one.

### Building

- Search in the raw log, and the build's output files (PDF, print PDF, SyncTeX, log) to save.

### Accessibility

- Every panel and dialog has been walked through by keyboard: dialogs take the keyboard and give
  it back, menus opened from the keyboard start on their first entry, the Tab order follows the
  reading order, and F6 moves between regions from anywhere.

Known limits:

- No AI in the browser yet.
- A screen-reader pass by a person is still to come.

## 0.9.0 (2026-11-25)

GitHub projects, tables and figures in the formatted view, and the macOS menu bar.

### Git forges

- Projects on GitHub open, commit, update and merge as GitLab projects do, with a personal access
  token (File › Open from GitLab or GitHub…). A branch you cannot push to gets a new branch and a
  pull request.

### The formatted view

- Tables (`tabular`) show as grids: edit a cell in place, Tab to the next, and add or remove rows
  and columns, set a column's alignment or the rules (none, all, booktabs) from the table's
  toolbar. A table the grid cannot show faithfully stays as source and says why.
- Figures have an Edit Figure… dialog: image, width, placement, caption and label. On the desktop,
  an image dropped on the view becomes a figure.

### macOS

- The menus are in the macOS menu bar, with the app menu (About, Settings, Quit). Built and
  compiled for macOS, not yet tried on a Mac: please report how it behaves.

Known limits:

- Merging and splitting table cells, `tabularx` and `longtable` are not in the grid yet.
- Signing in to GitLab or GitHub (instead of a token) comes later.

## 0.8.0 (2026-11-25)

Projects from GitLab, editing in the formatted view, and more of the window.

### GitLab

- File › Open from GitLab… opens a project of gitlab.com or another GitLab instance with a
  personal access token (kept for the session only, never written anywhere). Edit and build it,
  then File › Commit to GitLab… commits the changes to its branch.
- TeXCraft keeps up with the branch: File › Update from GitLab, and a quiet check every two
  minutes, take in what others committed, merging line by line; for files changed on both sides
  in the same place, you choose to keep yours or take theirs. A protected branch, or one you
  cannot push to, gets a new branch and a merge request instead. Comments are committed with the
  project.

### Writing

- The formatted view can be edited: type, delete, split paragraphs, add list items, and make
  text bold, italic or a heading from the toolbar, each change a small edit of the source, undone
  as in the source view. Formulas and other parts shown as source still open their source.

### The window

- F10, or Alt alone, reaches the menus from the keyboard.
- Focus mode (Ctrl+Shift+Enter) hides the menus, the rail and the side panel.
- The PDF can go to its own window (desktop).

Known limits:

- GitLab only, with a personal access token; signing in with GitLab, GitHub, and folders kept in
  step with a repository come later.
- In the formatted view, typing in another script's input method shows only the committed text.

## 0.7.0 (2026-11-25)

Suggested changes, history in the browser, and folding.

### Reviewing

- Suggest mode (Edit › Suggest Changes, Ctrl+Shift+E, or the Suggest switch above the editor):
  what you type is a suggestion instead of a change, shown struck through and underlined in your
  colour, with a card in the margin to accept or reject it, one by one or all at once, and to reply
  to it. Builds show the text as it is, or with the suggestions applied when you ask. Agents
  suggest through `review.suggest`.

### Projects

- Browser projects keep their history too: each save that changes something is a version, listed
  in the History panel, compared and restored as on the desktop.

### Editing

- Folding: sections, environments and the preamble fold from the gutter or with Ctrl+Shift+[ and
  Ctrl+Shift+] (`zc`, `zo`, `za`, `zM`, `zR` in Vim), and open again when the caret lands inside.
- Several carets: Alt+click adds one, Ctrl+D adds the next occurrence of the selection; typing,
  deleting and moving act at all of them, as one step to undo.

Known limits:

- In Suggest mode, completion and hover are off for files showing suggestions.
- Commands other than typing, deleting and moving act at the first caret only.

## 0.6.0 (2026-11-25)

Formulas typeset in the formatted view, a better PDF preview, and PDFs for screen readers and for
print. 0.6.0 also brings everything listed under 0.5.0, whose Linux packages were never
published (a test failed in CI).

### The PDF

- The preview draws pages in the background, follows links (to a page, or to the web after a
  safety check), lets you select and copy text, has zoom presets (Ctrl+0 for 100 %), and shows the
  PDF a page at a time in a presentation mode (F5).
- Accessible PDFs (PDF/UA) and print PDFs (PDF/X-4 or PDF/X-1a, with bleed and crop marks), from
  Settings › Building or the Compile button's menu. Their checks (missing alternative text, image
  resolution, ink coverage…) have their own filters in the build log, each linked to its source
  line, and print checks to their place on the page; a print build starts with "Ready for print"
  or the number of problems.

### Writing

- The formatted view shows formulas typeset by retex with the document's own preamble, on the
  desktop; editing one formula typesets only that one again.

Known limits:

- Formulas are typeset on the desktop only; the browser shows their source.
- An accessible build does not use the build worker's preamble snapshot yet, so its rebuilds are
  slower.

## 0.5.0 (2026-11-25, not published)

Every project now has a history, kept in git, and TeXCraft exports to Word, PowerPoint and
more. First experimental builds for Windows and macOS.

### Projects and history

- Every project is a git repository, as on online LaTeX editors: a folder that is not in one
  becomes one on its first save, and each save is a version. A folder already in a repository is
  used as it is, and TeXCraft commits there only when asked.
- A History panel lists the versions, shows what changed in each file, and restores a file or
  the whole project.
- Go to File (Ctrl+P), new folders, and moving files by dragging them onto a folder.
- Search and replace across the whole project (Ctrl+Shift+F), results grouped by file.
- Six templates of TeXCraft's own (article, thesis, slides, letter, CV, homework), built in: no
  download, offline and in the browser.
- A file deleted, renamed or moved in TeXCraft no longer comes back when the project is reopened
  (#2).

### Writing and building

- Export from the PDF pane: Word, OpenDocument, PowerPoint (first for beamer slides), HTML,
  Markdown and SVG, on the desktop and in the browser.
- A menu on right-click in the text (also Shift+F10): cut, copy, paste, add a comment, quick fix,
  show in PDF.
- In Vim mode, a Visual selection can be commented.
- The page being built shows beside the PDF too.

### In the browser

- A project's second visit builds without waiting for TeX files one by one.
- The engine's files are named after their contents, so an update never leaves an old engine in
  the browser.

### Windows and macOS (experimental)

- A Windows zip and a macOS app (Apple Silicon) are attached to the release on the GitHub mirror.
  They are not signed and have had little testing: please report what goes wrong.

Known limits:

- History is kept on the desktop only; browser projects keep their latest version.
- In the browser, a document that needs a package outside the site's TeX tree does not build.

## 0.4.0 (2026-11-24)

The browser version is now self-contained and keeps your work: projects, templates and the TeX
files all come from TeXCraft's own site, and stay in the browser between visits.

### In the browser

- Projects are kept in the browser: save a project under a name, and the start page lists the
  projects of this browser to open or delete. Comments are kept with them, and the page asks before
  you leave it with unsaved changes.
- Templates work in the browser: they are served with the browser version, and a template makes a
  named project.
- The TeX files come from TeXCraft's own site: the common classes and packages, and those of every
  template. What a visit downloads, and the engine's format, are kept for the next visit: a later
  first build takes a few seconds instead of 10 to 20.
- The dark theme applies to the whole window (only the borders turned dark before).
- A file dropped on the window from Firefox is added (Firefox could leave the drop empty); a drop
  that brings no file says so.

### Everywhere

- Click the project's name in the top bar to rename it (or to name and save an untitled project).
- Drag an image from the project's files onto the source to insert `\includegraphics` there, or use
  Insert in Document from the image's menu (the file list's menu now opens from the keyboard too).
- The build log's messages can be selected and copied, one by one or all at once.
- The comments beside the text can be hidden, from the margin or the View commands.
- retex 2.5.0: a quick fix adds the `\usepackage` a command needs, and hints about `.bib` files
  are more precise (capitals in titles only for styles that lowercase them, malformed ISBNs). In
  the browser, the whole project is built (files outside the main document's folder are found) and
  draft images work.

Known limits in the browser:

- A document that needs a package outside the site's tree fails to build; the desktop app
  downloads it.
- Tried in Chrome; the TeX tree and templates of this release not yet in Firefox.

## 0.3.0 (2026-11-24)

The browser version builds PDFs. TeXCraft's own build of retex's engine runs in the browser, with
no server, and the PDF appears beside the source as on the desktop.

### In the browser

- Compiling works: the engine runs in the background of the page, and the TeX files a document
  needs are downloaded from retex's site as it reads them. The first build after opening the page
  takes 10 to 20 seconds; later builds take well under a second.

Known limits in the browser:

- Each visit downloads the TeX files and makes the engine's format again: nothing is kept between
  visits yet.
- Projects are not kept in the browser yet: the start page cannot create a project, start one from a
  template or list recent ones. Work in the untitled project, add files with Add Files or by
  dropping them, and keep copies of your files.
- Only the TeX files retex's site carries are available (the common classes and packages); a
  document that needs another package fails to build.
- Only the main document's folder is built: files outside it are left out, and the log says so.
- Draft images are not available: images are drawn in full, and the log says so.
- Tried in Chrome and Firefox.

### Everywhere

- retex 2.4.1: OpenType features defined from Lua (`addfeature`) now substitute and ligate as with
  lualatex; completion, hover, definitions and fixes come from retex 2.4's language server.

## 0.2.2 (2026-11-23)

The first release (0.1.0, 0.2.0 and 0.2.1 were tagged but never published). TeXCraft is a LaTeX editor: a project
of files, a source editor and the PDF side by side, rebuilt when typing pauses by [retex](https://gitlab.com/severin-lemaignan/retex),
a TeX engine written in Rust. It is early software: expect rough edges, and keep your projects under
version control.

Linux packages: an AppImage and a Flatpak bundle (x86_64). On macOS and Windows, build it from
source (README, Quick start).

### Last additions

- While a build runs more than one pass, the PDF pane shows the PDF of the pass before, marked
  "Draft · pass 1", instead of waiting for the last pass; its references may show as "??" until
  the build ends.
- Stop Compiling no longer restarts the build helper: the next build after a Stop is as fast as
  any other.
- Double-clicking the last paragraph of a file included with `\input` opens that file, not the
  main document; Show in PDF from a line of the preamble no longer marks text further down that
  the line did not write.
- In the dark theme, a recent project without a PDF has a dark placeholder; the start page lines up
  its rows; the line from a comment to its passage no longer crosses the text.

### Projects and the window

- A project is a folder on your disk. A start page offers New Project, Open Folder and your recent
  projects as cards with the first page of their last PDF; File › Open Recent lists them too.
- New Project opens a gallery of 15 publishers' templates (Elsevier, IEEE, ACM, APS, AAS, MNRAS,
  Nature, ACS, APA, ASME, moderncv, Europass, tikzposter) with search, categories, source and
  licence. A template is downloaded from TeX Live the first time it is used, checked against TeX
  Live's signed package database, and kept for offline use.
- Project files with folders and the main document marked, the outline, tabs, and files added by
  Add Files or by dropping them on the window.
- A short menu bar (File, Edit, Insert, Help); every command has a shortcut you can change in
  Keyboard Shortcuts.
- Light and dark themes. The window draws its own title bar, with minimize, maximize and close in
  the top bar; Settings › Appearance › System title bar brings the system's frame back.
- Dialogs ask before unsaved changes are lost.

### The source editor

- TeXCraft's own editor: LaTeX highlighting, line numbers, error markers, grouped undo, input
  methods, and an accessible text node for screen readers.
- Find and replace with match case, regular expressions, whole words and in-selection; Replace All
  is one undoable step. Go to Line.
- Completions as you type from retex's language server: labels in references, and the commands,
  environments and citations of the document once it has been built. Hover and the lint's hints are
  there for agents.
- An Insert menu and a toolbar for the usual markup.
- Undo takes back only your own changes: an edit made outside TeXCraft or by a peer stays.

### Vim

- Vim key bindings, chosen in Settings: modes, motions, operators, text objects (LaTeX environments
  and inline math included), search, `:s`, registers, marks and macros. The mode shows in the status
  bar.

### Building

- A build starts when typing pauses and once when a project opens; the PDF keeps its place across
  builds.
- The desktop app builds through retex's worker, which keeps the preamble between builds: on a
  document with a heavy preamble a rebuild after an edit takes under a second instead of about five.
- Builds report their progress (the page, the pass) and can be stopped.
- The build log shows each error and warning as a card with retex's explanation and help, and links
  to its line. Errors are underlined on their exact span in the editor, with retex's quick fixes.
- Settings › Building chooses the engine (from the document, LuaLaTeX or pdfLaTeX), stopping at the
  first error and draft images, also from the Compile button's menu. Delete Build Files clears the
  build folder.
- TeX Live: TeXCraft uses an installed TeX Live 2025 or 2026. A package the document needs and the
  installed TeX Live lacks is downloaded; without a TeX Live at all, retex makes a tree of its own in
  its cache and downloads packages as documents need them (this needs the network the first time).
- What retex brings: SVG, JPEG XL, WebP, GIF and TIFF images in `\includegraphics`; BibTeX, biber and
  makeindex built in; minted without Python.

### The PDF

- The preview with zoom, fit to width and pages.
- Source and PDF navigation: a double-click or Ctrl+click in the PDF opens the line that made it;
  Edit › Show in PDF (Ctrl+Alt+J; Cmd+Alt+J on macOS) marks the caret's line in the PDF. A line reached from the PDF, the
  log or the outline flashes in the editor.

### The formatted view

- A Source/Formatted switch above the editor shows a `.tex` file as a document: headings, styled
  text, lists, figure cards with their images, chips for references and citations, typographic
  quotes and dashes. Formulas and anything it does not know show as their source.
- It is read-only in this release: double-click any part to edit its source.

### Review

- Comments: select text and choose Edit › Comment (Ctrl+Alt+M; Cmd+Alt+M on macOS; or `:comment` in Vim). Threads sit in
  a margin beside their passage, take replies, can be resolved, reopened, edited and deleted by their
  author, filtered, and visited with F8 and Shift+F8. A narrow editor shows them in the Review panel.
- Comments are kept in the project (`.texcraft/review/`, one file per thread) and follow the text as
  it is edited.

### Agents

- Every command of the menus can be run over a JSON control channel of the open window
  (`texcraft --control`, docs/control-protocol.md) and by an MCP server,
  `texcraft-cli mcp --automation-read-root DIR --automation-write-root DIR`, which refuses to start
  without both folders and stays inside them.
- `texcraft-cli compile FOLDER` builds a project without a window.

### The browser

- A browser build (`apps/texcraft-web`) with a loading screen that shows the download's progress, and
  files added through the browser's file picker or by dropping them.

### Known limits

- The browser build does not build documents yet: retex does not run in the browser, so Compile says
  the engine is not available there.
- The formatted view is read-only. Suggested changes, spell checking, history and working together
  are not there yet.
- The interface is in English only.
- The Flatpak does not see a TeX Live installed on the host: it uses retex's own tree, downloaded
  into its sandbox as documents need it, so its first build needs the network.
- The AppImage needs glibc 2.36 or newer (Debian 12, Ubuntu 24.04, Fedora 37 and later; not Ubuntu 22.04).
- Only x86_64 Linux packages are built; the packages are not signed.
