# TeXCraft roadmap

**Last updated:** 2026-11-25 · **Current release:** 0.7.0

TeXCraft is a LaTeX editor for the desktop and the browser: a project of files, a source editor and
the PDF side by side, rebuilt as you type by [retex](https://gitlab.com/severin-lemaignan/retex).
This page says what is done and what comes next. What changed in each release is in
[CHANGELOG.md](../CHANGELOG.md), the day-by-day record in [progress.md](progress.md).

Each feature has a code (P3, W5…) to refer to it by: in issues, commits and discussions. Codes stay with their feature; new features take the next number of their area.

✅ done, with the release it shipped in · 🚧 partly done · ⬜ planned

## Projects and files

| Code | | Feature | Description |
|---|---|---|---|
| P1 | ✅ 0.2.2 | Projects as folders | Open, save and save as through the system's file pickers; asks before unsaved changes are lost. |
| P2 | ✅ 0.2.2 | Start page and recent projects | New project, open folder, and the recent projects as cards with the first page of their PDF. |
| P3 | ✅ 0.2.2 | Publishers' templates | 15 journal, conference, CV and poster templates, downloaded from TeX Live and checked against its signed package database, kept for offline use. |
| P4 | ✅ 0.2.2 | Files of a project | Folders, a main-document mark, new, rename, delete, add files by picker or by dropping them; images shown in the editor pane. |
| P5 | ✅ 0.4.0 | Renaming a project | Click its name in the top bar; an untitled project is saved under the name typed. |
| P6 | ✅ 0.5.0 | New folders and moving files | New folder in the file tree, and dragging files between folders (Move to… from the keyboard). |
| P7 | ✅ 0.5.0 | Go to File | A palette of the project's files, the recent ones first. |
| P8 | ⬜ | Recent projects in the system | The dock menu on macOS, jump lists on Windows, recently used on Linux; pinning a project. |
| P9 | 🚧 | More templates | ✅ 0.5.0: TeXCraft's own templates (CC0, built in, offline and in the browser): article, thesis, slides, letter, CV, homework. Still to do: thumbnails in the gallery, saving a project as a template. |

## Source editor

| Code | | Feature | Description |
|---|---|---|---|
| E1 | ✅ 0.2.2 | TeXCraft's own editor | Rope with grouped undo, gutter, soft wrap, bracket matching, closing brackets and environments, IME, an accessible text node. |
| E2 | ✅ 0.2.2 | Vim key bindings | Modes, motions, operators, LaTeX text objects (environments, inline math), search, `:s`, registers, marks, macros. |
| E3 | ✅ 0.2.2 | Find and replace | Match case, regular expressions, whole words, in selection; Replace All as one undoable step. |
| E4 | ✅ 0.2.2 | Language features | Completion, hover, go to definition, quick fixes and hints as you type, from retex's language server. |
| E5 | ✅ 0.2.2 | Insert toolbar and menu | Headings, bold and italic, lists, figure, table, formulas, references, citations, links. |
| E6 | ✅ 0.4.0 | Images by drag and drop | An image dragged from the files onto the source is inserted with `\includegraphics`. |
| E7 | ✅ 0.5.0 | Context menu in the text | Right-click, Shift+F10 or the Menu key: Cut, Copy, Paste, Select All, Add a Comment on a selection, Quick Fix, Show in PDF. |
| E8 | ✅ 0.5.0 | Selections in Vim mode | The mouse and Visual mode's selection usable by commands such as commenting. |
| E9 | ✅ 0.7.0 | Folding and several carets | Folding environments, sections and the preamble from the gutter, Edit › Fold and Vim's `z` keys; several carets with Alt+click and Cmd+D. |
| E10 | ✅ 0.5.0 | Project-wide search | Search and replace across files, results grouped by file. |
| E11 | ⬜ | Writing aids | A symbol palette, and spell checking that skips commands, math and verbatim. |

## Formatted view

| Code | | Feature | Description |
|---|---|---|---|
| F1 | ✅ 0.2.2 | Formatted view, read-only | A Source/Formatted switch: headings, styled text, lists, figure cards, chips for references and citations; double-click shows the source there. |
| F2 | ✅ 0.6.0 | Typeset formulas | Formulas drawn by retex with the document's preamble, cached, in the colour of the text; errors underlined with retex's message. Desktop only: the browser still shows their source. |
| F3 | ⬜ | Editing in the formatted view | Typing, styles and lists in the view, each edit a minimal change of the source. |
| F4 | ⬜ | Tables and figures | A grid editor for `tabular`, and a dialog for figures. |

## Building, preview and navigation

| Code | | Feature | Description |
|---|---|---|---|
| B1 | ✅ 0.2.2 | Builds as you type | Automatic builds after a pause, progress (pass, page), a draft PDF while later passes run, Stop. |
| B2 | ✅ 0.2.2 | Fast rebuilds | retex's worker keeps the preamble between builds: under a second after an edit. |
| B3 | ✅ 0.2.2 | Build log | Errors, warnings and notes tied to their line, with retex's help and fixes. |
| B4 | ✅ 0.4.0 | Copying build messages | Messages can be selected and copied, one by one or all at once. |
| B5 | ✅ 0.2.2 | Build options | Engine (from the document, LuaLaTeX, pdfLaTeX), stop at the first error, draft images, delete build files. |
| B6 | ✅ 0.2.2 | Source and PDF navigation | Double-click in the PDF opens the source line; Show in PDF marks the caret's line. |
| B7 | ✅ 0.5.0 | Progress beside the PDF | The page being built also in the PDF pane's header. |
| B8 | ✅ 0.6.0 | A better preview | Pages drawn in the background, zoom presets, links, text selection, presentation mode. Selection is on one page at a time; presentations fill the window, not the screen. |
| B9 | ⬜ | The raw log and outputs | Search in the raw log, and the list of output files. |

## Comments and working together

| Code | | Feature | Description |
|---|---|---|---|
| C1 | ✅ 0.2.2 | Comments | Threads on passages in a margin beside the text, replies, resolve, filters, kept in the project. |
| C2 | ✅ 0.4.0 | Hiding the comments | The margin can be hidden; the Review panel still lists the comments. |
| C3 | ✅ 0.7.0 | Suggested changes | Suggest mode: edits proposed instead of made, shown struck through and underlined in the text and as cards in the margin, accepted or rejected one by one or all at once (each accept undoable), replies, a PDF with or without them. Outside Suggest mode a replacement shows only in its card. |
| C4 | ✅ 0.5.0 | History | The project's commits (C8) by date, a diff view, restoring a file or the whole project at a version (itself a new commit). |
| C5 | 🚧 | Git forges | A project opened from GitLab or GitHub (their web APIs answer browsers), committed back, kept up with the branch with a three-way merge, a merge request when the branch cannot take a commit; push and pull for folders (retex's `docs/design/gitlab.md`; docs/forges.md). Done, step 1: GitLab with a personal access token, kept for the session only: a branch opened (in memory on the desktop, in the library in the browser), its changed files, commits back to it. Step 2: kept up with the branch (Update from GitLab, and a check every two minutes and when the window comes back), files changed on both sides merged three-way line by line, conflicts settled per file (keep mine, take theirs) or sent to a new branch with a merge request, which a protected branch also gets; review threads carried with the project. Still to do: signing in with OAuth, GitHub, folders. |
| C6 | ⬜ | Cloud projects and sharing | Projects in cloud storage, shared with others. |
| C7 | ⬜ | Real-time editing together | Waits for the suite's shared-text layer; one participant commits for the session (retex's collaboration notes). |
| C8 | ✅ 0.7.0 | Every project a git repository | As Overleaf does: a project's history is a git repository, whether or not it is ever pushed anywhere. A folder that is not in one becomes one on its first save, and each save is a commit; a folder already in a repository is used as it is, and commits only when asked. Browser projects keep the same history in the browser, as versions whose files have git's blob ids; writing them as git commits comes with C5. |

## In the browser

| Code | | Feature | Description |
|---|---|---|---|
| W1 | ✅ 0.2.2 | The web app | The editor in the browser, with a loading screen; files added by picker or by dropping them. |
| W2 | ✅ 0.3.0 | Builds in the browser | TeXCraft's own build of retex's engine runs in the page, through a WASI host written in Rust. |
| W3 | ✅ 0.4.0 | TeXCraft's own TeX tree | The site serves the TeX files itself: the common classes and packages, and every template's. |
| W4 | ✅ 0.4.0 | Kept between visits | Downloaded TeX files and the engine's format stay in the browser: later first builds take seconds. |
| W5 | ✅ 0.4.0 | Projects in the browser | Saved by name, listed on the start page, with their comments; a warning before leaving with unsaved changes. |
| W6 | ✅ 0.4.0 | Templates in the browser | Served with the browser version; a template makes a named project. |
| W7 | ✅ 0.4.0 | Whole projects and draft images | Files outside the main document's folder are found; draft images work. |
| W8 | ⬜ | Missing packages | Downloaded during the build (retex 2.5.0 can), once a TeX Live repository is hosted with CORS. |
| W9 | ✅ 0.5.0 | Faster first builds | A read list per project: the TeX files its builds read are kept, and fetched together before its first build of a visit, so the build no longer stops for each package beyond a plain article's. |
| W10 | ✅ 0.5.0 | Safe updates | The engine's files are named after their contents at each build (the page names the worker's), and the TeX tree's files carry its index's version, so a browser never keeps an old engine or mixes trees after an update. |
| W12 | ⬜ | Waiting for a file | A build that needs a file not fetched yet waits for it instead of stopping and starting again (JavaScript Promise Integration). |
| W11 | ⬜ | Cloud storage and embedding | Opening and saving to cloud storage; TeXCraft embedded in another page. |

## Output formats and print

| Code | | Feature | Description |
|---|---|---|---|
| O1 | ✅ 0.5.0 | Export | The PDF pane's Export menu: PDF, Word, OpenDocument text, PowerPoint and OpenDocument presentations (first for beamer), HTML, Markdown, SVG; on the desktop and in the browser (as downloads). The pane always shows the PDF: HTML and Markdown are exports only (operator's decision, 2026-11-25). |
| O2 | ✅ 0.6.0 | Accessible PDF | PDF/UA output, with retex's accessibility findings linked to the source. |
| O3 | ✅ 0.6.0 | Print | PDF/X with bleed and crop marks, and preflight checks placed on the page and in the source. |

## App, platforms and automation

| Code | | Feature | Description |
|---|---|---|---|
| A1 | ✅ 0.2.2 | Window and themes | TeXCraft's own window frame, light and dark themes from tokens, a contrast check on every colour pair. |
| A2 | ✅ 0.2.2 | Menus, shortcuts and settings | Menus from the command registry, every shortcut user-editable, a Settings dialog. |
| A3 | ✅ 0.2.2 | Accessibility basics | Every control named, build results announced, F6 between regions. |
| A4 | ✅ 0.2.2 | Agents and automation | Every action is a command; agent tools confined to their folders; a control channel. |
| A5 | ✅ 0.2.2 | Linux packages | AppImage and Flatpak, built and published by CI. |
| A6 | ✅ 0.4.0 | GitHub mirror and releases | Each push mirrored to GitHub, each release published there with its files. |
| A7 | 🚧 | macOS and Windows | Experimental builds on the GitHub releases (a Windows zip, a macOS app for Apple Silicon, unsigned); still to do: signing, installers, Intel Macs. |
| A8 | ⬜ | Updates and translations | In-app updates (craft-update), translations (craft-i18n). |
| A9 | 🚧 | More of the window | The PDF in its own window, a distraction-free mode (focus mode), Alt or F10 for the menus: done. Remaining: the macOS menu bar (craft-menu), not built here for want of a macOS toolchain. |
| A10 | ⬜ | Accessibility pass | Keyboard-path tests for each panel, a screen-reader pass. |

## AI assistance

Off until the user connects a model (any OpenAI-compatible endpoint, a local one included); every change it proposes arrives as a suggested change.

| Code | | Feature | Description |
|---|---|---|---|
| AI1 | ⬜ | The connection | Endpoint, token in the system's secret store, model; what is sent, and to whom, shown before it is sent. |
| AI2 | ⬜ | Explaining build errors | A plain-language explanation and a proposed fix. |
| AI3 | ⬜ | Rewriting and continuing | Rewrite a selection (clearer, shorter, translated) keeping its LaTeX; optional completion as you type. |
| AI4 | ⬜ | Making LaTeX | Tables, equations, TikZ figures from a description; BibTeX entries from a title or DOI. |
| AI5 | ⬜ | Asking and reviewing | Questions about the project; a review left as comments. |

## Open decisions

- **Licence:** `MIT OR Apache-2.0`, the suite's; retex is CC0, so either works.
- **Home of the repository:** on gitlab.com, mirrored to GitHub; joining the Craft suite means an RFC in craft-contrib.

## Further reading

[docs/retex-changes.md](retex-changes.md) (what TeXCraft uses of each retex release, and what it asked for),
[docs/design.md](design.md) (the design language), [docs/collaboration.md](collaboration.md) and
[docs/collaboration-roadmap.md](collaboration-roadmap.md) (comments and working together),
[docs/control-protocol.md](control-protocol.md) (automation), [docs/releasing.md](releasing.md).
