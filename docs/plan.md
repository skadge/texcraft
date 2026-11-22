# TeXCraft plan

> **Last updated:** 2026-11-22 · **Change:** first plan, with the scaffold

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

### Phase 1: the window and the suite (next)

1. **Shell from craft-libs.**
   - `craft-window`: title bar policy and the app id `ai.storyteller.texcraft`.
   - `craft-ui`: `panes::PaneHost` and `Rail` for the side panel and the dividers (collapse arrows on the dividers), `ComboBox`, and the account and share pieces later.
   - `craft-menu`: the macOS menu bar.
   - `craft-quit`: quitting with unsaved changes.
   - `craft-dialogs`: native folder and file pickers. This replaces the typed-path prompt.
2. **Menus.** File, Edit, Insert, View, Format, Build, Help, built from the command registry. A **Layout** menu: editor and preview, editor only, preview only, preview in a separate window (an egui viewport), and a distraction-free mode that hides the menus and the rail.
3. **Keyboard.** `craft-keymap` for every shortcut, user overrides, the shared Keyboard Shortcuts dialog, F6 to cycle regions, and Alt/F10 to reach the menu bar.
4. **Settings dialog**, stored through `craft-settings`:
   - Editor tab: automatic completion, closing brackets, code check, spelling language, key bindings.
   - Build tab: engine mode (retex default, `--compat=lualatex`, `--compat=pdflatex`), main document.
   - Appearance tab: theme, editor colours, font size, font family, line height.
5. **Control channel.** `craft-control` (loopback JSON lines with a token) for `engine.execute`, `engine.commands`, `ui.inspect`, `ui.screenshot`. A `ui_cmd` mirror check in xtask. `craft-web-api` gives the web build `window.texcraft`.
6. **Accessibility.**
   - Announce build results through a live region.
   - Drawn icons for the rail, with names.
   - kittest keyboard-path tests for each panel.
7. **Translations.** Request `texcraft.tsv` in storytold/craft-i18n, pin it with a `craft-i18n.rev` file, and use plural forms (`trn`) for counts.
8. **Release plumbing.**
   - `packaging/` (desktop file, metainfo, icons drawn in code or by a contributor).
   - CI workflows calling `storytold/craft-common-actions`.
   - `craft-update`, which needs a `texcraft` entry in craft-libs' `update/src/suite.rs` (a craft-libs PR).
   - `ATTRIBUTION.md` and `NOTICE` kept by `cargo xtask assets`.
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

### Phase 3: building, preview and navigation

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
6. **Exports retex already offers.** Accessible PDF (PDF/UA-2), SVG, ODT, ODP, HTML and Markdown, each a command with a file dialog.

### Phase 4: the browser

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

### Phase 5: sharing, history and working together

1. **History.** Snapshots on save and on each successful build, a list of versions with dates (`chrono`), a diff view, and restoring a file or the project.
2. **Git.**
   - Opening a project from a repository, committing and pushing, pulling with a three-way merge and a choice per conflict.
   - Built in Rust over the hosts' HTTP APIs, as retex's GitLab integration does, and also over plain git for folders.
3. **Cloud projects and sharing.** `craft-cloud` backends, `craft_ui::share`, accounts through `craft-suite-account` (desktop).
4. **Working together in real time** waits for the suite's concurrent-text contract (RFC-0020's `Texts` API behind a mature CRDT). The editor's transactions (phase 2) and stable marks are designed so that this layer can be added. Presence uses `craft_ui::presence`, and chat belongs to the host.
5. **Review.** Comments anchored to text ranges and suggested edits shown in the margin, accepted or rejected one by one. They are designed on the same marks.

### Phase 6: comfort

- A formatted editing mode: sections, emphasis, lists, figures and citations shown formatted while the source stays the file, built on `craft-latex` and `craft-flow`.
- A start page with recent projects and templates (our own, CC0, generated in the repository).
- Word count (`craft-latex`'s reader).
- Vim and Emacs key bindings.

## 4. Changes to retex

The scaffold uses retex 2.0.0 through its embedding API (`retex::Retex`). These changes make the embedding complete. Each is a retex merge request of its own.

| # | Change | Why TeXCraft needs it |
|---|---|---|
| R1 | The embedding API returns the SyncTeX data, the `symbols` records, the diagnostics' `fixes` and `end_column`, and the files written (`.aux`, `.bbl`), all in memory. Today `embed.rs` drops them. | Source ↔ PDF navigation, language features, quick fixes. |
| R2 | Cancellation: `Engine::check_interrupt` (an empty stub in `error.rs`, already called from the input layer) polls a flag set by `Retex::cancel_token()`, and the pass loop and bib/index runs check it. | A Stop button, and a build replaced by a newer one while the user types. |
| R3 | A progress callback (a closure, not `fn`) with pass, page and file. | A progress line during long builds. |
| R4 | A file-access hook: the host decides which files a build may read and write. The engine, kpathsea, fonts and Lua's `io`/`lfs` go through a trait, following the existing `retex-bib`/`retex-index` `FileAccess` and `retex-mp` `Host` patterns. | RFC-0015: a confined agent session must check every path a document names. Until then, building is refused while confined. Also the base for an in-memory web build. |
| R5 | No process-global state in embedded builds: `set_current_dir`, the `BUILD` mutex, the JSON capture static, `JOB_SETTINGS` and the configuration read from the environment move into a `Session` value. | Parallel builds of several projects; predictable behaviour in a long-running app. |
| R6 | Preamble snapshots for embedded builds (retex roadmap P9), through a child process or a memory image. | Rebuilds while typing at the speed of the command line. |
| R7 | A public SyncTeX reader with forward and inverse queries, generalised from `preflight::Synctex` and the heuristics of `web/editor.js`. Possibly a separate crate. | Navigation (phase 3). |
| R8 | A typed facade over `retex_lsp::Server` (requests and results as Rust types, not `serde_json::Value`). | Simpler, checked use from the editor. |
| R9 | `cfg(target_family = "wasm")` where the code means "no threads or processes" (`retex-pdf` `writer.rs` and `import.rs`, `retex-kpath` `process_id`, `retex-lua` `os.clock`), and an injected clock instead of `Instant::now`. | Pure-Rust pieces (lsp, svg, image) used from wasm32-unknown-unknown without panics. |
| R10 | Publishing or mirroring retex where Craft rule 10 allows git dependencies (storytold), or crates.io. | The current gitlab.com git dependency is an agreed departure from rule 10. |

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
5. **retex as a dependency** (R10).
