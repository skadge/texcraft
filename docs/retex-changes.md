# retex for TeXCraft

> **Last updated:** 2026-11-22 · **Change:** TeXCraft builds with retex 2.1.0; what it uses, what it does not use yet, and what the integration asks of retex

TeXCraft asked retex for twelve changes, R1 to R12, and a thirteenth since (R13). retex answered with a design note, `docs/design/embedding.md` in the retex repository. That note is the reference: it lists each change's interface, phase and status. This page records what TeXCraft does with each retex release, and the points still open on TeXCraft's side.

## What arrives when

| retex release | What it brings (R) | What TeXCraft does with it |
|---|---|---|
| 2.0.0 (done) | crates.io (R10) | `retex = "2"` and `retex-lsp = "2"` from crates.io. |
| 2.1.0 (done, in use) | `Retex::synctex`, `symbols`, `on_progress`, `cancel_token`; `compile_file_outcome`, whose `Outcome` keeps a failed build's `Outputs`; `Diagnostic::end_column` and `fixes` (R1, R3, R9, R12) | **In use:** see "The in-process backend" below. **Not used yet:** `identity`, `halt_on_error`, `draft_images`, `retex::clean`, `retex::Worker` (R2, R4 to R6), `retex-synctex`'s reader (R7), fragments (R11). |
| 2.2.0 | Typed language-server queries (R8) | Completion, hover and definitions through them. Until then, `Server::handle` with JSON values. |

## The in-process backend (2.1.0)

`crates/compile/src/retex_backend.rs` copies the project into `<cache>/src/` and builds it there with `Retex::compile_file_outcome`, keeping build files in `<cache>/out/`. Each build sets:

- **`symbols(true)`.** The record goes to the engine's language server after the build (`crates/engine/src/language.rs`), so completion offers the document's commands, environments, labels and citations, and hover describes them. A failed build gives its symbols too.
- **`synctex(true)`.** The engine reads it with `retex-synctex`: a double-click or Ctrl+click in the PDF opens the source line, and View › Show in PDF (Cmd+Alt+J; Ctrl+click in the editor where nothing is defined) marks the caret's line in the PDF. Agents get `pdf.source` and `pdf.locate`.
- **`on_progress`.** Passes, pages shipped against the last pass's, files read and steps between passes reach the status bar and the PDF pane ("Compiling page 14 of 40 · pass 2 (cross-references)"). retex calls back on its build thread; the backend forwards the events through a channel to the build worker's thread, which passes them to the session.
- **`cancel_token`.** Build › Stop Compiling cancels the running build; it ends with TeXCraft's `Status::Cancelled`.
- **Diagnostics.** `end_column` gives each problem its exact span in the editor; `fixes` become the quick fixes of build messages (the problem popup, Edit › Quick Fix), applied as one undoable edit.

Diagnostics name files by their absolute path in the copy, and the backend turns them back into project paths. The `Backend` trait gained `compile_reporting` (progress) and `canceller`; backends that have neither keep the defaults.

The editor pane's image preview uses `retex-image` (PNG, GIF, WebP, TIFF, JPEG XL), with JPEG through `zune-jpeg`, the decoder the PDF renderer already uses.

## Next on TeXCraft's side

1. **The rest of 2.1.0's options.** The Build menu's draft images (`draft_images`) and stop at the first error (`halt_on_error`), the engine identity in Settings (`identity`), and Delete Build Files (`retex::clean` with the main document).
2. **The `Worker` backend** for the desktop app, replacing the copy into `<cache>/src/`:
   - **Project folder.** A project that has one is built in place (`BuildRequest::dir`), with unsaved files sent as `overlay`, so nothing is copied.
   - **No folder yet.** A new, unsaved project is written to a session folder under the cache first: the worker needs a directory.
   - **Progress** comes from its records; the draft PDF record shows the first pass of a first build in the preview.
   - **Stop** calls `BuildHandle::cancel`; if the worker does not answer within a few seconds, the backend kills it and starts a new one, as the note allows.
   - **Agent sessions** spawn their worker with a `Sandbox` whose `roots` are the session's read and write roots, the TeX tree and the cache. Building is refused to agents until then.

   The in-process backend stays for the CLI's one-shot `compile` and where a worker cannot start.
3. **Fragments** for the formatted mode (plan phase 3), through the worker.

## Open points for retex

### From the 2.1.0 integration

1. **Builds the host starts.** `retex_lsp::Server` takes a build's records (`Input::Started`, `Records`, `Finished`) only for a build it asked for (`Output::Build`). An editor builds on its own schedule (after typing pauses, on Compile), so TeXCraft announces each of its builds as the main file being saved (`textDocument/didSave`), takes the build id the server asks for, and answers it with the finished build's records. An input for a build the host started (`Input::Built { main, dir, records }`, or `Started` with an id the host chooses) would remove the detour.
2. **A title for a fix.** `retex::Fix` has a span and a replacement but no description. TeXCraft titles build fixes "Change to “…”" or "Remove it", as retex-lsp titles its own. A short title or a reason from retex ("Use \textbf", "Close the group") would say more, in the build messages' own words.
3. **Pixels for hosts from `retex-image`.** An editor shows a project's images (the editor pane's preview, thumbnails). `retex-image` decodes PNG, GIF, WebP, TIFF and JPEG XL into a `Raster` (grey, RGB or CMYK, 8 or 16 bits, alpha apart), which TeXCraft turns into premultiplied RGBA8 itself, and has no JPEG decoder, since retex embeds JPEG as it is. Would a host-facing `retex_image::decode_rgba8(data) -> Result<(u32, u32, Vec<u8>)>`, covering JPEG too (and the first page of a PDF, rendered), belong in `retex-image`? TeXCraft would drop its conversion and its direct `zune-jpeg` dependency.
4. **Progress from a blocking call.** `compile_file_outcome` blocks and `on_progress` runs on retex's build thread, so a host whose build thread also owns the progress sink starts a thread for the call and pumps a channel. A `Retex::spawn(...) -> BuildHandle` in process (as the worker has), with `wait_with_progress`, would make the in-process and worker backends one shape.

5. **SyncTeX: the last paragraph of an included file.** `Synctex::inverse` takes the file of the line box under the point, and a paragraph's line boxes carry the line where it ended. The last paragraph of a file read with `\input` usually ends in the including file (at its next blank line, `\section` or `\end{document}`), so a click in it goes to that line of the including file, not to the paragraph's own file. Seen with TeXCraft's `examples/article` (the last paragraph of `sections/intro.tex` goes to `main.tex` line 18) and in `crates/engine/tests/fixtures/subfolder.synctex`. Could inverse search prefer the file of the glyphs, glue and kerns nearest the point when it differs from the box's? Forward search from a preamble line also finds the first output up to 40 lines below (`\documentclass` marks equation (1) of the article); for a line before `\begin{document}`, no place might be better.

### Still open from before

6. **Fragments in the browser.** R11's fragments are a `Worker` method, and the worker is a native process. TeXCraft's web build runs retex as a `wasm32-wasip1` module (plan phase 6). The formatted mode needs fragments there too. Possible shapes:
   - a `fragments` request handled by the WASI module itself, run in the same Web Worker as builds;
   - or a `retex fragments` entry point that the module exposes next to `retex_resume`.

   Which one fits the snapshot design on WASI?
7. **The worker protocol in the browser.** The JSON request and record protocol of `retex worker` would also suit the Web Worker that hosts the WASI module, with the same records on both platforms. Could the WASI build accept the same `build`, `cancel` and `fragments` requests on stdin (for example through `retex_resume` or a request file), so TeXCraft's host code is one protocol with two transports?
8. **Overlay names.** `files` in a build request names files relative to `dir`. TeXCraft's project paths are the same (relative, `/`-separated), so they can be passed unchanged. Please confirm that overlay names may contain subfolders (`chapters/intro.tex`) and that an overlay file need not exist on disk (a new, unsaved file).
9. **PDF/X and preflight in the embedding API (R13).** `-output-format=pdf-x4` and `pdf-x1a`, `--bleed`, `--crop-marks` and the GWG 2022 checks are on the command line only; `Retex` has `accessible` but no PDF/X. TeXCraft's print support (plan phase 5) needs:
   - options for the PDF/X flavour, the bleed and crop marks;
   - the PDF/X file next to the screen PDF from one build, as the command line writes both;
   - the preflight findings as data: kind, severity, page, the area on the page in PDF points, the measured value and the limit, and the image's file name when there is one. With these TeXCraft can list them, mark them on the page, and open their source line through SyncTeX.

   The accessibility checks of `--accessible` as data, in the same shape, would serve the accessible-PDF findings.
10. **Windows.** Without fork, worker builds on Windows run every pass from the start. TeXCraft will say so in Settings rather than leave the difference unexplained.

## TeXCraft's own part

Some features need no retex change, and TeXCraft builds them itself:
- **Fixes.** A build fix is an edit on the diagnostic's line, from `column` to `end_column` (characters, from 1); a language-server fix is a code action's edits. Both are applied to the editor's buffer as one undoable edit, and agents get `language.fixes` and `language.fix`.
- **The language server.** Until the typed queries, TeXCraft drives `retex_lsp::Server::handle` with JSON values in process, in UTF-32 positions (the editor's character columns).
