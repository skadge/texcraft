# TeXCraft

TeXCraft is a LaTeX editor for the desktop (Linux, macOS, Windows) and the browser, written in Rust with egui and the Craft shared libraries. Its typesetting engine and language server are [retex](https://gitlab.com/severin-lemaignan/retex).

A project is a folder of files. The window shows the project files and the outline, the source, and the PDF, which is rebuilt shortly after typing stops. The build log lists errors and warnings, and selecting one opens its line.

Status: early scaffold. [docs/plan.md](docs/plan.md) describes what works and the plan.

## Building and running

Rust 1.95 (`rust-toolchain.toml` selects it).

```sh
cargo run --release -p texcraft -- examples/article                  # desktop app
cargo run --release -p texcraft-cli -- compile examples/article      # build a project without a window
cargo run --release -p texcraft-cli -- mcp --automation-read-root DIR --automation-write-root DIR   # agent tools
cd apps/texcraft-web && trunk serve                                  # web app (no engine in the browser yet)
```

For the interface faces, clone [craft-fonts](https://github.com/storytold/craft-fonts) into `./craft-fonts` (gitignored); `.cargo/config.toml` points `CRAFT_FONTS_DIR` there. Without it, egui's bundled faces are used.

The desktop app finds TeX Live the way retex does: an installed TeX Live 2025 or 2026, or the tree retex downloads.

## Checks

`cargo xtask ci` runs fmt, clippy, the tests, the layering check and the wasm32 build. Rules for contributors and agents are in [AGENTS.md](AGENTS.md).

## Licence

MIT or Apache-2.0, at your choice. Third-party material is listed in [ATTRIBUTION.md](ATTRIBUTION.md) and [NOTICE](NOTICE).
