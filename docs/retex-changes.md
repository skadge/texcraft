# retex for TeXCraft

> **Last updated:** 2026-11-22 · **Change:** follows retex's agreed design (`docs/design/embedding.md` in retex) instead of TeXCraft's first proposal

TeXCraft asked retex for twelve changes, R1 to R12. retex answered with a design note, `docs/design/embedding.md` in the retex repository. That note is the reference: it lists each change's interface, phase and status. This page records only what TeXCraft does with each retex release, and the points still open on TeXCraft's side.

## What arrives when

| retex release | What it brings (R) | What TeXCraft does with it |
|---|---|---|
| 2.0.0 (done) | crates.io (R10) | `retex = "2"` and `retex-lsp = "2"` from crates.io. |
| 2.1.0 | SyncTeX, symbols, fixes, written files (R1); progress (R3); identity, halt on error, draft images, `clean` (R12); wasm32 gates (R9) | Build menu options become real: draft images, stop at the first error, engine identity in Settings, Delete build files. Progress line while building. Quick fixes in the log and the gutter. The language server gets the symbols record. The SyncTeX bytes are kept for 2.3. |
| 2.2.0 | `retex::Worker` (a `retex worker` child per project); cancelling (R2); preamble snapshots (R6); confinement through `roots` and the OS sandbox (R4); a worker per project (R5); draft PDF as a record | A `Worker` backend in `texcraft-compile` replaces the in-process one for the desktop app. Stop works. Rebuilds while typing start from the snapshot. Unsaved buffers go as the `files` overlay. Agent sessions build in a confined worker, and the refusal to build while confined goes. |
| 2.3.0 | `retex-synctex` (R7); fragments on the worker (R11); typed language-server queries (R8) | Source and PDF navigation (plan phase 4). Typeset formulas for the formatted mode and the equation preview (plan phase 3). Completion, hover and definitions through the typed queries (plan phase 2). |

## How TeXCraft's build backend changes

**Today (2.0).** The retex backend in `crates/compile/src/retex_backend.rs` copies the project into `<cache>/src/` and builds it there with `Retex::compile_file`, keeping build files in `<cache>/out/`.
- Diagnostics name files by their absolute path in that copy, and the backend turns them back into project paths.
- Builds are serialised process-wide and cannot be stopped. The Stop command says so.

**With 2.1.** The same backend, with the new options and outputs. `Outcome` gains:
- `synctex: Option<Arc<[u8]>>` and `symbols: Option<Value>`;
- `fixes` on each `Diagnostic`.

The `Backend` trait does not change.

**With 2.2.** A second backend, `WorkerBackend`, holds one `retex::Worker` per open project:
- **Project folder.** A project that has one is built in place (`BuildRequest::dir`), with unsaved files sent as `overlay`, so nothing is copied.
- **No folder yet.** A new, unsaved project is written to a session folder under the cache first. The worker needs a directory.
- **Progress** feeds the status bar and the preview's progress line. The draft PDF record shows the first pass of a first build in the preview.
- **Stop** calls `BuildHandle::cancel`. If the worker does not answer within a few seconds, the backend kills it and starts a new one, as the note allows.
- **Agent sessions** spawn their worker with a `Sandbox` whose `roots` are the session's read and write roots, the TeX tree and the cache.

The in-process backend stays for the CLI's one-shot `compile` and as the fallback on platforms where a worker cannot start.

## Open points for retex

1. **Fragments in the browser.** R11's fragments are a `Worker` method, and the worker is a native process. TeXCraft's web build runs retex as a `wasm32-wasip1` module (plan phase 5). The formatted mode needs fragments there too. Possible shapes:
   - a `fragments` request handled by the WASI module itself, run in the same Web Worker as builds;
   - or a `retex fragments` entry point that the module exposes next to `retex_resume`.

   Which one fits the snapshot design on WASI?
2. **The worker protocol in the browser.** The JSON request and record protocol of `retex worker` would also suit the Web Worker that hosts the WASI module, with the same records on both platforms. Could the WASI build accept the same `build`, `cancel` and `fragments` requests on stdin (for example through `retex_resume` or a request file), so TeXCraft's host code is one protocol with two transports?
3. **Overlay names.** `files` in a build request names files relative to `dir`. TeXCraft's project paths are the same (relative, `/`-separated), so they can be passed unchanged. Please confirm that overlay names may contain subfolders (`chapters/intro.tex`) and that an overlay file need not exist on disk (a new, unsaved file).
4. **Windows.** Without fork, worker builds on Windows run every pass from the start. TeXCraft will say so in Settings rather than leave the difference unexplained.

## TeXCraft's own part

Some features need no retex change, and TeXCraft builds them itself:
- **SyncTeX until 2.3.** Navigation waits for `retex-synctex`. No interim reader is written.
- **Fixes.** Applying a `Fix` is a `file.edit` on the diagnostic's line, from `column` to `end_column` (characters, from 1).
- **The language server.** Until the typed queries, TeXCraft drives `retex_lsp::Server::handle` with JSON values in process, and uses `retex_lsp::scan` and `lint::file_hints` directly.
