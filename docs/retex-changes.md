# retex for TeXCraft

> **Last updated:** 2026-11-24 · **Change:** retex 2.4.1 in use

TeXCraft asked retex for twelve changes, R1 to R12, and a thirteenth since (R13). retex answered with a design note, `docs/design/embedding.md` in the retex repository. That note is the reference: it lists each change's interface, phase and status. This page records what TeXCraft does with each retex release, and the points still open on TeXCraft's side.

## What arrives when

| retex release | What it brings (R) | What TeXCraft does with it |
|---|---|---|
| 2.0.0 (done) | crates.io (R10) | `retex = "2"` and `retex-lsp = "2"` from crates.io. |
| 2.1.0 (done, in use) | `Retex::synctex`, `symbols`, `on_progress`, `cancel_token`; `compile_file_outcome`, whose `Outcome` keeps a failed build's `Outputs`; `Diagnostic::end_column` and `fixes`; `retex::Worker` and its preamble snapshot; `retex-synctex` (R1 to R3, R6, R7, R9, R12) | **In use:** see "The build backend" below. **Not used yet:** `retex::clean` (below), the worker's `Sandbox` (R4, R5), fragments (R11). |
| 2.2.0 (done, in use) | `Server::start_build`; a `title` for each fix; `retex_image::decode_rgba8`, `retex::render_pdf`; `Retex::spawn` and `spawn_file`; hints for `.bib` files | **In use:** `start_build`, fix titles, `decode_rgba8`, the `.bib` hints (below); `Retex::spawn_file` from 2.3.0. **Not used:** `render_pdf`, as TeXCraft draws PDF figures with its own page renderer (craft-pdf, as the preview does). |
| 2.3.0 (done, in use) | `BuildHandle::canceller`; `Progress::Draft`; SyncTeX: the last paragraph of an included file, preamble lines; PDF/X (`Retex::pdfx`, `bleed_mm`, `crop_marks`, `output_intent`) with its print checks and rules as data (R13); DOCX output (experimental) | **In use:** the canceller (Stop keeps the worker; the in-process backend is `Retex::spawn_file`), the draft PDF in the preview, SyncTeX's fixes (below). **Available, not used yet:** PDF/X, bleed and crop marks, the print checks (`Document::print_checks`, `pdfx_rules`), the accessibility checks as a list (`Document::accessibility_checks`; they already arrive as diagnostics of origin `accessibility`), DOCX export (`Document::to_docx`). |
| 2.4.0 (done, in use) | Typed language-server queries (R8); `retex-fragments`, the fragments' document without I/O, for the browser (R11); the worker's sandbox on macOS and Windows (untried there); PPTX output and better ODT and DOCX output (experimental) | **In use:** completion, hover, definitions and fixes through `Server::completion`, `hover`, `definition` and `code_actions` (below). **Available, not used yet:** `retex-fragments` (the formatted mode's formulas, and in the browser), the sandbox on macOS and Windows, DOCX, PPTX and ODT export. |
| 2.4.1 (done, in use) | Lua-defined OpenType substitutions and ligatures (`addfeature`) in node mode | Builds get it with no change in TeXCraft. |

## The build backend (2.1.0)

**In the desktop app, builds go to `retex::Worker`**, run from TeXCraft's own executable (`retex::worker_entry` first in `main`). Its preamble snapshot makes rebuilds fast: on retex's magazine sample (TikZ, fonts), a rebuild after an edit in the body takes 0.3 to 0.8 s through the worker against about 5 s in process; the first build takes longer (12 s against 5.5 s), as the worker starts and takes the snapshot. The project is still written to `<cache>/src/`, now file by file and only what changed, so the snapshot sees an unchanged preamble as such and the user's folder gets no build files. If the worker cannot start, the build runs in process. The CLI and the tests build in process.



`crates/compile/src/retex_backend.rs` copies the project into `<cache>/src/` and builds it there, keeping build files in `<cache>/out/`. From 2.3.0 both kinds of build are a `retex::BuildHandle`: `Retex::spawn_file` in process, `Worker::build` through the worker, waited on with `wait_with_progress`. Each build sets:

- **`symbols(true)`.** The record goes to the engine's language server after the build (`crates/engine/src/language.rs`), so completion offers the document's commands, environments, labels and citations, and hover describes them. A failed build gives its symbols too.
- **`synctex(true)`.** The engine reads it with `retex-synctex`: a double-click or Ctrl+click in the PDF opens the source line, and Edit › Show in PDF (Cmd+Alt+J; Ctrl+click in the editor where nothing is defined) marks the caret's line in the PDF. Agents get `pdf.source` and `pdf.locate`.
- **Progress** (the handle's `wait_with_progress`). Passes, pages shipped against the last pass's, files read and steps between passes reach the status bar and the PDF pane ("Compiling page 14 of 40 · pass 2 (cross-references)"). The events go from the thread that waits for the build through a channel to the session.
- **The draft PDF** (`Progress::Draft`, 2.3.0): the PDF of a pass that is not the last. While the later passes run, the preview shows it with a "Draft · pass 1" badge (its references may be undefined); the session has it as `Session::draft_pdf` only while its build runs, so once the build ends, built, failed or stopped, the preview shows the last PDF a build wrote. Agents see it in `build.result` (`draft`: the job, the pass, its size).
- **Cancelling** (`BuildHandle::canceller`, 2.3.0). The Compile button's Stop Compiling cancels the running build through its `BuildCanceller`, from the interface's thread while the build thread waits; it ends with TeXCraft's `Status::Cancelled`. Through the worker this sends the worker's `cancel`, so the worker and its preamble snapshot stay for the next build (until 2.3.0, Stop ended the worker). A Stop while the sources are written or the worker starts cancels the build as soon as it has its handle.
- **Build options** (2.1.0): the engine (`identity`: chosen from the document, LuaLaTeX, pdfLaTeX) in Settings › Building, stopping at the first error (`halt_on_error`) and draft images (`draft_images`) there and in the Compile button's menu; agents get `build.options`. A changed option makes the PDF stale, so the automatic build runs again.
- **Delete Build Files** (`build.clean`) deletes TeXCraft's build folder (`<cache>/src/` and `<cache>/out/`) and ends the worker, whose snapshots are build files too; the next build starts from scratch. `retex::clean` is not used: it removes the command line's `.retex/<job>-<hash>/`, while TeXCraft's builds keep theirs in its own cache, and the worker's in `.retex/worker-<job>/`.
- **Diagnostics.** `end_column` gives each problem its exact span in the editor; `fixes` become the quick fixes of build messages (the problem popup, Edit › Quick Fix), applied as one undoable edit, under retex's title for them (2.2.0; "Change to “…”" or "Remove it" for a fix without one).

After each build, the `symbols` record goes to the language server as a build TeXCraft started (`Server::start_build`, 2.2.0), and also answers any build the server asked for. The language server's 2.2.0 hints for `.bib` files (months, dates, DOIs as links, names, duplicate keys) show in the editor like any other hint, since TeXCraft syncs every text file of the project.

Diagnostics name files by their absolute path in the copy, and the backend turns them back into project paths. The `Backend` trait gained `compile_reporting` (progress) and `canceller`; backends that have neither keep the defaults.

**SyncTeX** (2.3.0, nothing to do on TeXCraft's side): a click in the last paragraph of a file read with `\input` goes to that file, not to the line of the including file where TeX ended the paragraph; Edit › Show in PDF from a preamble line finds no place instead of the first output below it.

The editor pane's image preview decodes with `retex_image::decode_rgba8` (PNG, JPEG, GIF, WebP, TIFF, JPEG XL; 2.2.0) and premultiplies the alpha for drawing.

## Next on TeXCraft's side

1. **The rest of the `Worker` backend** (in use, above):
   - **In place.** Building a project in its folder with unsaved files as `overlay` (names with subfolders and files only in the overlay are fine, the note confirms) would save the copy, at the price of build files in the user's folder; for now the copy stays.
   - **Agent sessions** spawn their worker with a `Sandbox` (`Sandbox::new` with the session's write root as a project, its read root in `read`). Building is refused to agents until then. Linux only: elsewhere agents still cannot build.
2. **Fragments** for the formatted mode (plan phase 3), through the worker.
3. **From 2.3.0 and 2.4.0, not used yet:** a PDF/X for the press beside the PDF (`Retex::pdfx`, `bleed_mm`, `crop_marks`, `output_intent`), its print checks as a list placed on the page and in the source (`Document::print_checks`, `pdfx_rules`), and export to DOCX, PPTX (beamer) and ODT (`Document::to_docx`, `to_pptx`; experimental).

## Open points for retex

### From the 2.1.0 integration

Points 1 to 4 are answered by 2.2.0 (retex's `docs/design/embedding.md`, "For hosts: API changes in 2.2.0"); 1 to 3 are in use. The two below are answered by 2.3.0 ("For hosts: API changes in 2.3.0") and in use: none is open.
- **SyncTeX** (closed): inverse search takes the file most of a line's glue and kerns come from, and a preamble line gives no place. Nothing to change in TeXCraft.
- **Cancelling** (closed): `BuildHandle::canceller() -> BuildCanceller` (`Clone + Send + Sync`). Stop keeps the worker and its preamble snapshot, and the in-process backend is `Retex::spawn_file`, so both backends are one `BuildHandle` (see "The build backend").

As they were raised:

1. **SyncTeX: the last paragraph of an included file** (answered by 2.3.0). `Synctex::inverse` takes the file of the line box under the point, and a paragraph's line boxes carry the line where it ended. The last paragraph of a file read with `\input` usually ends in the including file (at its next blank line, `\section` or `\end{document}`), so a click in it goes to that line of the including file, not to the paragraph's own file. Seen with TeXCraft's `examples/article` (the last paragraph of `sections/intro.tex` goes to `main.tex` line 18) and in `crates/engine/tests/fixtures/subfolder.synctex`. Could inverse search prefer the file of the glyphs, glue and kerns nearest the point when it differs from the box's? Forward search from a preamble line also finds the first output up to 40 lines below (`\documentclass` marks equation (1) of the article); for a line before `\begin{document}`, no place might be better.

2. **Cancelling a build being waited on** (answered by 2.3.0). `BuildHandle::cancel` takes `&self`, and `wait_with_progress` takes the handle by value, so the thread that waits cannot hand a way to cancel to the thread that wants to (the interface's Stop). With the worker, TeXCraft ends the worker instead (dropping `Worker`): that stops the build but loses the preamble snapshot, so the next build starts a new worker. It is also why the in-process backend does not use `Retex::spawn` (2.2.0): the handle's cancel replaces the options' `cancel_token`, so Stop could not reach a spawned build, and the backend keeps `compile_file_outcome` on a thread of its own with a token it holds. A cancel handle that can be cloned and sent (`BuildHandle::canceller() -> BuildCanceller`, `Send + Sync`) would keep the worker and its snapshot, and make both backends one `BuildHandle`.

### Answered in retex's note after 2.1.0

The note's "TeXCraft's open points after 2.1.0" settles these; they are left here only as what TeXCraft waits for:
- `retex-synctex` is in 2.1.0 (in use). A worker stops a cancelled build within 4 s by itself.
- Overlay names may have subfolders, a file may exist only in the overlay, the main file may be overlaid; overlays are text only.
- **For a later release:** the draft PDF of a first build's first pass (in 2.3.0, as the draft of every pass that is not the last; in use); fragments in the browser, from a crate shared by `Worker::fragments` and a web host (no new entry point in the WASI module; `retex-fragments`, in 2.4.0, not used yet); PDF/X, bleed and crop marks in the embedding API with the print and accessibility checks as data (R13; in 2.3.0, not used yet).
- **The protocol in the browser** stays on TeXCraft's side: a host library maps `build` to an instantiation of the WASI module and `cancel` to ending its Web Worker.
- **Windows and macOS:** worker builds run every pass from the start on Windows (no fork). From 2.4.0 the sandbox exists on macOS (Seatbelt) and Windows (AppContainer) too, not yet tried on those systems; with downloads allowed, the Windows one is `FilesOnly`. TeXCraft will say so in Settings.

## TeXCraft's own part

Some features need no retex change, and TeXCraft builds them itself:
- **Fixes.** A build fix is an edit on the diagnostic's line, from `column` to `end_column` (characters, from 1); a language-server fix is a code action's edits. Both are applied to the editor's buffer as one undoable edit, and agents get `language.fixes` and `language.fix`.
- **The language server.** TeXCraft runs `retex_lsp::Server` in process, in UTF-32 positions (the editor's character columns). Documents are opened, changed and closed with `Server::handle` (JSON-RPC notifications), and the lint arrives in its `publishDiagnostics`; completion, hover, definitions and fixes are typed queries (2.4.0). A fix names its problem by the lint's message at the same range, which carries the help lines that the typed diagnostic's bare `message` lacks, since the editor matches a fix to the problem it shows by that message.
