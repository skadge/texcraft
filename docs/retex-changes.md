# retex for TeXCraft

> **Last updated:** 2026-11-22 · **Change:** retex's API changes for hosts in 2.1.0; the worker, SyncTeX and fragments now also target 2.1.0

TeXCraft asked retex for twelve changes, R1 to R12. retex answered with a design note, `docs/design/embedding.md` in the retex repository. That note is the reference: it lists each change's interface, phase and status. This page records only what TeXCraft does with each retex release, and the points still open on TeXCraft's side.

## What arrives when

retex's design note opens with "For hosts: API changes in 2.1.0", the list to work from. As of this writing 2.1.0 is not released (crates.io has 2.0.0); TeXCraft moves to it when it is.

| retex release | What it brings (R) | What TeXCraft does with it |
|---|---|---|
| 2.0.0 (done) | crates.io (R10) | `retex = "2"` and `retex-lsp = "2"` from crates.io. |
| 2.1.0, in process (done in retex's `main`) | `Retex::synctex`, `symbols`, `identity`, `halt_on_error`, `draft_images`, `on_progress`; `compile_outcome` and `compile_file_outcome`, whose `Outcome` keeps a failed build's `Outputs` (SyncTeX, symbols, written files); `Diagnostic::end_column` and `fixes`; `retex::clean(main, job)`; the browser crates on wasm32 (R1, R3, R9, R12) | The backend calls `compile_file_outcome`, so a failed build still feeds the language server and navigation. The Build menu's draft images and stop-at-the-first-error, the engine identity in Settings and Delete build files become real. Progress goes to the status bar. Quick fixes appear in the log and the gutter. |
| 2.1.0, planned | `retex::Worker` (R2, R4, R5, R6), the SyncTeX reader `retex-synctex` (R7), fragments on the worker (R11) | The `Worker` backend for the desktop app (below). Stop works. Rebuilds start from the preamble snapshot. Agent sessions build in a confined worker. Source and PDF navigation (plan phase 4). Typeset formulas for the formatted mode (plan phase 3). |
| 2.2.0 | Typed language-server queries (R8) | Completion, hover and definitions through them (plan phase 2). Until then, `Server::handle` with JSON values. |

## How TeXCraft's build backend changes

**Today (2.0).** The retex backend in `crates/compile/src/retex_backend.rs` copies the project into `<cache>/src/` and builds it there with `Retex::compile_file`, keeping build files in `<cache>/out/`.
- Diagnostics name files by their absolute path in that copy, and the backend turns them back into project paths.
- Builds are serialised process-wide and cannot be stopped. The Stop command says so.

**With 2.1's in-process API.** The same backend, building with `compile_file_outcome` and the new options. TeXCraft's `Outcome` gains:
- `synctex: Option<Arc<[u8]>>` and `symbols: Option<Value>`, also for failed builds;
- `fixes` and `end_column` on each `Diagnostic`.

Delete build files calls `retex::clean` with the main document. The `Backend` trait does not change.

**With 2.1's worker.** A second backend, `WorkerBackend`, holds one `retex::Worker` per open project:
- **Project folder.** A project that has one is built in place (`BuildRequest::dir`), with unsaved files sent as `overlay`, so nothing is copied.
- **No folder yet.** A new, unsaved project is written to a session folder under the cache first. The worker needs a directory.
- **Progress** feeds the status bar and the preview's progress line. The draft PDF record shows the first pass of a first build in the preview.
- **Stop** calls `BuildHandle::cancel`. If the worker does not answer within a few seconds, the backend kills it and starts a new one, as the note allows.
- **Agent sessions** spawn their worker with a `Sandbox` whose `roots` are the session's read and write roots, the TeX tree and the cache.

The in-process backend stays for the CLI's one-shot `compile` and as the fallback on platforms where a worker cannot start.

## Open points for retex

1. **Fragments in the browser.** R11's fragments are a `Worker` method, and the worker is a native process. TeXCraft's web build runs retex as a `wasm32-wasip1` module (plan phase 6). The formatted mode needs fragments there too. Possible shapes:
   - a `fragments` request handled by the WASI module itself, run in the same Web Worker as builds;
   - or a `retex fragments` entry point that the module exposes next to `retex_resume`.

   Which one fits the snapshot design on WASI?
2. **The worker protocol in the browser.** The JSON request and record protocol of `retex worker` would also suit the Web Worker that hosts the WASI module, with the same records on both platforms. Could the WASI build accept the same `build`, `cancel` and `fragments` requests on stdin (for example through `retex_resume` or a request file), so TeXCraft's host code is one protocol with two transports?
3. **Overlay names.** `files` in a build request names files relative to `dir`. TeXCraft's project paths are the same (relative, `/`-separated), so they can be passed unchanged. Please confirm that overlay names may contain subfolders (`chapters/intro.tex`) and that an overlay file need not exist on disk (a new, unsaved file).
4. **PDF/X and preflight in the embedding API (R13).** `-output-format=pdf-x4` and `pdf-x1a`, `--bleed`, `--crop-marks` and the GWG 2022 checks are on the command line only; `Retex` has `accessible` but no PDF/X. TeXCraft's print support (plan phase 5) needs:
   - options for the PDF/X flavour, the bleed and crop marks;
   - the PDF/X file next to the screen PDF from one build, as the command line writes both;
   - the preflight findings as data: kind, severity, page, the area on the page in PDF points, the measured value and the limit, and the image's file name when there is one. With these TeXCraft can list them, mark them on the page, and open their source line through SyncTeX.

   The accessibility checks of `--accessible` as data, in the same shape, would serve the accessible-PDF findings.
5. **Windows.** Without fork, worker builds on Windows run every pass from the start. TeXCraft will say so in Settings rather than leave the difference unexplained.

## TeXCraft's own part

Some features need no retex change, and TeXCraft builds them itself:
- **SyncTeX until 2.3.** Navigation waits for `retex-synctex`. No interim reader is written.
- **Fixes.** Applying a `Fix` is a `file.edit` on the diagnostic's line, from `column` to `end_column` (characters, from 1).
- **The language server.** Until the typed queries, TeXCraft drives `retex_lsp::Server::handle` with JSON values in process, and uses `retex_lsp::scan` and `lint::file_hints` directly.
