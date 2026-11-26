# UX review, 2026-11-26

A review of TeXCraft 0.12.2 (main at `86e66cf`) as a new user meets it: what gets in the way,
what is missing, and what works. Findings worth planning are rows in [ROADMAP.md](ROADMAP.md);
the quick wins were fixed with this review (marked **fixed**).

## Method

- **Seeing the app.** The main states were drawn offscreen with egui_kittest's wgpu renderer at
  1400×900 and 400×820 (a phone held upright), in the light and dark themes, in English, German
  and the pseudo locale (every label 40 % longer): the start page, the template gallery, the
  example article (`examples/article`) open beside its PDF, the formatted view, a build with two
  errors and its log, the Export menu, the History, Review, Outline and Ask AI sections, every
  Settings tab, the Open from GitLab or GitHub dialog (behind its unsaved-changes question), the
  keyboard shortcuts, and the phone's drawer and tabs. Each picture was read together with the
  accessibility tree of the same frame (role, name, value of every node). The pictures are not
  committed.
- **Journeys**, driven through the harness (commands, keys, clicks) and by reading the code paths
  behind them: (a) first start, a project from a template, a paragraph, a build, an error fixed;
  (b) an existing folder: finding a section, a cross-reference, a citation; (c) the formatted view
  as someone who does not know LaTeX; (d) review: comment, suggest, accept; (e) history: what
  changed, restore; (f) GitLab and GitHub; (g) export; (h) AI without a model connected; (i) the
  browser's code paths (library, downloads); (j) the keyboard and the screen reader's names for
  (a)–(c).
- **The user guide** (`docs/guide/*.html`) read against what the app shows.
- **Known items:** the open issues (#4, #5, #7, #12) and the earlier list of UX notes kept in the
  operator's notebook.

Not covered: a real phone, a real screen reader, a signed-in forge or a connected model (the
dialogs and messages were checked, not the round trips), and the browser build itself (its code
paths were read; the shared interface was drawn).

## Findings

Severity: **blocker** stops a core journey; **major** makes it much harder or misleads;
**minor** costs time or trust; **polish** is a rough edge.

### Blockers

1. **The start page did not fit a phone** — *fixed*. At 400 px wide the header's three buttons
   were laid out right to left past the page's left edge: New Project, the one thing a first
   visitor needs, was off screen (at x = −123), and the whole page was shifted so the template
   cards were cut off too. This is the web app's first screen on a phone (issue #12). Now, below
   600 px, the buttons go under the title, each as wide as the page, New Project first; a test
   checks every button lies inside a phone's window.
2. **Dialogs are wider than a phone.** Settings (720 px), the template gallery (866 px) and Open
   from GitLab or GitHub (460 px plus margins) are cut off on both sides at 400 px: the close
   button, Connect, Open and Create Project are out of reach. A phone user can start a project
   from the start page's cards but cannot open Settings or a repository. Proposal: dialogs no
   wider than the window less a margin; below a width, Settings shows its categories as a list
   that opens a page, the gallery drops its category column for a drop-down
   (ROADMAP **A12**).

### Major

3. **The PDF toolbar overlapped itself in longer languages** — *fixed*. In German (and the pseudo
   locale) "Exportieren" ran under the zoom buttons and the page arrows: two controls drawn on
   top of each other, neither readable. The pages and the zoom now move to a second row when the
   buttons before them leave no room, as they already did on a phone; a test checks it in the
   pseudo locale.
4. **Grammar underlined every indented line** — *fixed*. Harper's "Spaces" rule read a line end
   followed by an environment body's indent (the example article's abstract) as "There are 2
   spaces where there should be only one": a false mark at the start of each line of every
   indented environment, in the first document a new user opens. TeX reads any run of spaces as
   one, so runs of whitespace are no longer reported; a single space before punctuation still is.
5. **The formatted view does not keep its promise to readers who don't know LaTeX.** In the
   example article, cross-references show their keys (`eq:solution`, `eq:motion`) where the PDF
   says (2) and (1); the Introduction is a chip reading `sections/intro`, not its text; a formula
   is labelled `align` in the margin. The view reads as a map of the source rather than the
   document. Proposal: references with the number the last build gave them (the key on hover), an
   included file's text shown in place under a thin rule with its file name (editable there, or
   one click away), environment names in words (ROADMAP **F5**).
6. **Errors are reported where TeX noticed, not where they are.** An unclosed `itemize` in
   `sections/intro.tex` is reported at `main.tex:59` (`\end{document}`), and the editor marks only
   the other error. The message itself ("\begin{itemize} is never closed / add \end{itemize}
   where the itemize environment ends") is good; its place sends the user to the wrong file.
   Proposal: carry the `\begin`'s place with the message, link both (retex; ROADMAP **B10**).
7. **History versions are indistinguishable.** Every automatic version is "Saved with TeXCraft",
   told apart only by "1 file" or "3 files" and the time. To find the version before a change, a
   user opens them one by one. Proposal: describe each automatic version by what it changed (the
   files, and the sections touched), offer to name a version on save, and show unsaved changes as
   the list's first row (ROADMAP **C9**).
8. **Keyboard order is the reverse of the visual order in right-to-left rows.** The layout switch
   reads PDF Only, Editor Only, Editor and PDF (shown the other way round); the Files header reads
   Add Files…, New Folder…, New File…; the start page's header read Open from GitLab, Open Folder,
   New Project (*fixed* there, with the `right_aligned` helper that already exists for the status
   bar and the PDF toolbar). WCAG 2.4.3. Proposal: the same helper for the top bar's end (with the
   window buttons) and the panel headers (ROADMAP **A13**).
9. **The rail's Keyboard Shortcuts and Settings are announced as tabs.** They open dialogs; a
   screen reader user hears "tab, 7 of 8" and expects a panel. Proposal: buttons, kept apart from
   the section tabs (ROADMAP **A13**).
10. **Browser projects can vanish without warning.** Projects live in the site's storage, which a
    browser may evict under storage pressure unless the page asks for persistent storage; the web
    shell never asks, and a quota error on save has no message of its own. The guide warns about
    clearing site data, the app does not. Proposal: request persistent storage when the first
    project is saved, say on the start page whether it was granted, say clearly when a save fails
    for lack of space, and offer "Download all projects (.zip)" (ROADMAP **W13**).
11. **No example to learn from.** The repository ships an example article that exercises sections
    in several files, equations, cross-references, a list and a table, but the app does not
    offer it: a first start shows only empty templates. Proposal: "Open the Example Project" on the
    start page, built in like the templates (ROADMAP **A14**).

### Minor

12. **Template cards are titled "TeXCraft" six times.** For TeXCraft's own templates the card's
    heading is the publisher ("TeXCraft") and the kind ("Article", "Letter") is the muted
    subtitle; the licence is an SPDX id (`CC0-1.0`, `LPPL-1.3c`) and a green tick with no legend
    means "kept offline". Proposal: the kind as the heading, the source below, licences in words
    ("Free to use", "LaTeX Project Public License") with the id on hover, the tick explained or
    replaced by "Offline" (ROADMAP **A14**).
13. **The source toolbar hides most of itself at the default split.** At 1400 px with the side
    panel open the editor pane is about 545 px wide and shows only Heading, Bold and Italic; every
    other button is in More formatting. Proposal: smaller groups that give way one at a time, the
    formula and reference groups as single drop-downs (ROADMAP **E13**).
14. **The outline shows the shown file only.** In a project whose sections are in other files
    (the example), the Outline of `main.tex` lacks their headings, and that of `intro.tex` said
    "Sections of the shown file appear here." Proposal: the whole document's outline through
    `\input`/`\include`, the shown file's part marked (ROADMAP **E14**). Its empty text now says
    the file has no headings yet — *fixed*.
15. **The Settings dialog is shorter than its content.** At 1400×900 it is 400 px tall: the AI
    tab's Connect and "What may be sent" are below the fold of a scroll area that does not look
    scrollable. Proposal: the dialog as tall as the window allows (ROADMAP **A15**).
16. **Labels said the same thing two ways** — *fixed*. The layout switch said "Editor only" / "PDF
    only" where the menus and the shortcut list say "Editor Only" / "PDF Only"; the source
    toolbar said "Math", "Inline formula", "Cross-reference", "Bulleted list", "Paragraph" where
    the Insert menu and the formatted view's toolbar say "Formula", "Inline Formula",
    "Cross-Reference", "Bulleted List", "Paragraph Heading". Both now take the command's own
    label (and its translation by command id).
17. **Hand-written plurals** — *fixed*. The phone's PDF tab read "PDF, 1 errors"; the print
    summary and the preamble bar wrote their own singular. All three are plural rows now.
18. **The Export menu named other companies' products** — *fixed*. "Word Document (.docx)" and
    "PowerPoint Presentation (.pptx)" named the programs, not the files (standards/naming.md:
    name the file it writes). Now "DOCX Document (.docx)" and "PPTX Presentation (.pptx)",
    recorded in docs/feature-names.md.
19. **The phone's status bar overlaps itself.** "Compiled with 2 errors" is drawn under "Ln 1,
    Col 1 · LaTeX · 1 comment · 1 suggestion". Proposal: on a phone the build state only, the rest
    in the drawer or dropped (ROADMAP **A12**).
20. **The build log's button does not say its count.** The red "2" on the log button is drawn,
    not named: the node is "Build log". Proposal: "Build log, 2 errors" (ROADMAP **A15**).

### Polish

21. The start page's status bar said "Not compiled yet" (or the previous project's build) with no
    project open — *fixed*: the start page shows only messages there.
22. The guide said "the interface is in English for now" (Overview, Settings) while six catalogs
    have nearly 1,900 rows each — *fixed*. docs/releasing.md still expected "engine not available"
    in the browser and macOS and Windows from source only — *fixed*.
23. "Open the PDF in its own window" is offered on a phone, where it cannot help (ROADMAP **A15**).
24. The side panel changes width between sections (Review, Ask AI), and grows with a long
    empty-state text (a longer outline message pushed the PDF toolbar out of a 1280 px window in
    the pseudo locale), so the editor jumps (ROADMAP **A15**).
25. The AI token field's placeholder is "None"; it reads as a value. Better: "Not needed for a
    server on this computer" (ROADMAP **A15**).
26. The unsaved-changes question before opening from GitLab says "Save the changes to X?" without
    saying why it asks now (another project is about to open) (ROADMAP **A15**).
27. Build messages are the engine's lower-case fragments ("undefined command \undefinedmacro /
    check the spelling; …"), shown as titles. As sentences they would read better (retex)
    (ROADMAP **B10**).
28. In the dark theme the PDF stays a bright white page; an optional dimmed page would suit long
    sessions (ROADMAP **A15**).
29. Every tooltip and name capitalises as its command does ("Zoom Out") or as a sentence
    ("Previous page", "Fit width"); the toolbar's own labels mix the two (ROADMAP **A15**).

## The journeys

- **(a) First start to a fixed error.** The start page is clear on the desktop: a tagline, New
  Project as the one coloured button, the templates below. A project from a template opens beside
  an empty PDF pane that says what to do ("No PDF yet — compile the project to see it here"). With
  automatic builds on, the PDF follows typing; the status bar and the pane header say what the
  build is doing. An error shows as a red line in the editor, a red count on the log button and
  "Compiled with 2 errors" in the status bar; the log's cards explain it and link the line. Weak
  points: the log is reached by a small icon (finding 20), an error can point at the wrong file
  (6), and on a phone the first screen was broken (1).
- **(b) An existing project.** Open Folder, the file tree, Go to File (Ctrl+P), project search and
  the Outline work and are discoverable from the rail. The Outline is per file (14). In the
  source, Cross-Reference and Citation insert `\ref{label}` and `\cite{key}` with the placeholder
  selected and completion offered after a build; in the formatted view they open pickers of the
  project's labels and entries — the better path, not offered in the source.
- **(c) The formatted view without LaTeX.** Headings, paragraphs, lists, the abstract and typeset
  formulas read well; the toolbar shows the caret's style and has the expected Bold, Italic,
  lists, Link, Table and Figure. What breaks the illusion is listed in finding 5; also Suggest
  mode is read-only there (said in the guide, ROADMAP F3).
- **(d) Review.** A comment on a selection and a suggestion show as cards with the quoted passage,
  the author and Accept/Reject; the status bar counts them. In the split layout at 1400 px the
  margin gives way to the Review panel, which works but is less tied to the text.
- **(e) History.** The panel says when there is no history yet and why; restores ask first and
  come back as unsaved changes. The list itself is hard to use (7).
- **(f) GitLab and GitHub.** The Open dialog asks for the service, the address and the branch,
  suggests projects once signed in, and a token can be used instead; an unsaved project is asked
  about first. Not exercised against a server.
- **(g) Export.** The Export menu is in the PDF pane, ordered for the document (slides first for
  beamer), and the status line says when it starts and ends. It exports while the build has
  errors without saying so.
- **(h) AI.** Without a model, every entry says "Connect a model first (Settings › AI)." and the
  Ask AI section has Open Settings; Settings › AI explains what is sent before anything is. Clear
  and trustworthy; the tab's height hides its main button (15).
- **(i) The browser.** The start page lists "Projects in this browser" with Delete (confirmed),
  names are asked instead of folders, exports download. Storage durability is the gap (10).
- **(j) Keyboard and screen reader.** F6 regions, F10 menus and dialogs that take and give back
  the keyboard are well covered by tests. F1 now works from the start page (it opened the guide
  in the harness). Gaps: reversed orders (8), roles (9), unnamed counts (20).

## What works well

- One coloured action per screen; quiet, consistent chrome; both themes legible (the contrast
  test covers every token pair).
- Empty states say what to do: no PDF yet, no history yet (and why), no comments (with how to add
  one), no model connected (with a button to Settings).
- Errors in words, with help and a link to the line; the build's files are listed with Save As….
- Destructive actions are confirmed and recoverable: deleting a browser project, restoring a
  version (which comes back as unsaved changes), unsaved changes before leaving.
- AI is opt-in, explains what is sent and where before connecting, and every change it proposes
  arrives as a suggestion.
- Every action is a command, so menus, shortcuts, the shortcut editor and agents stay in step.
- The narrow layout's drawer and Source/PDF tabs work, and the editor at a phone's width is
  usable.

## Screens and states, for the record

Drawn at 1400×900 and 400×820, light and dark, English, German and pseudo: start page, gallery,
project open, built, formatted view, build with errors, build log, Export menu, History, Review,
Settings (each tab), Open from GitLab or GitHub, Ask AI, Outline, keyboard shortcuts; on the phone
also the drawer and the PDF tab. After the fixes: the phone's start page and the German PDF
toolbar again.
