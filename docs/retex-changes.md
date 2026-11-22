# Changes TeXCraft needs in retex

> **Last updated:** 2026-11-22 · **Change:** first version, against retex 2.0.0 (`93509ea`)

TeXCraft links retex as a library through its embedding API, `retex::Retex` (`crates/retex-cli/src/embed.rs`), and uses `retex-lsp` directly. This document lists the changes that would make that embedding complete. Each section is meant as one retex merge request: the problem as TeXCraft sees it, the current code, a proposed interface, and how to tell it is done. The interfaces are proposals. Where retex's own design notes point another way, retex's design wins and TeXCraft adapts.

Priorities:
- **P1** blocks a planned TeXCraft feature.
- **P2** makes an existing feature correct or fast.
- **P3** is cleanup.

| # | Change | Priority | TeXCraft feature that waits for it |
|---|---|---|---|
| R1 | Build outputs in memory: SyncTeX, symbols, fixes, written files | P1 | Source and PDF navigation, completion, quick fixes |
| R2 | Cancelling a build | P1 | Stop button; restarting a build while typing |
| R3 | Progress as a closure | P2 | Progress line, early draft PDF |
| R4 | Host-controlled file access | P1 | Agent builds (RFC-0015), in-memory projects |
| R5 | No process-global state in embedded builds | P2 | Several projects open; long-running app |
| R6 | Preamble snapshots for embedded builds | P2 | Fast rebuilds while typing |
| R7 | Public SyncTeX reader | P1 | Source and PDF navigation |
| R8 | Typed facade for retex-lsp | P3 | Simpler editor integration |
| R9 | Pure-Rust crates usable on wasm32-unknown-unknown | P2 | Web build (language server, PDF tools) |
| R10 | A dependency source the Craft rules accept | P3 | Joining the Craft suite |
| R11 | Typesetting fragments (formulas, tables) to images | P1 | Formatted editing mode, equation preview |
| R12 | Build options in the embedding API | P2 | Build menu (draft, stop on first error, engine mode) |

## R1. Build outputs in memory

**Problem.** `Document` (embed.rs:48) holds the PDF, page and pass counts, time, log and diagnostics. The builds can also produce the following, but the embedding API does not return them:
- The SyncTeX data. The build is not asked for it: no `-synctex` in the argv built in `build_in`.
- The `symbols` record that `--symbols` writes for the language server.
- Each diagnostic's `fixes` and `end_column`. They exist in `retex_diag::Diagnostic` and in the JSON record, but the embed `Diagnostic` (embed.rs:203) drops them.
- The auxiliary files written (`.aux`, `.bbl`, `.toc`). The language server and TeXCraft's outline of labels need these.

**Proposal.**

```rust
impl Retex {
    pub fn synctex(self, on: bool) -> Retex;   // default off; on = uncompressed (-synctex=-1)
    pub fn symbols(self, on: bool) -> Retex;   // default off
}
impl Document {
    pub fn synctex(&self) -> Option<&[u8]>;
    pub fn symbols(&self) -> Option<&serde_json::Value>;   // the `symbols` record, as the LSP reads it
    pub fn written(&self) -> &[(String, Vec<u8>)];          // files the build wrote, by name relative to the build dir
}
pub struct Diagnostic { /* existing fields */ pub end_column: Option<u32>, pub fixes: Vec<Fix> }
pub struct Fix { pub start: (u32, u32), pub end: (u32, u32), pub replacement: String }   // as retex_diag::Fix
```

The same data should also come with `Error::Failed`, because a build with errors still has symbols and partial SyncTeX.

**Done when** a build with `.synctex(true).symbols(true)` returns both, and the `fixes` match the JSON records of the same build on the command line.

## R2. Cancelling a build

**Problem.** `Engine::check_interrupt` (crates/retex-engine/src/error.rs:866) is an empty stub with the comment "retex does not support them". It is already called from the input layer (input.rs:502 and 521). A host cannot stop a build: TeXCraft's Stop command is disabled with a reason, and a build started while typing must run to its end before the next one starts.

**Proposal.**

```rust
#[derive(Clone)] pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken { pub fn new() -> Self; pub fn cancel(&self); pub fn is_cancelled(&self) -> bool; }
impl Retex { pub fn cancel_token(self, token: CancelToken) -> Retex; }
pub enum Error { /* … */ Cancelled }
```

The behaviour wanted:
- `check_interrupt` reads the flag and, when it is set, ends the run with a fatal error, the same way TeX's own interrupt does (a `JumpOut`).
- The driver checks the flag between passes and before the BibTeX/biber and makeindex runs.
- A cancelled build returns `Error::Cancelled` and leaves the build directory usable for the next build. In particular, a half-written `.aux` must not be read as complete. Either keep the previous pass's files, or mark the directory as needing a full rebuild.

**Done when** cancelling a long build (the pgf manual) returns within 100 ms, and the next build of the same directory gives the same PDF as a build from a clean directory.

## R3. Progress as a closure

**Problem.** `Engine::progress_hook: Option<fn(&mut Engine)>` (retex-engine/src/lib.rs:693) is a plain function pointer, so a host cannot capture a channel. The embedding API offers no progress at all.

**Proposal.**

```rust
pub enum Progress { Pass { n: u32, of: Option<u32> }, Page { n: u32 }, File { name: String }, Bibliography, Index, DraftPdf(Vec<u8>) }
impl Retex { pub fn on_progress(self, f: impl FnMut(Progress) + Send + 'static) -> Retex; }
```

`DraftPdf` is the PDF after the first pass of a first build. retex's web editor already shows it early. It is optional and depends on R1's in-memory output. The rate limit the JSON `progress` record uses (at most every 100 ms) should apply here too.

## R4. Host-controlled file access

**Problem.** The engine reads with `std::fs` in about 43 places:
- `files.rs` (`read_file_recorded`, `RecordingWriter`)
- `misc.rs` (`\openout`), `shipout.rs`, `synctex.rs`, `figures.rs`, `image.rs`, `the.rs`, `pdfext.rs`, `clock.rs`
- kpathsea (`texmf.cnf`, `ls-R`, directory scans), the font loader, and Lua's `io` and `lfs`

TeXCraft needs control over this for two reasons:
1. **Confined agent sessions** (Craft RFC-0015, AGENTS.md rule 18). Every path a document names (`\input{/etc/passwd}`, `\includegraphics{../../secret.png}`, `\openout`) must be checked against the session's roots and refused outside them. TeXCraft currently refuses to build at all while confined.
2. **In-memory projects.** On the web, and later for unsaved buffers on the desktop, the project files live in memory. Today they must be written to a temporary directory first.

**Proposal.** A file-access trait at the level retex already uses in `retex-bib` (`FileAccess`), `retex-index` and `retex-mp` (`files::Host`):

```rust
pub trait Files: Send {
    fn read(&self, path: &Path, kind: FileKind) -> io::Result<Vec<u8>>;   // or a reader, for large fonts
    fn metadata(&self, path: &Path) -> io::Result<Meta>;                     // len, mtime
    fn create(&self, path: &Path) -> io::Result<Box<dyn Write + Send>>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;            // kpathsea scans
    fn remove(&self, path: &Path) -> io::Result<()>;
}
pub enum FileKind { Document, TexTree, Font, Image, Aux, Output, Lua }
impl Retex { pub fn files(self, f: impl Files + 'static) -> Retex; }   // default: the real file system
```

Requirements:
- **The TeX tree stays fast.** `docs/design/in-memory.md` recommends against an in-memory mode for performance. The trait need not put the tree in memory. A host may answer `TexTree` and `Font` reads from the real file system unchecked, and check only `Document`, `Image`, `Aux` and `Output` paths. TeXCraft would confine document paths and let the tree be read as usual.
- **Lua's `io` and `lfs`.** These go through C stdio today. TeXCraft needs at least a guarantee that they cannot open document-named paths outside the host's check, for example by replacing `io.open` and the `lfs` functions with Rust ones that call the trait.
- **Shell escape** stays off in embedded builds unless asked for, as now. Confined sessions never ask.
- **The `MissHook`** (kpathsea) returns bytes or a path from the trait, so a lazily fetched tree file need not be written to disk first.

**Done when** a build with a `Files` implementation that refuses everything outside one folder:
- builds the TeXCraft example project;
- refuses `\input{/etc/hostname}` with an error diagnostic that names the path;
- passes the escape table in TeXCraft's `crates/engine/tests/it/file_access.rs` when that table is applied to document paths.

## R5. No process-global state in embedded builds

**Problem.** An embedded build:
- changes the process's current directory (`std::env::set_current_dir`, embed.rs:460 and 544);
- serialises all builds through `static BUILD: Mutex<()>` (embed.rs:30);
- captures JSON records in a static (`json::start_capture`);
- reads settings set once per process (`JOB_SETTINGS: OnceLock`, lib.rs:332).

Configuration also comes from environment variables (`RETEX_CACHE`, `TEXMFCNF`, `RETEX_NO_SNAPSHOT`, `RETEX_OFFLINE`) and from texlive `OnceLock`s.

For TeXCraft, this means:
- Two open projects cannot build at the same time.
- A change of the current directory can surprise other threads (file dialogs, relative paths in logs).
- Settings chosen in the app (the cache folder, the TeX Live year, offline mode) cannot be passed per build.

**Proposal.** A `Session` (or `Engine`-level configuration) value that owns the cache folder, the kpathsea instance, the loaded format, the TeX Live selection and the policy (offline, downloads), passed to each build:

```rust
let session = retex::Session::new(retex::Config { cache_dir, texlive: Selection::Auto, downloads: Downloads::Ask, .. })?;
let doc = session.build(Retex::new().file(…)).compile_file(main)?;   // builds of different sessions may run in parallel
```

Paths are made absolute against the job's directory instead of the current directory. Builds of the same session may stay serialised.

## R6. Preamble snapshots for embedded builds

**Problem.** The command line forks after the preamble (`snapshot.rs`) and on WASI keeps a memory image (`snapshot_wasm.rs`). Embedded builds do neither (retex roadmap P9). A rebuild while typing therefore re-reads the preamble every time: 20 to 55 % more time per pass on the documents measured in the roadmap.

**Proposal.** Either of these:
- An embedded build runs the engine in a child `retex` process that keeps its fork-based snapshot between builds, with a request and response protocol over pipes. The JSON records already exist, and this also isolates crashes.
- An in-process snapshot of engine and Lua state. This is harder because of the Lua state.

TeXCraft prefers the first. It keeps the app's address space free of a 256 MB engine stack and lets the app kill a stuck build (R2 becomes a process kill as a last resort). It is not available on the web, where the WASI memory image is used instead (TeXCraft plan, phase 5).

**Done when** the second build of an unchanged-preamble article through the embedding API takes the same time as `retex --watch` does for it.

## R7. Public SyncTeX reader

**Problem.** SyncTeX is written by `retex-engine/src/synctex.rs`. Readers exist only in private code:
- `retex-cli/src/preflight.rs` (`struct Synctex`, inverse lookup);
- `web/editor.js` (`parseSyncTeX`, `syncFromPdf`, `syncToPdf`, with heuristics: line boxes, nearest leaf, a penalty for tall boxes, ±40 lines).

TeXCraft needs both directions.

**Proposal.** A crate `retex-synctex` (pure Rust, no I/O):

```rust
pub struct Synctex { /* parsed */ }
impl Synctex {
    pub fn parse(bytes: &[u8]) -> Result<Synctex, Error>;   // compressed or not
    /// Source → PDF: the boxes to mark for a line (and column, when known), page from 0, in PDF points.
    pub fn forward(&self, file: &str, line: u32, column: Option<u32>) -> Vec<(u32, Rect)>;
    /// PDF → source: the best place for a point on a page.
    pub fn inverse(&self, page: u32, x: f32, y: f32) -> Option<Place>;   // Place { file, line, column: Option<u32> }
}
```

It should carry the heuristics of `editor.js`, tested on the corpus. File names should be returned as written in the document (relative to the main file's folder), as `Diagnostic::file` is.

**Done when** forward and inverse round-trip, giving the same line or a line in the same paragraph, on the 640 corpus documents identical to lualatex.

## R8. Typed facade for retex-lsp

**Problem.** `retex_lsp::Server::handle` takes and returns LSP-shaped `serde_json::Value`s, and the feature functions (`hover`, `completion`, `document_symbols`) are `pub(crate)`. TeXCraft calls it in process, so it would build JSON-RPC requests only to parse the answers again.

**Proposal.** Public typed methods next to `handle`, keeping `handle` for LSP clients:

```rust
impl Server {
    pub fn open(&mut self, uri: &str, text: &str); pub fn change(&mut self, uri: &str, edits: &[TextEdit]); pub fn close(&mut self, uri: &str);
    pub fn completion(&self, uri: &str, pos: Position) -> Vec<CompletionItem>;
    pub fn hover(&self, uri: &str, pos: Position) -> Option<Hover>;
    pub fn definition(&self, uri: &str, pos: Position) -> Vec<Location>;
    pub fn symbols(&self, uri: &str) -> Vec<DocumentSymbol>;
    pub fn diagnostics(&self, uri: &str) -> Vec<retex_diag::Diagnostic>;
    pub fn build_records(&mut self, build: BuildId, records: &[serde_json::Value]);   // or typed, after R1
}
```

`lint::hints` reads with `std::fs`. Nothing changes for TeXCraft, which uses `lint::file_hints` with text it holds. Document that `file_hints` is the in-memory entry point.

## R9. Pure-Rust crates usable on wasm32-unknown-unknown

**Problem.** Several gates use `cfg(target_os = "wasi")` where they mean "no threads or processes". On wasm32-unknown-unknown those code paths take the native branch and panic. TeXCraft's web build links the pure-Rust crates (lsp, svg, image, highlight, diag) directly. Only the engine runs as a separate WASI module.

**Proposal.**
- Use `cfg(target_family = "wasm")` in:
  - `retex-pdf/src/writer.rs:396` (compression thread);
  - `retex-pdf/src/import.rs:257`;
  - `retex-kpath/src/lib.rs:22` (`process_id`);
  - `retex-lua/src/extras.rs:192` (`os.clock`).
- Guard the profiler thread (`retex-engine/src/profile.rs:186`) and `Document::render_all`'s `thread::scope` the same way.
- Replace `Instant::now` and `SystemTime::now` in crates TeXCraft links on the web with an injected clock or `web-time`.
- Add a CI job that runs `cargo check --target wasm32-unknown-unknown` for retex-lsp, retex-diag, retex-highlight, retex-svg and retex-image.

## R10. A dependency source the Craft rules accept

**Problem.** Craft rule 10 expects git dependencies from storytold only. TeXCraft takes `retex` and `retex-lsp` from gitlab.com at a pinned rev, a departure recorded in TeXCraft's AGENTS.md.

**Options.**
- Publish the library crates on crates.io. The CC0 licence is on the Craft allowlist.
- Or mirror retex under storytold.

Either way, a tagged release that TeXCraft pins.

## R11. Typesetting fragments to images

**Problem.** TeXCraft's formatted editing mode (plan, phase 3) and the equation preview in the source editor show formulas, and later small tables and TikZ pictures, typeset as they will be in the PDF. Building the whole document for each formula is far too slow while typing. TeXCraft's own math layout would differ from TeX's output in spacing and fonts.

**Proposal.** A fragment build: one document preamble and a list of fragments, each typeset in its own box, returned as images with their metrics. It works like the `preview` package, run in process:

```rust
pub struct Fragment { pub id: u64, pub source: String, pub mode: FragmentMode }   // InlineMath, DisplayMath, Text { width_pt }
pub struct Typeset { pub id: u64, pub pdf_page: u32, pub width_pt: f32, pub height_pt: f32, pub depth_pt: f32, pub diagnostics: Vec<Diagnostic> }
impl Retex {
    /// Typesets each fragment with `preamble` (the document's, up to \begin{document}),
    /// one fragment per page of the returned PDF, with its box's width, height and depth.
    pub fn fragments(&self, preamble: &str, fragments: &[Fragment]) -> Result<(Document, Vec<Typeset>), Error>;
}
```

Requirements:
- **The preamble is loaded once per session and reused.** With R6, a fragment batch after an unchanged preamble should cost tens of milliseconds, not a full build.
- **A failing fragment does not fail the batch.** It returns its diagnostics and an empty box, and the others are typeset.
- **The depth is returned**, so inline formulas sit on the text baseline in the formatted view.
- **The same call works in the WASI build**, since the web app needs it too.

TeXCraft draws the returned pages with craft-pdf and caches them by the fragment source and preamble hash.

**Done when** a batch of 50 display formulas after a typical article preamble returns in under 200 ms natively once the preamble is warm, and each image's baseline matches the formula's position in the full document's PDF.

## R12. Build options in the embedding API

**Problem.** Several options the command line has are not reachable through `Retex`. TeXCraft's Build menu offers them:
- the engine identity (`--compat=lualatex`, `--compat=pdflatex`, or retex's own choice);
- stop at the first error (`-halt-on-error`);
- draft builds, where images are drawn as boxes (an option at the `graphicx` level, as the reference editors offer, distinct from `-draftmode`, which writes no PDF);
- clearing the build files (`.retex/<job>-<hash>/` and the cache entries of a job).

**Proposal.**

```rust
pub enum Identity { Auto, LuaLatex, PdfLatex }
impl Retex {
    pub fn identity(self, i: Identity) -> Retex;
    pub fn halt_on_error(self, on: bool) -> Retex;
    pub fn draft_images(self, on: bool) -> Retex;   // \PassOptionsToPackage{draft}{graphicx} before the document
}
pub fn clean(build_dir: &Path, job: &str) -> io::Result<()>;
```
