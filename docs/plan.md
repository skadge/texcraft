# TeXCraft plan

> **Last updated:** 2026-11-22 · **Change:** templates from publishers, downloaded and cached rather than bundled

TeXCraft is a LaTeX editor for the desktop and the browser, written in Rust on egui and the Craft shared libraries, with [retex](https://gitlab.com/severin-lemaignan/retex) as its typesetting engine and language server. Its scope is that of the established online LaTeX editors: a project of files, a source editor, a PDF preview kept up to date while typing, a build log tied to the source, navigation between the source and the PDF, and later sharing, history and collaboration. Layout and conventions follow those editors where users expect them (clean-room standard, "Assets" rule 7). Code, wording, icons and artwork are our own.

Behaviour specifications from observing other editors are kept in the gitignored `plan/observations/`. The agent that writes TeXCraft's code never operates those editors (clean-room standard, "Observing a proprietary app").

## 1. Where the scaffold stands

| Area | State |
|---|---|
| Workspace | Edition 2024, Rust 1.95 (`rust-toolchain.toml`), workspace lints from the Craft rules (no `unwrap`/`expect`/`panic` outside tests, `unsafe_code = "deny"`), craft-libs at one rev (`f8c3cef`), the suite's egui and winit forks and craft-libs' vendored hayro and vello through `[patch.crates-io]`. |
| Layering | `cargo xtask layers`: `project`, `latex` (L0) → `compile`, `preview` (L1) → `engine` (L2) → `ui-egui`, `mcp` (L3) → apps. Only L3 may use egui. |
| Project model | `texcraft-project`: files by checked relative paths (no `..`, no absolute paths, size and count limits), main document, load from and save to a folder. |
| Building | `texcraft-compile`: `Backend` trait, the retex backend (in process, through `retex::Retex`), a worker thread that keeps at most one waiting job. Diagnostics carry severity, file, line, column, origin and help. |
| Preview | `texcraft-preview`: craft-pdf's bounded rasteriser. The UI draws only visible pages, again only when the zoom changes. |
| Engine session | `texcraft-engine`: `Session::execute(id, params)` is the only way to change the project. Commands: `project.{new,open,save,save_as,inspect}`, `file.{new,open,close,read,write,rename,delete,set_main,outline}`, `build.{compile,stop,auto,result}`. Each command has a label, a menu, a shortcut, a JSON schema and an enablement check that gives a reason. |
| Interface | Rail, project files with folders and a main-document mark, outline, tabs, highlighted source (`texcraft-latex` lexer), error lines, preview with zoom, fit width and page count, build log grouped by severity that jumps to the line, automatic build after typing stops and once on opening, light and dark themes from tokens with a contrast test. |
| Agents | `texcraft-cli mcp` refuses to start without both automation roots. Paths are checked after resolution (symbolic links, hard links, `..`). Building is refused while confined (see R4). |
| CLI | `texcraft-cli compile FOLDER [--output FILE]`. |
| Web | `texcraft-web` builds for wasm32 with Trunk, in the size-optimised `web` profile, with a loading screen; files come in through the browser's file picker or by dropping them. Builds run retex's WebAssembly module in a Web Worker (phase 6). |
| Tests | One integration binary per crate. A command sweep with hostile parameters, a path-escape table, lexer coverage invariants, theme contrast, and a kittest check that a build without an engine reports why. |

Measured on the example project (`examples/article`): `texcraft-cli compile` writes a one-page PDF with 0.5 s in the engine, and the desktop app compiles it on opening and shows it in the preview.

## 2. Principles that shape the work

- **Rust only** (rule 11). No handwritten JavaScript, no webview. In the browser, the engine host, the worker and storage are Rust through wasm-bindgen and web-sys. One departure, decided by the operator (2026-11-22): `apps/texcraft-web/loader.js`, Trunk's initializer hooks that show the download's progress on the loading screen, since no Rust runs before the WebAssembly module has arrived.
- **Commands first** (rule 14). Every panel action is a registered command, so the control channel, the page API and the agent tools can do everything the interface does. A UI change starts by adding or reusing a command.
- **Shared library first** (rule 9). Widgets, parsers and protocols that another Craft app could use go to craft-libs: the code editor widget, the SyncTeX reader and the WASI host are candidates. Each move is its own PR.
- **Honest results** (rule 15). An unsupported feature is disabled with its reason or answers with an error, in the interface and in automation.
- **Accessibility from the start** (rule 21, standards/accessibility.md). Keyboard paths, focus, names, live-region announcements, and the editor as an editable text node.

## 3. Phases

Each phase ends with a usable app. Work items name the shared crate or retex change they depend on.

### Phase 1: the window and the suite (in progress)

Done items are marked **done** with the commit subject that brought them. The rest is in the order it is planned.

1. **The window's look** (**done**). The design language in [docs/design.md](design.md): theme tokens, Inter and a monospace face from craft-fonts, Lucide icons, TeXCraft's own controls. The window is rebuilt on them: top bar, rail, file tree and outline, editor pane, preview pane and build log, status bar.
2. **Projects and files.**
   - Native pickers through `craft-dialogs` (**done**): open a project folder, save to a folder, save the PDF, add files.
   - Asking before unsaved changes are lost, through `craft-quit` for the window (**done**).
   - **Recent projects** (**done**): File › Open Recent and the project menu list them by folder name with their location, mark the open one, show missing folders disabled with the reason, and can be cleared. Up to twelve, kept in the preferences.
   - Still to do:
     - A start page when no project is open (**done**: new project, open folder, recent projects as cards with the first page of their last PDF, missing folders marked, removing an entry). Templates join it in phase 8.
     - Recent files of the open project in the Go to File palette (with phase 2's palette).
     - Recent projects in the operating system's own lists (the dock menu on macOS, jump lists on Windows, recently-used on Linux).
     - Pinning a project to the top of the list, and removing one entry.
     - Renaming a project folder from the project menu.
2b. **Templates from publishers.** A new project can start from a journal's, a conference's, a thesis or a CV template. The templates are not bundled: their licences rarely allow it (many are LPPL, which the Craft licence list lacks, and some publishers' terms forbid redistribution). TeXCraft downloads them from their original source, the way retex downloads TeX Live packages, and keeps them for offline use.
   - **Status:** the catalogue (15 templates from TeX Live packages), checked downloads, kept copies, `template.list`, `template.fetch`, `project.from_template`, and the gallery with categories, search, source and licence, and Download All are **done**. Templates come from the installed TeX Live's year so they match its classes; `cargo xtask templates` fetches and builds each one. The start page shows a card per template; choosing one downloads that template only. Still to do: thumbnails, refreshing kept copies when TeX Live's version changes, and publishers' own downloads.
   - **The catalogue** is TeXCraft's own data (`templates/catalog.toml`, MIT OR Apache-2.0), one entry per template:
     - its title, publisher and category (journal, conference, thesis, report, CV, letter, slides, poster, book);
     - where it comes from;
     - its licence as its source states it;
     - its main file.

     Sources, in order of preference:
     - **TeX Live packages.** Most publishers' classes are on CTAN with a template in their documentation: `elsarticle`, `IEEEtran`, `acmart`, `llncs`, `revtex`, `amsart`, `aastex`, `mnras`, `beamer`, `moderncv`, `tufte-latex` and more. The template comes from the package's documentation container, fetched from a TeX Live mirror and checked against TeX Live's signed package database, through `retex-texlive`. The class itself is in TeX Live already.
     - **A publisher's own download.** For templates not on CTAN: a pinned URL with its SHA-256. A changed file is refused until the catalogue is updated.
     - **TeXCraft's own templates.** Generated in the repository, CC0. Only these, and those whose licence allows it, may ever be bundled.

     The catalogue is never built from another editor's template gallery.
   - **Downloading.**
     - At first start, TeXCraft asks once whether to download the catalogue's templates for offline use, giving their total size. The answer is a setting.
     - Otherwise a template is downloaded the first time it is chosen, with progress, and kept.
     - Downloads go to the data folder (`TeXCraft/templates/<id>/<version>/`), with the source's licence text beside them. Updates follow the catalogue's versions.
     - Without a connection, the kept templates are offered and the others say they need one.
     - Confined agent sessions download nothing (RFC-0015); they use kept templates only.
   - **Choosing.** The start page's New Project becomes a gallery:
     - categories and search by title, publisher or class;
     - a thumbnail per template, its first page, built once locally after download and kept;
     - the source and the licence shown before creating.

     Creating copies the template's files into a new folder, opens it, and keeps the licence notice in the project (`TEMPLATE-LICENSE`). Users can also save a project as their own template.
   - **Commands** for agents: `template.list` (with categories, source, licence, whether kept) and `project.from_template {template, folder}`.
   - **The web build** offers only TeXCraft's own templates and those whose licence allows hosting, served by the site itself, because CTAN mirrors send no CORS headers. Hosting LPPL files waits for the RFC of §6.
3. **Menus** (**done** for File, Insert, Format, Build, View, Help, all built from the command registry). Still to do:
   - An Edit menu with undo, redo, find and go to line (with phase 2's editor);
   - the PDF in a separate window (an egui viewport);
   - a distraction-free mode that hides the menus and the rail;
   - `craft-menu` for the macOS menu bar.
4. **Keyboard** (**done**: `craft-keymap` for every shortcut, user overrides kept in the preferences, the shared Keyboard Shortcuts dialog, F6 and Shift+F6 between regions). Still to do: Alt or F10 to reach the menu bar.
5. **Settings** (**done**: a Settings dialog with Appearance, Editor, Building and Keyboard tabs, kept through `craft-settings`). The Building tab has the engine, stopping at the first error and draft images (retex 2.1). Editor options (completion, closing brackets, spelling language, key bindings) arrive with phase 2's editor.
6. **Control channel** (**done** for `engine.commands`, `engine.execute`, `document.inspect`, `document.revision`, `ui.inspect`; see [docs/control-protocol.md](control-protocol.md)). Still to do:
   - `ui.screenshot` (**done**) and synthetic input;
   - `texcraft-cli mcp --connect` to drive the open window;
   - `craft-web-api` for the web build's `window.texcraft`.
7. **Shell from craft-libs.**
   - `craft-window`: the title bar policy, with the app id `io.gitlab.severin_lemaignan.TeXCraft`.
   - `craft-ui`'s `panes::PaneHost` for dividers with collapse handles.
8. **Accessibility.**
   - Build results are announced through a live region (**done**).
   - Every control has a role, name and state, and the kittest harness's check stays on (**done**; it found unnamed controls twice).
   - Still to do: kittest keyboard-path tests for each panel, and a screen-reader pass.
9. **Translations.** Request `texcraft.tsv` in storytold/craft-i18n and pin it with a `craft-i18n.rev` file. Plural forms are used for counts (**done** in the status bar).
10. **Release plumbing.**
    - GitLab CI running `cargo xtask ci` (**done**; the project is on gitlab.com, so not craft-common-actions).
    - The app icon (**done**, `assets/app-icon/`, `cargo xtask icons`) and the Linux desktop entry, metainfo and a user install script (**done**, `packaging/linux/`). Still to do: macOS and Windows packaging, Flatpak.
    - `craft-update`, which needs a `texcraft` entry in craft-libs' `update/src/suite.rs` (a craft-libs PR).
    - `cargo xtask assets` to keep `ATTRIBUTION.md` complete.
    - `contributors/`.

### Phase 2: the source editor

The interim editor is egui's multi-line text field with a highlighting layouter. It cannot show line numbers, folds, gutter markers, multiple carets or a proper accessibility node, and it copies the whole text on every keystroke. Phase 2 replaces it.

1. **Buffer crate** (`texcraft-buffer`, L0).
   - **Status:** the crate is **done** (rope, transactions, grouped undo, marks, UTF-16 positions, edit origins, and others' edits that undo skips). The editor sends its edits with `file.edit` (docs/collaboration-roadmap.md, F1).
   - A rope (`ropey`, MIT).
   - Edits as transactions, undo and redo grouped by pause and word, marks that follow edits (diagnostics, bookmarks, the build's line map), and line and column conversion in UTF-8 and UTF-16.
   - The session's `file.write` gets an `edit` sibling (`file.edit {path, edits:[{start,end,text}]}`) so agents and collaboration send changes, not whole files.
2. **Incremental lexing.** Per-line start state (text, math kind, verbatim environment, comment), re-lexing from the first changed line until the state matches again.
3. **Editor widget** (in `ui-egui` first, then a craft-libs widget so other apps can use it).
   - **Status:** the widget replaces egui's text field (`crates/ui-egui/src/source.rs`): one galley per line, the gutter with line numbers and markers, the caret line's band, soft wrap, mouse selection of words and lines, standard keys with grouped undo, Tab and Shift+Tab on lines, Ctrl+/ for comments, IME through craft-ui's `Composer`, and an editable AccessKit node with one text run per line. Bracket matching, closing brackets and `\begin`/`\end` pairs (a setting), and Go to Line are **done**. Still to do: folding, several carets.
   - Gutter: line numbers, error and warning markers with tooltips, fold markers for environments and sections.
   - Bracket matching, closing brackets and `\begin`/`\end` pairs, comment toggle, indent and outdent, go to line.
   - Soft wrap, current-line highlight, selection of words and lines.
   - **To do (operator, 2026-11-24):** a context menu on right-click in the text, with Add a Comment when text is selected (beside Cut, Copy and Paste), opened from the keyboard too (the Menu key, Shift+F10).
   - **To do (operator, 2026-11-24):** dragging an image from the file list onto the text inserts its `\includegraphics` (with the path relative to the main document's folder, and `graphicx` loaded if it is not), at the place it is dropped.
   - IME through `craft_ui::ime::Composer`.
   - AccessKit: an editable multi-line text node with value, caret and selection (RFC-0014; not a read-only document).
3b. **Vim mode** (essential, part of the widget from the start, not an add-on). A setting (Settings › Editor and Settings › Keyboard: Editor key bindings, Standard or Vim), kept per user, with the mode shown on Vim's line at the bottom of the editor, with the command line and messages (`NORMAL`, `INSERT`, `VISUAL`, `V-LINE`, `REPLACE`, and the pending command).
   - **Status:** **done** in `texcraft-vim` and the widget, with the setting and Vim's line at the bottom of the editor (mode, command line, messages; announced when they change). Not yet: Visual block mode, `=`, back-references in patterns, case changes in replacements, `:s` with `c` (each says so when used), and `"+p` pastes the system clipboard only through the window's Paste, as egui cannot read it on demand.
   - **Normal mode.** Counts, motions (`h j k l w W b B e E 0 ^ $ gg G f F t T ; , % { }`, `H M L`), text objects (`iw aw iW aW is as ip ap i( a( i{ a{ i[ a[ i" a"`, and LaTeX's own: `ie ae` for an environment, `i$ a$` for inline math, `ic ac` for a command's argument). Operators (`d c y > < = gu gU g~`) with motions and objects. `x X s S r J p P u Ctrl-R .` and `~`.
   - **Insert mode** and its keys (`Ctrl-W Ctrl-U Ctrl-O`), and completion (phase 2's) that cooperates with it.
   - **Visual modes:** character, line and block, with the operators.
   - **Search:** `/ ? n N * #`, sharing the find bar's matches.
   - **Command line:** `:w` (save), `:q` (close the file), `:wq`, `:e FILE`, `:s` and `:%s` with flags, `:{line}`, `:noh`, `:set` for a few options.
   - **Registers** (unnamed, named, `"+` the system clipboard, `"0`), **marks** (`m` and `` ` `` and `'`), the **jump list** (`Ctrl-O Ctrl-I`), and macros (`q` and `@`).
   - Undo groups follow Vim's: one per change, an insert session is one change.
   - Shortcuts: in Normal mode, command chords of the keymap (Ctrl+S, Ctrl+Enter, F6) still run; plain keys belong to Vim. In Insert mode, the editor's text-editing chords apply as usual.
   - **Accessibility:** the mode is announced on change, and the caret's shape (block, bar, underline) is never the only signal of the mode.
   - **To do (operator, 2026-11-24):** selecting text in Vim mode (with the mouse, and Visual mode's selection) for the commands that act on a selection, such as Edit › Comment: today a passage cannot be selected for a comment in Vim mode.
   - Tests: a table of keystroke sequences and the text and caret they must give, written from Vim's documentation (`:help`), as the clean-room standard allows. Vim's code is not read, and its documentation is not copied.

4. **Find and replace.** Match case, regular expression (`regex`, MIT/Apache-2.0), whole word, within selection, replace all. Then project-wide search with results grouped by file.
   - **Status:** the find bar is **done** (Edit › Find, Cmd+F; Replace, Cmd+Alt+F): match case, regular expression, whole word, in selection, the count, Previous/Next, Replace and Replace All as one undoable edit; agents have `file.find` and `file.replace` with the same matching. Project-wide search is to do. Edit › Go to Line… moves the caret to a line.
5. **Language features from retex-lsp, in process** (`retex_lsp::Server::handle`, no JSON-RPC transport):
   - **Status:** the server runs in the engine (`crates/engine/src/language.rs`) and the editor has its completion list (after `\`, in `\ref{`, `\cite{`, `\begin{`…, and on Ctrl+Space; Up/Down, Enter/Tab, Escape; in Vim's Insert mode too). Agents have `language.complete`, `language.hover`, `language.definition` and `language.lint`. Each build's `symbols` record (retex 2.1.0) gives completion and hover the document's commands, environments, labels and citations. The lint's hints show as typed, in the gutter and underlined. Ctrl+click and F12 go to a definition (a reference to its label), and resting the pointer shows the server's description. Problems are underlined by severity; resting the pointer on one (or Edit › Quick Fix, Cmd+.) opens its message, help and fixes, with Fix All for hints; agents have `language.fixes` and `language.fix`. Still to do: links, the outline from the server.
   - Completion of commands, environments, `\ref` labels and `\cite` keys.
   - Hover, go to definition (also into read-only TeX Live files).
   - Outline from `documentSymbol`.
   - Quick fixes from diagnostics (`Ctrl+.`) and lint hints while typing.
   - Builds feed it the `symbols` records (needs R1).
6. **Writing aids.**
   - An insert toolbar: bold, italic, section levels, lists, figure, table, inline and display math, citation, reference.
   - A symbol palette (our own grouping and drawing).
   - Spell checking with `craft-proof` and craft-i18n dictionaries, which skips commands, math and verbatim.
7. **Files.**
   - File tree actions: new folder, rename, delete with confirmation, drag to move, upload by drop.
   - Image and PDF previews for binary files.
   - Preview tabs (a single click shows a file without pinning a tab).

### Phase 3: formatted editing mode

A second way to show the same file: sections as headings, emphasis as bold and italic, lists as lists, formulas typeset, figures as cards with their images, tables as editable grids, and the preamble folded away. It is a view of the source, not a separate document model:

- The `.tex` file stays the only truth.
- Switching modes never rewrites it.
- Every edit made in formatted mode is a minimal edit of the source, so diffs, git and collaboration see ordinary text changes.

A toggle at the top of the editor switches between **Source** and **Formatted**, per file, remembered with the project.

**Status:** the toggle works for `.tex` files, per file for the session (not yet remembered with the project). The formatted view is read-only: it draws the view model (item 2) at a reading measure in Source Serif 4, with formulas as their source in a soft pill, chips for labels, references, citations and `\input`, footnote markers, links, figure cards with their images, the preamble folded into a bar, and source blocks for the rest. Double-clicking a part, or Enter on it, shows the source with the caret at that part's start. It re-parses the whole file when the session's revision changes.

1. **A tolerant LaTeX structure parser** (`texcraft-latex`).
   - It produces a concrete syntax tree with byte spans: commands with their optional and required arguments, groups, environments, math (inline and display), comments, verbatim, paragraphs.
   - It never fails. Unbalanced or half-typed input yields error nodes that cover the text exactly.
   - It is incremental by block, reusing the per-line lexer state from phase 2.
   - Hostile-input mutation sweep and a round-trip property: the spans tile the text.
   - **Status:** the parser is in (`texcraft_latex::tree`), with the sweep and the tiling check. It re-parses the whole file; incremental parsing by block is still to do.
2. **The view model.** It maps the tree to blocks and inline atoms, each holding the source span it stands for:

   | Source | Shown as |
   |---|---|
   | Preamble (up to `\begin{document}`) | One folded bar; expanding it shows the source |
   | `\maketitle` with `\title`, `\author`, `\date` | Title block |
   | `\part` … `\subparagraph`, starred or not | Headings at their level, with the number the last build gave them (from the `.aux`) |
   | `\textbf`, `\textit`, `\emph`, `\underline`, `\texttt` and their nesting | Styled text, commands hidden |
   | `itemize`, `enumerate`, `description` | Lists with markers and numbers |
   | `$…$`, `\(…\)` | Inline formula image on the baseline |
   | Display math and the math environments | Centred formula image, with the equation number from the last build |
   | `\label`, `\ref`, `\eqref`, `\cref`, `\cite` and kin | Chips: an icon and the key; hovering shows the target (the label's heading or formula, the reference's title and authors) |
   | `figure` with `\includegraphics`, `\caption` | A card with the image (PNG and JPEG through craft-image-codecs, PDF pages through craft-pdf), the caption and an edit button |
   | `table` with `tabular` | A card with a grid editor (item 5) |
   | `\footnote`, `\url`, `\href` | Marker with a popover; link text |
   | Anything else | The source itself, inline, in the code font. Nothing is hidden that cannot be shown faithfully |

   **Status:** the mapping is in (`texcraft_latex::view`), except heading and equation numbers from the `.aux`; tables stay as source until the grid editor. Text environments (`abstract`, `quote`, `center`…) show their body formatted under their name, and a figure that holds more than an image, a caption and a label stays as source.

3. **Typeset formulas.**
   - Formulas are typeset by retex with the document's own preamble, in batches, through a fragment API (docs/retex-changes.md, R11).
   - The images are cached by the formula text and the preamble's hash, and drawn by craft-pdf at the screen's density.
   - While a formula is being typed, its source is shown with a live typeset preview above it, the same preview that source mode shows inside display math.
   - A formula that fails to typeset shows its source with the error underlined.
4. **Editing in the formatted view.**
   - The caret moves through text runs as through text. Atoms (chips, formulas, figures) are single caret stops: Backspace next to one selects it first and a second press deletes its whole source span.
   - Enter ends a paragraph (a blank line). Inside a list it starts a new item.
   - Typing at a style boundary continues the style the caret is in.
   - Bold, italic, heading levels and lists from the toolbar wrap or rewrite exactly the selected source span.
   - Pressing Enter or double-clicking on a formula or a fallback span opens its source in place, edited with the same widget and completion as source mode, and closes back to the rendered form.
   - Undo history is shared with source mode, since both edit the same buffer.
5. **Table grid editor.**
   - `tabular` (and `tabularx`, `longtable` later) is parsed into rows and cells, with `\multicolumn` spans, `\hline`, booktabs rules and the column specification.
   - Cells are edited in place.
   - A floating toolbar offers: insert and delete rows and columns, merge and split cells, alignment per column, borders (none, all, booktabs), and the caption position.
   - Every operation writes only the cells and separators it changes and keeps the author's spacing elsewhere.
   - Tables that use features the grid cannot represent faithfully stay as source, with a message saying which feature.
6. **Figures.**
   - The edit button opens a dialog: image from the project or from a file, width, placement, caption, label.
   - Dropping an image file on the view adds it to the project and inserts a figure.
7. **Accessibility.** The formatted view publishes a real structure: headings with levels, lists, tables with cells, formula images named by their source (later by a spoken form from `craft-math`'s MathML). It remains an editable text node with caret and selection.
8. **Typography.** The formatted view uses a text face chosen for reading (from craft-fonts) at a comfortable measure (about 70 characters), centred in the pane, with spacing from tokens. It does not imitate the PDF. The preview pane is for that.

### Phase 4: building, preview and navigation

1. **Build menu.**
   - Automatic build on or off.
   - Normal or draft build (images as boxes, through retex's draft option).
   - A syntax check before building.
   - Stop on the first error, or go on.
   - Delete build files.
   - Stop the build (needs R2).
2. **Progress** (**done**: page of pages, the pass and why, the time; the first build says why it is slower). Pass and page counts while building (needs R3). A draft PDF after the first pass of a first build, as retex's own web editor does.
   - **To do (operator, 2026-11-24):** the page being built also in the PDF pane's header, in a dim tone so that it adds little noise, beside the PDF it is about; today it is only in the status bar, far from the PDF.
3. **Build log.**
   - Groups with counts, entries that open the file at the line and column, help and notes.
   - The raw log with search, and the list of output files with download.
   - A count badge on the log button, with its number written out so colour is not the only signal.
4. **Source ↔ PDF navigation.**
   - Double-click in the PDF moves the cursor to the source line.
   - A divider button and `Ctrl+Click` in the source move the PDF to the place and mark it.
   - This needs the SyncTeX data from builds (R1) and a reader (R7).
   - **Status:** **done** with retex 2.1.0's `retex-synctex`: double-click or Ctrl+click in the PDF opens the source line; Edit › Show in PDF (Cmd+Alt+J, and Ctrl+click in the editor where nothing is defined) marks the caret's line in the PDF for a moment; agents have `pdf.source` and `pdf.locate`. A click in the last paragraph of an `\input` file goes to the including file (docs/retex-changes.md).
5. **Preview.**
   - Page rendering on a background thread with a texture cache, and the scroll position kept across rebuilds.
   - Page navigation, zoom presets (fit width, fit page, 50–400 %), and a presentation mode.
   - Links followed through `craft_pdf::safe_url`, and internal links.
   - Downloading or exporting the PDF.
   - Text selection later (craft-pdf text extraction).

### Phase 5: output formats and print

retex writes more than a screen PDF, and TeXCraft offers it: one source, several outputs, each a command (so agents reach them too), each kept per project.

1. **Output settings per project**, in the project's settings and the Build menu's Output submenu:
   - The main output: screen PDF (default), accessible PDF, or PDF/X for print.
   - Additional outputs written with each build or on demand: SVG, OpenDocument, HTML, Markdown, PNG pages.
   - Where outputs go, which by default is the project folder under the main document's name.
2. **Export commands** (File › Export), each with a file dialog and a progress line:
   - **SVG**: the whole document as one SVG file (`Document::to_svg`).
   - **OpenDocument**: an article as a Writer document (`.odt`), a beamer deck as an Impress presentation (`.odp`), marked experimental as retex marks them (`to_odt`, `to_odp`).
   - **HTML** and **Markdown** with their images, as a folder or a zip (`to_html`, `to_markdown`).
   - **PNG pages** at a chosen resolution (`Document::render`).
   - TeXCraft says which output a document is suited to: Impress output from a non-beamer class is allowed, with a note that it is meant for slides.
3. **Accessible PDF** (PDF/UA-2, `Retex::accessible`).
   - A build option and an export.
   - retex's accessibility findings shown in the log, in their own filter, each linked to the source: missing alternative text, reading order, unlabelled tables.
4. **PDF/X for print** (PDF/X-4 and PDF/X-1a).
   - Bleed (default 3 mm) and crop marks as project settings.
   - The preview can show the trim, the bleed and the crop marks, as a view mode of the PDF pane.
5. **Preflight checks** after GWG 2022, run on every PDF/X build and on demand:
   - **Findings:** image resolution (with the effective ppi and the page), total ink coverage, text too small for its colours, objects missing the bleed.
   - **Preflight panel:** a tab of the log listing each finding with its severity and where it is. Selecting one marks the area on the page and, through SyncTeX, opens the source line that placed it (the image's `\includegraphics`, the text's paragraph).
   - **Report:** a summary at the top ("ready for print" or the number of problems), and a report export (text or PDF) for the printer.
6. **Engine identity** in the project's settings: retex's choice, `lualatex`, or `pdflatex` (`Retex::identity`, retex 2.1). The pdflatex mode is described as newer.

The in-process API has the ODT, ODP, SVG, HTML, Markdown, page images and accessibility pieces today. PDF/X and structured preflight findings are on retex's command line only, which is asked for as R13 in [docs/retex-changes.md](retex-changes.md). Finding a finding's source line needs the SyncTeX reader (R7).

### Phase 6: the browser

retex's engine runs in the browser only as a `wasm32-wasip1` module. The C Lua it embeds through mlua rules out wasm32-unknown-unknown, and the engine needs `panic=unwind` with wasm exceptions. retex's own web page runs it through a JavaScript WASI host. TeXCraft can't use handwritten JavaScript, so it has its own host in Rust.

**Status (2026-11-24): builds run in the browser** (Chrome and Firefox tried). A first build of the blank project takes about 18 s (the engine loads in 3 s, the format is made in 5 s, the rest is fetching the tree's files); later builds resume from the preamble's snapshot, in 0.3 to 0.5 s.

1. **The engine** (**done**) is TeXCraft's own build of the retex release `Cargo.lock` pins (`cargo xtask retex-wasm`: nightly with `rust-src`, wasi-sdk), not the module retex's site serves (the operator's decision, 2026-11-24). The web build serves it beside the app; CI's web job builds it with a dated nightly.
2. **A WASI host in Rust** (**done**: `crates/wasi`, a craft-libs candidate).
   - The 29 preview-1 imports retex uses over an in-memory file system, as plain Rust with no I/O of its own (tested natively), and a binding that instantiates the module through `js_sys::WebAssembly` (`path_open`'s 9 arguments, over wasm-bindgen's 8, go through a `Proxy`).
   - It runs in a dedicated Web Worker (`apps/texcraft-web/src/bin/engine-worker.rs`, a Trunk worker target) that sends progress records, drafts and the outcome (PDF, SyncTeX, retex's JSON records, read by `texcraft_compile::records`) to the editor (`texcraft_compile::web::WebWorker`, the session's builder in the browser).
   - A pure-Rust interpreter (wasmi) was considered and rejected: it is far slower for a TeX engine and lacks the exception-handling proposal retex needs.
3. **The TeX tree** (**done**, from retex's site for now: operator's decision, 2026-11-24) comes in the layout retex's `xtask wasm --site` produces: `tree/index.txt` (a line per file), the files and packs of small files, and per-example read lists. retex's site (`severin-lemaignan.gitlab.io/retex/`) serves it with `Access-Control-Allow-Origin: *`, so TeXCraft hosts no LPPL file (§6, question 4). The site's `seed/` caches (format, indexes) are made by its own module and not used: TeXCraft's engine makes its own (R17 in [retex-changes.md](retex-changes.md)).
   - WASI reads are synchronous. Before its first build the worker fetches what a plain article reads (retex's read list); a file still missing is fetched with a synchronous request in the first 1.5 s of a run. After that, Chrome lets a synchronous request hang once the worker has not returned to its event loop for a few seconds (observed), so the run stops instead, the file is fetched, and the run starts again (7 times on the first build of the blank project, for fonts).
   - **Kept between visits** (2026-11-24): the fetched files and what the engine makes in `/cache` (format, indexes) go to the browser's Cache Storage, in caches named after the tree's index and the module, so a later visit neither fetches nor makes them again (a blank project's first build: 5.8 s on a first visit, 0.8 s on the next, Chrome).
   - **Next:** a read list per project; JavaScript Promise Integration (`WebAssembly.Suspending`, Chrome 137 and later) would let a run wait for a fetch instead of stopping.
   - **To decide for deployment:** Trunk does not hash the worker's file names, so a browser can keep an old worker after an update; the host's cache headers, or versioned names.
4. **Projects in the browser** (**done**, 2026-11-24) are kept by name in the session's library (`texcraft_engine::library`; `project.open`, `project.save_as` and `project.delete` take a name there), which the web app stores in IndexedDB (`apps/texcraft-web/src/store.rs`, a craft-libs candidate) with the preferences and the thumbnails. The start page lists them, the last saved first.
   - **Templates** (2026-11-24): `cargo xtask templates-site OUT` fetches every template as the desktop does (TeX Live's signed database, checked archives) and writes it to `OUT/templates/<id>/` with `files.txt`; CI's web job runs it into the web build, so the page fetches a template from its own site (TeX Live's mirrors send no CORS headers). Only 4 of the 15 build there yet (acmart, revtex4-2, IEEEtran are in retex's tree; nature, elsarticle, moderncv and the others are not): their packages have to be served too (item 6).
   - **Not yet:** review threads of a browser project are not kept (they live in the project's folder on the desktop); a warning before leaving the page with unsaved changes. Opening and saving to cloud storage goes through `craft-cloud` (RFC-0024), and embedding in a host page through `craft-web-host`.
5. **Licences.** The LaTeX kernel and most packages are LPPL, which is not on the Craft licence allowlist. Hosting a TeX tree for the web build needs an RFC first (see §6). The desktop app reads an installed TeX Live or retex's own downloaded tree and bundles none.
6. **Packages missing from the tree** (checked against retex 2.4.1, 2026-11-24). retex downloads a missing TeX Live package natively (`retex-texlive`'s `download`, through kpathsea's miss hook in retex-cli), but not in the browser, for three reasons:
   - **No source.** TeX Live's mirrors send no CORS headers (`mirrors.ctan.org` and eight mirrors tried), so a browser can't fetch from them. A CORS-enabled copy of the year's tlnet archives (about 1.7 GB a year, over GitLab Pages' 1 GB) has to be hosted somewhere: open question 5 of retex's `docs/design/package-download.md`.
   - **No miss reaches the host.** The WASI module is built without `download`, and no kpathsea miss hook calls out of the module: a missing file fails the build ("File `x.sty' not found"). A retex change (a hook that asks the host, through an import or a reserved path) lets the host fetch it during the run. Without it, the host can fetch after the failure and build again, once per missing package (the tlpdb's dependencies cut the rounds).
   - **The host's side.** The parts of `retex-texlive` a host needs build for wasm32-unknown-unknown with `download` (tried with 2.4.1): the tlpdb index and file-to-package lookup, the OpenPGP check, xz, SHA-512. `Tree`, which installs into `std::fs`, does not run there, so the host installs into its in-memory files and keeps them in the origin private file system; retex's design proposes an I/O-injected package crate for this.

### Phase 7: sharing, history and working together

1. **History.** Snapshots on save and on each successful build, a list of versions with dates (`chrono`), a diff view, and restoring a file or the project.
2. **Git.**
   - Opening a project from a repository, committing and pushing, pulling with a three-way merge and a choice per conflict.
   - Built in Rust over the hosts' HTTP APIs, as retex's GitLab integration does, and also over plain git for folders.
3. **Cloud projects and sharing.** `craft-cloud` backends, `craft_ui::share`, accounts through `craft-suite-account` (desktop).
4. **Working together in real time** waits for the suite's concurrent-text contract (RFC-0020's `Texts` API behind a mature CRDT). The editor's transactions (phase 2) and stable marks are designed so that this layer can be added. Presence uses `craft_ui::presence`, and chat belongs to the host.
5. **Review.** Comments anchored to text ranges and suggested edits shown in the margin, accepted or rejected one by one. They are designed on the same marks.
   - **Status:** comments (docs/collaboration-roadmap.md, R1) are **done**: threads in a margin beside their passages (a Review panel when the editor is narrow), replies, resolving, filters, F8, kept in `.texcraft/review/` (docs/review-format.md) and reachable by agents through `review.*`. Suggested changes (R2) are next.

### Phase 8: comfort

- More of TeXCraft's own templates (CC0, generated in the repository), beside the publishers' ones of phase 1.
- Word count (`craft-latex`'s reader).
- Emacs key bindings.

### Phase 9: AI assistance

Help from a language model the user chooses, for writing and for LaTeX's chores. It is off until the user sets it up, and nothing leaves the machine without saying what and to whom.

1. **The connection.**
   - **Settings › AI:** an OpenAI-compatible endpoint (base URL), a token, and a model picked from the endpoint's model list. A local server (llama.cpp, Ollama, vLLM) is the same thing at a loopback address, with no token.
   - **Built on the suite's AI layer** (craft-libs: `craft-ai-core`'s connections, models, roles, consent and request log; its OpenAI-compatible provider; the shared store and connection panel, where they have landed). TeXCraft adds no client of its own (rule 9), and registers its tasks as roles of that layer.
   - **The token** goes to the system's secret store (`SecretString`), never into the preferences file or a log.
   - **Disclosure:** the first use of each kind of request shows what will be sent (the selection, the paragraphs around it, the preamble, a build message) and to which endpoint, and asks once per connection. Each request is logged with its size and the endpoint. A project can turn AI off for itself (`.texcraft/` setting), for work under embargo.
   - **The browser:** the same, if the endpoint answers cross-origin requests (many hosted APIs don't). The setup says so when the endpoint refuses.
2. **What it does.** Every change it proposes arrives as a suggested change (`docs/collaboration-roadmap.md`, R2), so it is reviewed, accepted or rejected like a co-author's, and undone like any edit. It never writes to a file by itself.
   - **Explain a build message:** on a card of the build log or in the problem popup, a plain-language explanation of the error with the source around it, and a proposed fix as a suggestion.
   - **Rewrite the selection:** clearer, shorter, more formal, fix grammar and spelling, change the tense or voice, translate. LaTeX markup, labels, citations and maths are kept as they are, and the result is checked for it before it is offered.
   - **Continue as you type:** an optional grey completion after the caret, accepted with Tab (also in Vim's Insert mode), after a pause, cancelled by typing on.
   - **Make LaTeX from a description:** a table from pasted data or a sentence, an equation from words or a photo of a formula (when the model takes images), a TikZ figure, a `figure` environment. Each is typeset in the preview before it is inserted.
   - **Bibliography help:** a BibTeX entry completed from a title or DOI, marked as model output, never added without review (models invent references).
   - **Ask about the document:** a side panel to ask questions about the open file or the project (what a macro does, where a symbol is defined, what a reviewer might object to), with answers that link to the lines they mention.
   - **Review:** "Review this section" leaves comments in the margin (R1), written by "AI (model name)", for the author to resolve.
3. **For agents and automation:** the same tasks as commands (`ai.explain`, `ai.rewrite`, `ai.generate`…), which refuse plainly when no connection is set.
4. **Tests:** against craft-ai-core's mock provider: what is sent for each task (no more than disclosed), suggestions that keep markup, refusals without a connection or consent, cancellation; nothing in tests reaches a network.

## 4. retex

retex's design note `docs/design/embedding.md` (in the retex repository) answers TeXCraft's requests R1 to R12. [docs/retex-changes.md](retex-changes.md) says what TeXCraft does with each release and lists the points still open. In short:
- **2.1.0 (in use):** progress, cancelling, SyncTeX, symbols and fixes, and the worker's preamble snapshot in the desktop app (docs/retex-changes.md); still to use: the `retex worker` process per project (cancelling, preamble snapshots, unsaved buffers as an overlay, OS-level confinement for agent sessions), the SyncTeX reader and fragments for the formatted mode.
- **2.2.0:** typed language-server queries.

Phases 2 to 5 of this plan follow those releases. Where a phase needs a later release, its work starts on the TeXCraft side (interfaces, UI, tests against the in-process backend) and is finished when the release lands.

## 5. Quality gates

- `cargo xtask ci`: fmt, clippy with `-D warnings`, tests, layering, wasm32 build.
- Every new command is covered by the hostile sweep without changes. Every parser (lexer, SyncTeX, project loader) gets a mutation sweep.
- UI changes come with a kittest keyboard path and read the accessibility tree, and the harness's accessibility check stays on.
- Every colour token pair is in the contrast test.
- Corpus: build retex's sample documents through `texcraft-cli compile` and compare pages and diagnostics with retex's command line. A difference is a TeXCraft bug unless retex differs from itself.

## 6. Decisions for the operator

1. **Name.** "TeX" is a trademark of the American Mathematical Society. The Craft naming standard (rule 22) asks for a trademark check before shipping a user-visible name. Many editors use the word, but that does not settle it. Run the check, or choose another name.
2. **Licence.** The scaffold uses the suite's `MIT OR Apache-2.0`. retex is CC0, so either works.
3. **Home of the repository.** Craft rules expect GitHub under storytold (craft-common-actions workflows, the one-rev craft-libs rule, the add-an-app RFC with its 7-day comment period). The project is on gitlab.com today. Joining the suite means an RFC in craft-contrib.
4. **TeX tree on the web.** Hosting LPPL files needs `LPPL-1.3c` on the licence allowlist (an RFC), or a design where the browser fetches the tree from a host the user chooses.
