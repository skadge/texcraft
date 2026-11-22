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
| Web | `texcraft-web` builds for wasm32 (Trunk). Building says the engine is not available there yet (phase 4). |
| Tests | One integration binary per crate. A command sweep with hostile parameters, a path-escape table, lexer coverage invariants, theme contrast, and a kittest check that a build without an engine reports why. |

Measured on the example project (`examples/article`): `texcraft-cli compile` writes a one-page PDF with 0.5 s in the engine, and the desktop app compiles it on opening and shows it in the preview.

## 2. Principles that shape the work

- **Rust only** (rule 11). No handwritten JavaScript, no webview. In the browser, the engine host, the worker and storage are Rust through wasm-bindgen and web-sys.
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
5. **Settings** (**done**: a Settings dialog with Appearance, Editor, Building and Keyboard tabs, kept through `craft-settings`). Options waiting for retex 2.1 are shown disabled with the reason. Editor options (completion, closing brackets, spelling language, key bindings) arrive with phase 2's editor.
6. **Control channel** (**done** for `engine.commands`, `engine.execute`, `document.inspect`, `document.revision`, `ui.inspect`; see [docs/control-protocol.md](control-protocol.md)). Still to do:
   - `ui.screenshot` (**done**) and synthetic input;
   - `texcraft-cli mcp --connect` to drive the open window;
   - `craft-web-api` for the web build's `window.texcraft`.
7. **Shell from craft-libs.**
   - `craft-window`: the title bar policy, with the app id `ai.storyteller.texcraft`.
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
   - A rope (`ropey`, MIT).
   - Edits as transactions, undo and redo grouped by pause and word, marks that follow edits (diagnostics, bookmarks, the build's line map), and line and column conversion in UTF-8 and UTF-16.
   - The session's `file.write` gets an `edit` sibling (`file.edit {path, edits:[{start,end,text}]}`) so agents and collaboration send changes, not whole files.
2. **Incremental lexing.** Per-line start state (text, math kind, verbatim environment, comment), re-lexing from the first changed line until the state matches again.
3. **Editor widget** (in `ui-egui` first, then a craft-libs widget so other apps can use it).
   - Gutter: line numbers, error and warning markers with tooltips, fold markers for environments and sections.
   - Bracket matching, closing brackets and `\begin`/`\end` pairs, comment toggle, indent and outdent, go to line.
   - Soft wrap, current-line highlight, selection of words and lines.
   - IME through `craft_ui::ime::Composer`.
   - AccessKit: an editable multi-line text node with value, caret and selection (RFC-0014; not a read-only document).
4. **Find and replace.** Match case, regular expression (`regex`, MIT/Apache-2.0), whole word, within selection, replace all. Then project-wide search with results grouped by file.
5. **Language features from retex-lsp, in process** (`retex_lsp::Server::handle`, no JSON-RPC transport):
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

1. **A tolerant LaTeX structure parser** (`texcraft-latex`).
   - It produces a concrete syntax tree with byte spans: commands with their optional and required arguments, groups, environments, math (inline and display), comments, verbatim, paragraphs.
   - It never fails. Unbalanced or half-typed input yields error nodes that cover the text exactly.
   - It is incremental by block, reusing the per-line lexer state from phase 2.
   - Hostile-input mutation sweep and a round-trip property: the spans tile the text.
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
2. **Progress.** Pass and page counts while building (needs R3). A draft PDF after the first pass of a first build, as retex's own web editor does.
3. **Build log.**
   - Groups with counts, entries that open the file at the line and column, help and notes.
   - The raw log with search, and the list of output files with download.
   - A count badge on the log button, with its number written out so colour is not the only signal.
4. **Source ↔ PDF navigation.**
   - Double-click in the PDF moves the cursor to the source line.
   - A divider button and `Ctrl+Click` in the source move the PDF to the place and mark it.
   - This needs the SyncTeX data from builds (R1) and a reader (R7).
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

retex's engine runs in the browser only as a `wasm32-wasip1` module. The C Lua it embeds through mlua rules out wasm32-unknown-unknown, and the engine needs `panic=unwind` with wasm exceptions. retex's own web page runs it through a JavaScript WASI host. TeXCraft can't use handwritten JavaScript, so:

1. **A WASI host in Rust** (`texcraft-wasi`, a craft-libs candidate).
   - It is compiled into the web app (wasm32-unknown-unknown) and runs in a dedicated Web Worker started from Rust (a Trunk worker target, wasm-bindgen, web-sys).
   - It instantiates `retex.wasm` through `js_sys::WebAssembly` and implements the WASI preview-1 imports retex uses (about 29) over an in-memory file system, giving the same behaviour as retex's `web/wasi.js`.
   - It returns PDF, SyncTeX and JSON records to the main thread by message.
   - A pure-Rust interpreter (wasmi) was considered and rejected: it is far slower for a TeX engine and lacks the exception-handling proposal retex needs.
2. **The TeX tree** is served from the site in the index-and-packs layout retex's `xtask wasm --site` produces, with the prebuilt format as a seed.
   - WASI reads are synchronous, so the worker prefetches what a document needs before a build (the seed read lists, then the files the previous build recorded).
   - It falls back to synchronous requests inside the worker.
   - Cross-origin isolation (needed for `SharedArrayBuffer`) comes from the host's headers (`packaging/web/_headers`), not from a service worker script.
3. **Rebuilds.** retex's memory-image snapshot (`snapshot_wasm.rs`) gives fast rebuilds after the preamble.
4. **Projects in the browser** live in IndexedDB, through web-sys. Opening and saving to cloud storage goes through `craft-cloud` (RFC-0024), and embedding in a host page through `craft-web-host`.
5. **Licences.** The LaTeX kernel and most packages are LPPL, which is not on the Craft licence allowlist. Hosting a TeX tree for the web build needs an RFC first (see §6). The desktop app reads an installed TeX Live or retex's own downloaded tree and bundles none.

### Phase 7: sharing, history and working together

1. **History.** Snapshots on save and on each successful build, a list of versions with dates (`chrono`), a diff view, and restoring a file or the project.
2. **Git.**
   - Opening a project from a repository, committing and pushing, pulling with a three-way merge and a choice per conflict.
   - Built in Rust over the hosts' HTTP APIs, as retex's GitLab integration does, and also over plain git for folders.
3. **Cloud projects and sharing.** `craft-cloud` backends, `craft_ui::share`, accounts through `craft-suite-account` (desktop).
4. **Working together in real time** waits for the suite's concurrent-text contract (RFC-0020's `Texts` API behind a mature CRDT). The editor's transactions (phase 2) and stable marks are designed so that this layer can be added. Presence uses `craft_ui::presence`, and chat belongs to the host.
5. **Review.** Comments anchored to text ranges and suggested edits shown in the margin, accepted or rejected one by one. They are designed on the same marks.

### Phase 8: comfort

- More of TeXCraft's own templates (CC0, generated in the repository), beside the publishers' ones of phase 1.
- Word count (`craft-latex`'s reader).
- Vim and Emacs key bindings.

## 4. retex

retex's design note `docs/design/embedding.md` (in the retex repository) answers TeXCraft's requests R1 to R12. [docs/retex-changes.md](retex-changes.md) says what TeXCraft does with each release and lists the points still open. In short:
- **2.1.0:** build options, progress, SyncTeX bytes, symbols and fixes in process (done in retex's `main`), and also the `retex worker` process per project (cancelling, preamble snapshots, unsaved buffers as an overlay, OS-level confinement for agent sessions), the SyncTeX reader and fragments for the formatted mode.
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
