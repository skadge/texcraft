# Changelog

What changed in each release of TeXCraft, for the people who use it. The day-by-day record is the
progress log in [ROADMAP.md](ROADMAP.md); how a release is made is in
[docs/releasing.md](docs/releasing.md).

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
