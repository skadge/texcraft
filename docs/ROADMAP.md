# TeXCraft roadmap

**Last updated:** 2026-11-24 · **Current release:** 0.4.0

TeXCraft is a LaTeX editor for the desktop and the browser: a project of files, a source editor and
the PDF side by side, rebuilt as you type by [retex](https://gitlab.com/severin-lemaignan/retex).
This page says what is done and what comes next. What changed in each release is in
[CHANGELOG.md](../CHANGELOG.md), the day-by-day record in [progress.md](progress.md).

✅ done, with the release it shipped in · 🚧 partly done · ⬜ planned

## Projects and files

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | Projects as folders | Open, save and save as through the system's file pickers; asks before unsaved changes are lost. |
| ✅ 0.2.2 | Start page and recent projects | New project, open folder, and the recent projects as cards with the first page of their PDF. |
| ✅ 0.2.2 | Publishers' templates | 15 journal, conference, CV and poster templates, downloaded from TeX Live and checked against its signed package database, kept for offline use. |
| ✅ 0.2.2 | Files of a project | Folders, a main-document mark, new, rename, delete, add files by picker or by dropping them; images shown in the editor pane. |
| ✅ 0.4.0 | Renaming a project | Click its name in the top bar; an untitled project is saved under the name typed. |
| ⬜ | New folders and moving files | New folder in the file tree, and dragging files between folders. |
| ⬜ | Go to File | A palette of the project's files, the recent ones first. |
| ⬜ | Recent projects in the system | The dock menu on macOS, jump lists on Windows, recently used on Linux; pinning a project. |
| ⬜ | More templates | TeXCraft's own templates (CC0), thumbnails in the gallery, saving a project as a template. |

## Source editor

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | TeXCraft's own editor | Rope with grouped undo, gutter, soft wrap, bracket matching, closing brackets and environments, IME, an accessible text node. |
| ✅ 0.2.2 | Vim key bindings | Modes, motions, operators, LaTeX text objects (environments, inline math), search, `:s`, registers, marks, macros. |
| ✅ 0.2.2 | Find and replace | Match case, regular expressions, whole words, in selection; Replace All as one undoable step. |
| ✅ 0.2.2 | Language features | Completion, hover, go to definition, quick fixes and hints as you type, from retex's language server. |
| ✅ 0.2.2 | Insert toolbar and menu | Headings, bold and italic, lists, figure, table, formulas, references, citations, links. |
| ✅ 0.4.0 | Images by drag and drop | An image dragged from the files onto the source is inserted with `\includegraphics`. |
| ⬜ | Context menu in the text | Right-click (or the Menu key): Cut, Copy, Paste, and Add a Comment on a selection. |
| ⬜ | Selections in Vim mode | The mouse and Visual mode's selection usable by commands such as commenting. |
| ⬜ | Folding and several carets | Folding environments and sections; multiple carets. |
| ⬜ | Project-wide search | Search and replace across files, results grouped by file. |
| ⬜ | Writing aids | A symbol palette, and spell checking that skips commands, math and verbatim. |

## Formatted view

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | Formatted view, read-only | A Source/Formatted switch: headings, styled text, lists, figure cards, chips for references and citations; double-click shows the source there. |
| ⬜ | Typeset formulas | Formulas drawn by retex with the document's preamble, cached. |
| ⬜ | Editing in the formatted view | Typing, styles and lists in the view, each edit a minimal change of the source. |
| ⬜ | Tables and figures | A grid editor for `tabular`, and a dialog for figures. |

## Building, preview and navigation

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | Builds as you type | Automatic builds after a pause, progress (pass, page), a draft PDF while later passes run, Stop. |
| ✅ 0.2.2 | Fast rebuilds | retex's worker keeps the preamble between builds: under a second after an edit. |
| ✅ 0.2.2 | Build log | Errors, warnings and notes tied to their line, with retex's help and fixes. |
| ✅ 0.4.0 | Copying build messages | Messages can be selected and copied, one by one or all at once. |
| ✅ 0.2.2 | Build options | Engine (from the document, LuaLaTeX, pdfLaTeX), stop at the first error, draft images, delete build files. |
| ✅ 0.2.2 | Source and PDF navigation | Double-click in the PDF opens the source line; Show in PDF marks the caret's line. |
| ⬜ | Progress beside the PDF | The page being built also in the PDF pane's header. |
| ⬜ | A better preview | Pages drawn in the background, zoom presets, links, text selection, presentation mode. |
| ⬜ | The raw log and outputs | Search in the raw log, and the list of output files. |

## Comments and working together

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | Comments | Threads on passages in a margin beside the text, replies, resolve, filters, kept in the project. |
| ✅ 0.4.0 | Hiding the comments | The margin can be hidden; the Review panel still lists the comments. |
| ⬜ | Suggested changes | Edits proposed in the margin, accepted or rejected one by one. |
| ⬜ | History | Snapshots on save and on builds, a diff view, restoring a file or the project. |
| ⬜ | Git | Open from a repository, commit, push, pull with a choice per conflict. |
| ⬜ | Cloud projects and sharing | Projects in cloud storage, shared with others. |
| ⬜ | Real-time editing together | Waits for the suite's shared-text layer. |

## In the browser

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | The web app | The editor in the browser, with a loading screen; files added by picker or by dropping them. |
| ✅ 0.3.0 | Builds in the browser | TeXCraft's own build of retex's engine runs in the page, through a WASI host written in Rust. |
| ✅ 0.4.0 | TeXCraft's own TeX tree | The site serves the TeX files itself: the common classes and packages, and every template's. |
| ✅ 0.4.0 | Kept between visits | Downloaded TeX files and the engine's format stay in the browser: later first builds take seconds. |
| ✅ 0.4.0 | Projects in the browser | Saved by name, listed on the start page, with their comments; a warning before leaving with unsaved changes. |
| ✅ 0.4.0 | Templates in the browser | Served with the browser version; a template makes a named project. |
| ✅ 0.4.0 | Whole projects and draft images | Files outside the main document's folder are found; draft images work. |
| ⬜ | Missing packages | Downloaded during the build (retex 2.5.0 can), once a TeX Live repository is hosted with CORS. |
| ⬜ | Faster first builds | A read list per project; waiting for a file instead of stopping the run (JavaScript Promise Integration). |
| ⬜ | Safe updates | Versioned worker files, so a browser never keeps an old engine after an update. |
| ⬜ | Cloud storage and embedding | Opening and saving to cloud storage; TeXCraft embedded in another page. |

## Output formats and print

| | Feature | Description |
|---|---|---|
| ⬜ | Export | SVG, OpenDocument and Word, PowerPoint for beamer, HTML, Markdown, PNG pages (retex writes them). |
| ⬜ | Accessible PDF | PDF/UA output, with retex's accessibility findings linked to the source. |
| ⬜ | Print | PDF/X with bleed and crop marks, and preflight checks placed on the page and in the source. |

## App, platforms and automation

| | Feature | Description |
|---|---|---|
| ✅ 0.2.2 | Window and themes | TeXCraft's own window frame, light and dark themes from tokens, a contrast check on every colour pair. |
| ✅ 0.2.2 | Menus, shortcuts and settings | Menus from the command registry, every shortcut user-editable, a Settings dialog. |
| ✅ 0.2.2 | Accessibility basics | Every control named, build results announced, F6 between regions. |
| ✅ 0.2.2 | Agents and automation | Every action is a command; agent tools confined to their folders; a control channel. |
| ✅ 0.2.2 | Linux packages | AppImage and Flatpak, built and published by CI. |
| ✅ 0.4.0 | GitHub mirror and releases | Each push mirrored to GitHub, each release published there with its files. |
| ⬜ | macOS and Windows | Packages for both. |
| ⬜ | Updates and translations | In-app updates (craft-update), translations (craft-i18n). |
| ⬜ | More of the window | The PDF in its own window, a distraction-free mode, the macOS menu bar, Alt or F10 for the menus. |
| ⬜ | Accessibility pass | Keyboard-path tests for each panel, a screen-reader pass. |

## AI assistance

Off until the user connects a model (any OpenAI-compatible endpoint, a local one included); every change it proposes arrives as a suggested change.

| | Feature | Description |
|---|---|---|
| ⬜ | The connection | Endpoint, token in the system's secret store, model; what is sent, and to whom, shown before it is sent. |
| ⬜ | Explaining build errors | A plain-language explanation and a proposed fix. |
| ⬜ | Rewriting and continuing | Rewrite a selection (clearer, shorter, translated) keeping its LaTeX; optional completion as you type. |
| ⬜ | Making LaTeX | Tables, equations, TikZ figures from a description; BibTeX entries from a title or DOI. |
| ⬜ | Asking and reviewing | Questions about the project; a review left as comments. |

## Open decisions

- **Name:** "TeX" is a trademark of the American Mathematical Society; the name needs a trademark check before it ships widely.
- **Licence:** `MIT OR Apache-2.0`, the suite's; retex is CC0, so either works.
- **Home of the repository:** on gitlab.com, mirrored to GitHub; joining the Craft suite means an RFC in craft-contrib.

## Further reading

[docs/retex-changes.md](retex-changes.md) (what TeXCraft uses of each retex release, and what it asked for),
[docs/design.md](design.md) (the design language), [docs/collaboration.md](collaboration.md) and
[docs/collaboration-roadmap.md](collaboration-roadmap.md) (comments and working together),
[docs/control-protocol.md](control-protocol.md) (automation), [docs/releasing.md](releasing.md).
