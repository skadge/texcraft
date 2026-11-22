<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/ai.storyteller.texcraft.png" width="128" height="128" alt="TeXCraft icon: an engraved blackbird on a gold tile">
</p>

<h1 align="center">TeXCraft</h1>

<p align="center">
  <b>LaTeX writing; an open-source editor that shows the PDF as you type, built in pure Rust.</b>
</p>

<p align="center">
  A project of files, a source editor and the PDF side by side, rebuilt as you type by
  <a href="https://gitlab.com/severin-lemaignan/retex">retex</a>, a TeX engine written in Rust.
  It runs natively on Linux, macOS and Windows; a browser version is being built.
</p>

<p align="center">
  <img alt="Status: early development" src="https://img.shields.io/badge/status-early%20development-d6bc25?style=flat-square">
  <img alt="Written in pure Rust" src="https://img.shields.io/badge/pure-Rust-a08a10?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on Linux, macOS and Windows; web in progress" src="https://img.shields.io/badge/runs%20on-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows%20%C2%B7%20Web%20(soon)-555555?style=flat-square">
  <img alt="Engine: retex" src="https://img.shields.io/badge/engine-retex-555555?style=flat-square">
  <img alt="MCP server for agents" src="https://img.shields.io/badge/agents-MCP%20server-555555?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-555555?style=flat-square">
</p>

<br>

<p align="center">
  <img src="docs/images/editor-magazine.jpg" alt="TeXCraft with retex's magazine sample open: the project's fonts, images and files in the side panel, magazine.tex in the editor, and the magazine's cover, a lion in micrography on a photograph of Barcelona, in the PDF pane" width="100%">
  <br><sub><b>The magazine</b>: four full-bleed pages with OpenType fonts, photographs, TikZ and MetaPost, built by retex inside TeXCraft · retex's <code>samples/magazine</code></sub>
</p>

<p align="center">
  <a href="#a-look-around">A look around</a> ·
  <a href="#why-texcraft">Why TeXCraft</a> ·
  <a href="#what-retex-brings">What retex brings</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#status">Status</a> ·
  <a href="#downloads">Downloads</a> ·
  <a href="#license-and-credits">License</a>
</p>

## A look around

<table>
<tr>
<td width="50%" valign="top">
  <img src="docs/images/paper-dark.png" alt="TeXCraft in the dark theme with retex's research paper sample: the outline lists the paper's sections, paper.tex is in the editor and the first page of the two-column paper is in the PDF pane" width="100%">
  <p align="center"><sub><b>Outline and dark theme</b>: a two-column paper with theorems, a TikZ diagram and a biber bibliography · retex's <code>samples/paper</code></sub></p>
</td>
<td width="50%" valign="top">
  <img src="docs/images/log-dark.png" alt="The build log in place of the PDF: an error and two warnings as cards with their messages, help and links to the lines, which the editor's gutter marks" width="100%">
  <p align="center"><sub><b>Build log</b>: each message as a card with retex's help, a link to its line, and markers in the gutter</sub></p>
</td>
</tr>
<tr>
<td colspan="2" valign="top">
  <img src="docs/images/start.png" alt="The start page: New Project and Open Folder, and three recent projects as cards with the first page of their last PDF" width="100%">
  <p align="center"><sub><b>Start page</b>: recent projects with the first page of their last PDF</sub></p>
</td>
</tr>
</table>

## Why TeXCraft

- **Familiar.** The arrangement people know from online LaTeX editors: project files on the left, the source in the middle, the PDF on the right, the build log in the PDF's place, an Insert menu and a toolbar for the usual markup.
- **The PDF follows your typing.** A build starts when typing pauses, and the PDF keeps its place across builds.
- **Messages you can act on.** retex explains errors and warnings in words, with help on how to fix them; each one links to its line, and the editor marks the lines.
- **Yours, offline.** A project is a folder on your disk, opened, saved and versioned like any other.
- **Agent-native.** Every command of the menus can be run over a JSON control channel of the open window and by an **MCP server**, confined to the folders it is given.
- **One codebase.** The same Rust interface for the desktop and, once the engine runs there, the browser.

## What retex brings

TeXCraft builds with [retex](https://gitlab.com/severin-lemaignan/retex), a TeX engine written in Rust that runs inside it. A few things work that a LaTeX setup usually needs extra tools for:

- **More image formats.** `\includegraphics` takes **SVG** directly (no Inkscape, no `-shell-escape`), **JPEG XL**, **WebP**, GIF and TIFF, as well as PNG, JPEG and PDF.
- **No TeX installation needed.** A package the document needs and the installed TeX Live lacks is downloaded from TeX Live; without a TeX Live at all, retex keeps a tree of its own.
- **Bibliographies and indexes without external programs.** BibTeX, biber and makeindex are built into the engine (no Perl, no Java), and run as many times as the document needs within one build.
- **minted without Python.** Code listings are highlighted in process, without Python, Pygments or `-shell-escape`.
- **Colour emoji** in documents, through OpenType colour fonts.

## Quick start

```sh
git clone https://gitlab.com/severin-lemaignan/texcraft
cd texcraft

cargo run --release -p texcraft                                   # start page
cargo run --release -p texcraft -- examples/article               # open a project folder
cargo run --release -p texcraft -- --control                      # + JSON control channel
cargo run --release -p texcraft-cli -- compile examples/article   # build without a window
cargo xtask ci                                                    # fmt, clippy, tests, layering, wasm32 build
```

TeXCraft needs Rust 1.95 (`rust-toolchain.toml` selects it) and finds TeX Live as retex does: an installed TeX Live 2025 or 2026, or the tree retex downloads.

The interface uses Inter from [storytold/craft-fonts](https://github.com/storytold/craft-fonts), an optional build input: clone it into `./craft-fonts`, which `.cargo/config.toml` points `CRAFT_FONTS_DIR` at. Without it, egui's bundled fonts are used.

### Use it from Claude Code and other agents

Register the MCP server, giving it the folders it may read and write:

```sh
claude mcp add texcraft -- /path/to/texcraft-cli mcp --automation-read-root /work/thesis --automation-write-root /work/thesis
```

It refuses to start without both. The open window's control channel is described in [`docs/control-protocol.md`](docs/control-protocol.md).

## Status

TeXCraft is in early development. [`docs/plan.md`](docs/plan.md) has what works, the phases ahead and the changes it needs in retex; [`ROADMAP.md`](ROADMAP.md) the stage.

**What works:**
- projects as folders, a start page and recent projects;
- the source editor with highlighting, line numbers, error markers and an insert toolbar;
- the PDF preview with zoom and pages;
- builds after typing pauses, the build log, and the outline;
- native file dialogs, keyboard shortcuts you can change, settings;
- light and dark themes;
- the control channel and the MCP server.

**What's next:**
- TeXCraft's own editor widget, with completion, find and replace, and spell checking;
- a formatted editing mode, with sections, emphasis, formulas and tables shown formatted;
- navigation between the source and the PDF;
- the engine in the browser;
- history, sharing and working together.

**Workspace:** `crates/{project, latex, compile, preview, engine, ui-egui, mcp}` and `apps/{texcraft, texcraft-cli, texcraft-web}`. The egui frontend is its own crate, so the interface can change without touching the engine. The interface's look is described in [`docs/design.md`](docs/design.md).

Agent and contributor rules are in [`AGENTS.md`](AGENTS.md), which also lists the checks for the crates you change.

## Downloads

There are no releases yet. Build it from source as shown in [Quick start](#quick-start).

## License and credits

TeXCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Copyright (c) 2026 the TeXCraft contributors. Required notices are in [NOTICE](NOTICE).

The typesetting engine and language server are [retex](https://gitlab.com/severin-lemaignan/retex) (CC0-1.0). The interface builds on the Craft shared libraries ([storytold/craft-libs](https://github.com/storytold/craft-libs), MIT OR Apache-2.0) and egui.

Bundled fonts, icons and other assets keep their own open licenses; each one is listed with its author, source and license in [ATTRIBUTION.md](ATTRIBUTION.md). The screenshots show retex's magazine and paper samples, made for retex and released with it under CC0-1.0.

The app icon (an engraved blackbird on TeXCraft gold, `#d6bc25`) is the project owner's original artwork; its palette and files are in [`assets/app-icon/`](assets/app-icon/README.md).
