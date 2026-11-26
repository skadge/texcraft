# TeXCraft roadmap

**Last updated:** 2026-11-26 · **Current release:** 0.14.0

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
| P10 | ✅ 0.12.0 | Folders and zip files | Dropping a folder adds its files with their folders; a zip file is unzipped into the project (or added as it is), also from File › Import a Zip File…; on the start page a folder opens and a zip file becomes a new project. Hidden files, build outputs and links out of the folder are left out, with limits, and what was left out is said. |

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
| E11 | ✅ 0.10.0 | Writing aids | A symbol palette, and spell checking that skips commands, math and verbatim. |
| E12 | 🚧 | Grammar | ✅ 0.13.0 on the desktop and the CLI: English grammar checked offline by Harper on the prose (commands, math and keys skipped), underlined apart from spelling, with replacements that keep the LaTeX, Ignore, rules and groups switched off; any language checked by the connected model on request. Still to do: the browser build (Harper with its dependencies adds about 10 MB to the page; a lighter Harper loses its agreement rules), offline grammar in other languages. |
| E13 | ⬜ | A toolbar that fits the split | At the default split the source toolbar shows only Heading, Bold and Italic: smaller groups that give way one at a time, formulas and references as single drop-downs; the pickers of labels and bibliography entries the formatted view has, in the source too (UX review, findings 13 and b). |
| E14 | ⬜ | The whole document's outline | The Outline through `\input` and `\include`, the shown file's part marked, so a project whose sections are in other files has one outline (UX review, finding 14). |

## Formatted view

| Code | | Feature | Description |
|---|---|---|---|
| F1 | ✅ 0.2.2 | Formatted view, read-only | A Source/Formatted switch: headings, styled text, lists, figure cards, chips for references and citations; double-click shows the source there. |
| F2 | ✅ 0.6.0 | Typeset formulas | Formulas drawn by retex with the document's preamble, cached, in the colour of the text; errors underlined with retex's message. Desktop only: the browser still shows their source. |
| F3 | 🚧 | Editing in the formatted view | ✅ 0.8.0: typing in text runs. ✅ 0.13.0: the toolbar, structural typing, pickers, the spelling menu. Next release: LaTeX blocks edited in place, formulas with a live preview, toolbar state for `\emph` and declarations. Typing, styles and lists in the view, each edit a minimal change of the source. Done: caret and selection in text runs, typing and paste escaped (bulleted and numbered lines pasted as lists), Enter (paragraphs, items, an empty item ending the list), Tab and Shift+Tab nesting list items, Backspace lifting a heading or an item, atoms as single stops with a two-press delete, a toolbar showing the caret's state (paragraph style as the document's heading levels, Bold, Italic, Code, lists, indent, link, footnote, citation and cross-reference pickers, formulas, table, figure), the spelling menu, citations' author and year on hover, Ctrl+click to a reference's label, undo shared with the source, an editable text node, LaTeX blocks and runs shown as source edited in place (click or Enter; Escape or a click outside to finish, one minimal edit), formulas edited in place and inserted with a live typeset preview (desktop), the toolbar's state in `\emph` and declarations (`\bfseries`, `\itshape`). Still to come: pasted HTML, IME marked text, Suggest mode, the formula preview in the browser. |
| F4 | 🚧 | Tables and figures | ✅ 0.9.0: the grid and the figure dialog. Next release: header row, merged cells, caption and label, figures resized and captioned in the view. A grid editor for `tabular`, and a dialog for figures. Done: `tabular` read as a grid (or kept as source with the reason), cells edited in place, Tab and arrows between cells, a toolbar for rows, columns, alignment and borders (none, all, booktabs, the package said not added), the figure dialog (image, width, placement, caption, label), `table.*` and `figure.*` commands, an image file dropped on the view made a figure (desktop and browser), a header row, merging and splitting cells (`\multicolumn`), a table's caption and label (a bare table put in a float), the image resized by a handle (snapping to quarters of the line), its placement and label in its context menu, Add Caption. Still to come: cells spanning rows (`\multirow`), the caption's position, `tabularx` and `longtable`. |
| F5 | 🚧 | The formatted view for readers | What a reader who does not know LaTeX expects: cross-references as the numbers the last build gave them (the key on hover), an included file's text shown in place under its name, environments named in words (UX review, finding 5). |

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
| B9 | ✅ 0.10.0 | The raw log and outputs | Search in the raw log (count of matches, previous and next, marked and scrolled to), and the build's files (PDF, PDF/X, SyncTeX, log) with Save As…, Download in the browser and Show in Folder on the desktop. |
| B10 | 🚧 | Errors where they are | Next release: an environment left open, or a `{` never closed, in an included file is reported at its `\begin` (or its `{`) in that file, read from TeX's log (the files it opened, the line it names) and the project's files; where TeX noticed it (`main.tex:59`) is linked under the message and marked in the editor too; the build log's button is named with its count ("Build log, 2 errors"). Still to do: messages written as sentences (retex) (UX review, findings 6, 20 and 27). |

## Comments and working together

| Code | | Feature | Description |
|---|---|---|---|
| C1 | ✅ 0.2.2 | Comments | Threads on passages in a margin beside the text, replies, resolve, filters, kept in the project. |
| C2 | ✅ 0.4.0 | Hiding the comments | The margin can be hidden; the Review panel still lists the comments. |
| C3 | ✅ 0.7.0 | Suggested changes | Suggest mode: edits proposed instead of made, shown struck through and underlined in the text and as cards in the margin, accepted or rejected one by one or all at once (each accept undoable), replies, a PDF with or without them. Outside Suggest mode a replacement shows only in its card. |
| C4 | ✅ 0.5.0 | History | The project's commits (C8) by date, a diff view, restoring a file or the whole project at a version (itself a new commit). |
| C5 | 🚧 | Git forges | A project opened from GitLab or GitHub (their web APIs answer browsers), committed back, kept up with the branch with a three-way merge, a merge request when the branch cannot take a commit; push and pull for folders (retex's `docs/design/gitlab.md`; docs/forges.md). ✅ 0.8.0, step 1: GitLab with a personal access token, kept for the session only: a branch opened (in memory on the desktop, in the library in the browser), its changed files, commits back to it. Step 2: kept up with the branch (Update from GitLab, and a check every two minutes and when the window comes back), files changed on both sides merged three-way line by line, conflicts settled per file (keep mine, take theirs) or sent to a new branch with a merge request, which a protected branch also gets; review threads carried with the project. ✅ 0.9.0, step 3: GitHub with a fine-grained personal access token, through the same sync (its REST API and Git Data API; the branch moved without forcing, a moved branch caught up and the commit made again), a pull request when the branch cannot take a commit; the dialogs and menus name the service. ✅ 0.11.0, step 4: signing in instead of a token: GitLab with OAuth and PKCE (the desktop's loopback redirect, the browser's whole-page round trip), reading first and asking for `api` at the first commit, renewed with the refresh token, kept in the system's keychain (the tab's session storage in the browser), revoked on sign out; GitHub with the device flow on the desktop. ✅ 0.12.0: self-managed GitLab and GitHub Enterprise servers: added in Settings › Accounts (checked, then what to register on them shown with copy buttons, the application's id pasted), proposed by a shared link (asked first) or provided by an organisation's `texcraft-forges.toml`, then signed in to as gitlab.com; the desktop trusts the system's certificate store for private authorities. The Open dialog suggests the projects the account reaches as their path is typed (GitHub's list of the user's repositories and its search, GitLab's search of the user's projects; visibility, last activity, default branch) and lists the chosen project's branches in a drop-down with the default one chosen (`forge.projects`, `forge.branches`). ✅ 0.12.0, step 5: push and pull for project folders on the desktop, with git's smart HTTP protocol spoken by TeXCraft (`texcraft-gitwire`; gix has no push): remotes in `.git/config`, ahead and behind in the History panel, Push (refused over commits not pulled), Pull (a fast-forward, or a merge of text files line by line, refused with the files named when they do not merge); Save Project to GitHub… or to GitLab… makes a private repository for the account or one of its organisations or groups, sets it as `origin` and pushes the whole history (in the browser the files go as its first commit and the project is linked to it). |
| C6 | ⬜ | Cloud projects and sharing | Projects in cloud storage, shared with others. |
| C7 | ⬜ | Real-time editing together | Waits for the suite's shared-text layer; one participant commits for the session (retex's collaboration notes). |
| C8 | ✅ 0.7.0 | Every project a git repository | As Overleaf does: a project's history is a git repository, whether or not it is ever pushed anywhere. A folder that is not in one becomes one on its first save, and each save is a commit; a folder already in a repository is used as it is, and commits only when asked. Browser projects keep the same history in the browser, as versions whose files have git's blob ids. A folder's history goes to GitHub or GitLab with Push and Pull, or Save Project to GitHub or GitLab (C5, step 5). |
| C9 | 🚧 | A history that says what changed | Next release: a save's version describes itself (the files added, edited, deleted or renamed, at most three names; for a LaTeX file the sections whose text changed, from the outline: "Edited Introduction and Results (main.tex); added figures/plot.png"), its subject at most 72 characters and each file in its body; a commit made with a message keeps it; the History panel shows the subject, when, who and the files' names, the body on hover; desktop repositories and browser versions alike. Still to do: naming a version on save, unsaved changes as the first row (UX review, finding 7). |

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
| W11 | ⬜ | Cloud storage and embedding | Opening and saving to cloud storage; TeXCraft embedded in another page. |
| W12 | ⬜ | Waiting for a file | A build that needs a file not fetched yet waits for it instead of stopping and starting again (JavaScript Promise Integration). |
| W13 | 🚧 | Projects kept for good | ✅ 0.14.0: the first save asks the browser for persistent storage, and the start page says whether it was granted ("Kept until you delete them" or "may remove them when space runs low") and the room used and available; a write the browser refuses leaves the project unsaved and says why (for lack of room, what to do); File › Download Project as Zip (`project.download_zip`). Still to do: Download All Projects as one zip; a version whose write failed is not stored again with the next save (UX review, finding 10). |

## Output formats and print

| Code | | Feature | Description |
|---|---|---|---|
| O1 | ✅ 0.5.0 | Export | The PDF pane's Export menu: PDF, DOCX, OpenDocument text, PPTX and OpenDocument presentations (first for beamer), HTML, Markdown, SVG; on the desktop and in the browser (as downloads). The pane always shows the PDF: HTML and Markdown are exports only (operator's decision, 2026-11-25). |
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
| A8 | 🚧 | Updates and translations | ✅ 0.13.0: Translations: every interface string, command label and engine message goes through the catalogs (`i18n/<locale>.tsv`, craft-i18n's format, until the cutover to craft-i18n), Settings › Appearance › Interface language switches at once (System follows the system, or the browser), dates, times, sizes and units follow the region, and a pseudo locale checks longer languages' layouts; ✅ 0.13.0: German, Spanish, French, Italian, Dutch and Portuguese (Portugal), drafted with an AI model; ✅ 0.14.0: brought up to date, the English made easier to translate. Still to do: a review of each by a native speaker; in-app updates (craft-update). |
| A9 | 🚧 | More of the window | The PDF in its own window, a distraction-free mode (focus mode), Alt or F10 for the menus: ✅ 0.8.0. ✅ 0.9.0: the macOS menu bar (craft-menu, the in-window menus hidden there), not yet tried on a Mac. |
| A10 | 🚧 | Accessibility pass | ✅ 0.10.0: keyboard-path tests for every region, panel and dialog (focus moves in the accessibility tree, reading order, no trap, Escape gives the keyboard back, roles and names), and the fixes they found. Still to do: a screen-reader pass by a person; the Keyboard Shortcuts table's focusable node without a role (craft-keymap); the review margin's controls reached only by F6 (the source editor keeps Tab). |
| A11 | 🚧 | User guide | Help pages for people who use TeXCraft. ✅ 0.13.0: `docs/guide/`, static HTML pages for every area of the app (desktop and browser differences marked, unfinished features said), Help › User Guide (F1) and a link from Settings › AI, published beside the web editor, `cargo xtask guide-check`. Still to do: a copy in the packages for offline reading, screenshots for more pages, translations. |
| A12 | 🚧 | Dialogs and bars on a phone | ✅ 0.14.0: the start page at a phone's width (its buttons under the title, the main one first); every dialog no wider than the window, and below the narrow width a sheet over it (Close and the title at the top, the content scrolling, the actions in a bar at the bottom), Settings as a list of its sections that opens each as a page with Back, the template gallery in one column with its categories as a drop-down, the status bar with the build's state alone, no "PDF in its own window" there; Settings as tall as the window on the desktop. Still to do: trying it on real phones (the on-screen keyboard over a sheet's fields, the safe area of phones with a notch); the Keyboard Shortcuts dialog's own layout at a phone's width (craft-keymap) (issue #12; UX review, findings 1, 2, 15, 19 and 23). |
| A13 | 🚧 | Reading order and roles | ✅ the start page's buttons in the order they are shown. Still to do: the top bar's end (layout switch, window buttons) and the panel headers (Files: New File…, New Folder…, Add Files…) in reading order with `widgets::right_aligned`; the rail's Keyboard Shortcuts and Settings as buttons, not tabs (UX review, findings 8 and 9). |
| A14 | ⬜ | First steps | An example project on the start page (the article in `examples/`), template cards titled by their kind with the licence in words and the offline mark explained, a first-run hint for the Formatted view and the build log (UX review, findings 11 and 12). |
| A15 | 🚧 | Polish from the UX review | docs/ux-review.md, 2026-11-26. ✅ 0.14.0: labels named as their commands (layout switch, source toolbar), plurals, export formats by file name, the PDF toolbar's second row in longer languages, grammar not flagging indentation, the start page's status bar, the outline's empty text, stale guide notes, Settings as tall as the window (with A12), no "PDF in its own window" on a phone (with A12). Still to do: the side panel's width kept between sections, the AI token's placeholder, the unsaved-changes question saying why it asks, a dimmed PDF page in the dark theme, one capitalisation for names and tooltips. |

## AI assistance

Off until the user connects a model (any OpenAI-compatible endpoint, a local one included); every change it proposes arrives as a suggested change.

| Code | | Feature | Description |
|---|---|---|---|
| AI1 | 🚧 | The connection | Endpoint, token in the system's secret store, model; what is sent, and to whom, shown before it is sent. ✅ 0.10.0 on the desktop (each kind agreed to by connecting, and the editable prompts, ✅ 0.12.1): Settings › AI (endpoint, token in the system's keychain, model from the endpoint's list, Test Connection, Forget), each kind of request listed in Settings › AI with what it sends, where, and a switch, agreed to by connecting (another address or model asks again) and kept in the preferences (the agreement, never any text), each kind's prompt editable with its placeholders and Reset to Default, the request log with Show What Was Sent for this session's last requests, a project's own off switch. Still to come: the browser (the suite's AI layer has no provider for a web page yet). |
| AI2 | 🚧 | Explaining build errors | A plain-language explanation and a proposed fix. ✅ 0.10.0 on the desktop: Explain on build log cards and in the problem popup; a fix arrives as a suggested change by "AI (model)". Still to come: the browser, streaming the answer. |
| AI3 | 🚧 | Rewriting and continuing | Rewrite a selection (clearer, shorter, translated) keeping its LaTeX; optional completion as you type. ✅ 0.10.0 on the desktop (Ask AI to Change This… ✅ 0.12.0): Rewrite with AI (Clearer, Shorter, More Formal, Fix Grammar and Spelling, Translate to…) in the Edit menu and the text's context menu, the rewrite arriving as a suggested change by "AI (model)" on the selection and refused, saying why, when it dropped or altered a label, reference, citation, formula or command; Ask AI to Change This… in the same submenu, a change in the user's words ("flesh out these bullet points", the recent ones offered again) arriving the same way, allowed to add text and change markup but refused when it dropped or altered a label, reference, citation or formula, its own kind of request for consent; Settings › AI › Suggest the next words as I type (off by default), the words shown in grey after the caret, Tab takes them (Vim's Insert mode too). Still to come: the browser, and the words shown in the formatted view. |
| AI4 | 🚧 | Making LaTeX | Tables, equations, TikZ figures from a description; BibTeX entries from a title or DOI. ✅ 0.11.0 on the desktop: Insert › Make with AI… (a table, an equation, a TikZ diagram, a figure for one of the project's images), the model's LaTeX typeset after the project's preamble and previewed in the dialog before Insert puts it at the caret, selected, as one edit Undo takes out (with the packages it needs before `\begin{document}`; a suggested change while suggesting, or from an agent), retex's messages shown when it does not typeset with Ask Again with the Error; Insert › Add a Reference with AI… from a title or DOI, the entry a suggested change in a `.bib` under a "Drafted by AI (model): check it" line, the dialog saying models invent references. Still to come: the browser (the suite's AI layer has no provider for a web page yet). |
| AI5 | 🚧 | Asking and reviewing | Questions about the project; a review left as comments. ✅ 0.11.0 on the desktop: the rail's Ask AI panel (the open file or, by choice, the whole project's text files sent with numbered lines; `path:line` places in the answer are links to the line); Review This Section with AI (the section at the caret sent, each remark a comment by "AI (model)" on the words it quotes, remarks whose quote is not found dropped and counted). Still to come: the browser, streaming answers. |

## Open decisions

- **Licence:** `MIT OR Apache-2.0`, the suite's; retex is CC0, so either works.
- **Home of the repository:** on gitlab.com, mirrored to GitHub; joining the Craft suite means an RFC in craft-contrib.
- **Grammar in the browser:** Harper 3.2 adds about 10 MB to the web page (it needs an ML framework); Harper 0.54 is light (about 3.5 MB) but misses agreement errors; a fork without the framework would need hosting under storytold (E12).
- **Missing packages in the browser:** a TeX Live tree served with CORS, about 3–4 GB stripped of documentation and sources; GitLab and GitHub Pages are too small, an object store with CORS (Cloudflare R2, or a server of our own) fits (W8).
- **Translations:** the six catalogues were drafted with an AI model; a native speaker's review before they move to craft-i18n, which waits for TeXCraft joining the suite (A8).

## Further reading

[docs/retex-changes.md](retex-changes.md) (what TeXCraft uses of each retex release, and what it asked for),
[docs/design.md](design.md) (the design language), [docs/collaboration.md](collaboration.md) and
[docs/collaboration-roadmap.md](collaboration-roadmap.md) (comments and working together),
[docs/control-protocol.md](control-protocol.md) (automation), [docs/releasing.md](releasing.md).
