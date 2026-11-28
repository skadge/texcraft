# UX review 2: consistency, 2026-11-28

A second review of TeXCraft (main at `6ef4ec3`, 0.24.0), after the one of 2026-11-26
([ux-review.md](ux-review.md)). This one asks one question across the whole app: where the same
kind of thing is shown or done in two places, is it shown or done the same way? A setting, a
dialog's header and footer, a state's badge, an empty section, a message, a label's
capitalisation. Where it is not, the fix is preferably a shared helper, so that the next dialog or
setting is consistent by construction rather than by care. The newest screens (Import a Project,
Settings › Accounts, New Project's Where to Keep It; docs/design/) set the house style. The
patterns this review settled are written down in [design.md](design.md#patterns).

Each finding says where, what, why it matters, and what was done: **fixed** (with the commit's
subject), **proposed** (needs a decision or more work), or **left** (with the reason).

## Method

- **Pictures.** Every dialog, panel and Settings section was drawn offscreen with egui_kittest's
  wgpu renderer and the real interface fonts (craft-fonts), at 1280×860 and 390×844 (a phone held
  upright), light and dark: about 45 states, some 180 pictures, each read with the accessibility tree of the
  same frame (role, name, rect, focus). Before and after for the main fixes. The pictures are not
  committed.
- **Code.** Every place that draws a dialog header, a footer button, a setting, a badge, an empty
  state, a status message, or opens Settings was found by searching the source, and the ones that
  do the same job were compared.
- **Tests** for the rules that can be stated (below).

Not covered: a real screen reader, the browser build itself (its code paths were read), the
formatted view's internals (another agent is working there).

## Known papercuts (the operator's list)

1. **The PDF toolbar offered Save PDF twice** — *fixed* ("PDF pane: one way to the PDF, and
   controls named as their commands"). A download icon beside the Export pill ran `build.save_pdf`, which
   is also the Export menu's first entry (Save PDF As…, Download PDF in the browser). The icon is
   gone; the command, File › Save PDF As… and the menu entry stay, and the entry now shows the
   command's shortcut when one is set. The keyboard path test, the browser's Download PDF test and the guide
   (Building) were updated; nothing else (the command palette, the phone layout, tooltips)
   referred to the icon. Export stays usable while only the PDF can be had, as before.
2. **Settings stopped short of the window's bottom** — *fixed* ("Settings: as tall as the window in
   every section, the shown section selected"). The A12/A15 work computed a height from the window, but the box is a
   centred modal sized from its previous frame: the room below its top limited the scroll area,
   which limited the box, which kept its top where it was — a fixed point at the first section's
   height (358 points at 1280×860, ending near y = 630, Accounts' second card cut off). The body
   now takes its room explicitly, so Settings is as tall as the window allows in every section and
   no longer changes height between sections (it did: Keyboard and Review were half as tall as
   Editor). A test checks Settings grows with the window and fills most of a 860-point one.
3. **Accounts' badges were faint in the dark theme** — *fixed* ("Widgets: one state badge seen in
   both themes, one empty state for panes and panels"). The badge mixed a `Color32` with alpha, whose channels egui keeps
   premultiplied, as if it were the colour: in the light theme that darkened white by a fifth
   (seen as grey), in the dark theme it nearly matched the surface. The tint is now mixed from the
   opaque colours with a strength per theme (`Tokens::tint_alpha`, stronger on dark surfaces), and
   the badge is a shared widget (`widgets::state_badge`). A test checks text keeps 4.5:1 on every
   tint and the dark tint stands as far from its surface as the light one.
4. **The side panel's width changed between sections** (A15 leftover) — *fixed* ("Side panel:
   the width it was given, in every section"). In a longer language (the pseudo locale, as German)
   the Review section's filter row widened the panel from 304 to 419 points, and it stayed wide
   for every later section: the editor moved. The panel's content is now drawn in the width the
   panel was dragged to, wrapping or cut at its edge. A test walks the six sections in the pseudo
   locale.
5. **A dimmed PDF page in the dark theme** (A15 leftover) — *fixed* ("Settings › Appearance: Dim
   the PDF in the dark theme"). Off by default (the PDF looks as it will print); on, the pages are
   multiplied by a lower white (`Tokens::page_dim`) while the theme is dark. Presentation mode
   stays white. A setting row, a command (`view.dim_pdf`, in All Commands and for agents), the
   guide.
6. **One capitalisation for names and tooltips** (A15 leftover) — *fixed for the PDF pane*,
   *rule written* (with 1). The rule (design.md, Words): what
   runs a command (buttons, icon buttons, menu entries, tabs, dialog titles) in title case, as the
   command is named; settings, options in a list, help and explanatory tooltips in sentence case.
   The PDF pane broke it most visibly — "Previous page", "Next page", "Build log", "Fit width" and
   "Fit page" beside "Zoom Out" and "Back to Fit Width", and the commands "Build Log", "Fit Width"
   — and now says "Previous Page", "Next Page", "Build Log", "Fit Width", "Fit Page" everywhere
   (catalog keys renamed, translations kept). *Left*: a sweep of every other tooltip; the other
   panels already follow the rule (their buttons take their command's label since the first
   review).

## Dialogs

7. **Close was in four places, or nowhere** — *fixed* ("Dialogs: one header with Close, one
   primary and secondary button"). Settings, the gallery, Import, Add a Connection and Where to Keep It drew
   their own header (five copies of the same row, two of them right to left), with × at the end;
   the twenty older dialogs (prompts, questions, About, Save to GitLab/GitHub, the server
   registration, the token, commit, conflicts, merge request, What's New, Missing Packages, Drop,
   Make with AI, Add a Reference, Ask AI to Change This, Go to File, All Commands, the pickers) had
   no × at all in a wide window — only Escape, which a pointer user does not see. Now
   `dialogs::header` draws Back (when the dialog has pages), the title, anything kept beside it
   (Settings' and the gallery's search) and Close, and `dialogs::title` gives every one-page
   dialog the same × at the end of its title's row. The plain dialogs' × is made after their
   controls, so the keyboard, given to a dialog's first control when it opens, still starts on
   the field or the action, not on Close (a test of the earlier keyboard paths confirms it). A
   test checks that every kind of dialog has Close ending its title's row.
8. **Footer buttons were hand-made in fourteen files** — *fixed* (with 7). Each primary was its own `Button` with the accent: 88, 100, 120, 140 or 160
   points wide, 30 or 32 tall, some in the strong face and some not, and the browser's text colour
   on the accent in two. `dialogs::primary` (the accent, or `error` for a destructive one) and
   `dialogs::secondary` replace them all; the order (primary right-most, Cancel to its left, a
   discarding action alone at the left) was already kept everywhere and is now checked by a test
   for each kind of dialog.
9. **Two copies of the footer's progress** — *fixed* (with 7). Import's and Where to Keep
   It's moving bar and phase were the same code; `dialogs::progress`.
10. **The citation, cross-reference and figure pickers were not sheets** — *fixed* (with 7). The formatted view's pickers drew a bare modal 520 points wide: on a
    phone they ran off the screen, with no Close. They now go through `sheet::show` and
    `dialogs::title`, like Go to File.
11. **About was titled "TeXCraft"** — *fixed* (with 7). Every other dialog is titled as the entry
    that opens it; About now reads "About TeXCraft", the name under the icon.
12. **Keyboard Shortcuts** has no × and its OK is not the accent — *left*: the dialog is
    craft-keymap's (storytold/craft-libs); its Cancel reverts the edits, so a × must be its own.
    *Proposed* there: the same header and a primary OK.
13. **Older list-style dialogs vs the card grids** — *left as they are*. Save Project to
    GitLab/GitHub, the server registration, commit, conflicts and the token are forms, not choices
    of a place: cards would not help. The one older dialog that is a choice, Missing Packages
    (three big buttons, the first in the accent, each with a line under it), reads well and its
    answers carry their consequences; *proposed*: Import's list rows (icon, title, line) if it
    gains a fourth answer.
14. **Menu labels and ellipses** — checked: entries that open a dialog asking more end with …
    (New File…, Save PDF As…, Settings…, Import a Project…), those that act do not. *Proposed*:
    Help › Keyboard Shortcuts opens an editor of the shortcuts and could take the ellipsis as
    Settings… does; left because the rail's button and the tests name it, and the gain is small.

## Settings

15. **Settings › AI's switches were not setting rows** — *fixed* ("Settings › AI: its switches
    are setting rows"). Every other section's on-or-off setting is a row (name and help on the
    left, the checkbox on the right, named after the row); AI's were checkboxes carrying their
    own words with the help under them (Use AI features, Suggest the next words as I type, Use AI
    in this project, and each of the nine kinds of request, whose "Not agreed to yet" was a
    coloured word beside it). They are rows now, the agreement said in the help. A test checks
    that every checkbox in every section is a row with its name shown. *Left*: the endpoint,
    token and model are fields with their labels above, as forms are; Prompts and the request
    log stay folding sections.
16. **Settings › AI's Connect was a plain button** — *fixed* (with 15). Accounts' Add a
    Connection… is the accent, the one coloured action of its section; AI's Connect, the action
    that section is for, now is too.
17. **Quick choices without a way to their settings** — *fixed* ("Quick menus end with their
    settings"). The Compile button's menu has four build settings (and a comment saying
    "Settings › Building has the print options") but no way there; the status bar's spelling
    menu has the language but not the comments or grammar. Both now end with "Build Settings…"
    and "Spelling Settings…", which open Settings at Building, and at the spelling settings found
    by the search (`settings::menu_link`). A test follows both.
18. **Ask AI's Open Settings opened whatever section was last** — *fixed* (with 17). It set the
    dialog directly, not through `app.settings`, so it showed the last section seen rather than
    AI (and bypassed the command, AGENTS.md rule 14). It runs `app.settings` with `tab: ai` now.
    The other Settings links (the tips, Missing Packages) already named their section.
19. **The section tabs did not say which is shown** — *fixed* (with 2). The tab list's tabs had no selected state for a screen reader (the
    highlight was paint only); they have it now. While the search's results are shown, no tab is
    highlighted (Appearance was, though its settings were not what was shown).
20. **Appearance's order** — *fixed* (with 5): Theme, Dim the PDF (beside the theme it depends
    on), Interface language (second: it is what a new user looks for), then System title bar,
    Check for updates and Show tips. Other sections already put the most used first.
21. **Where each feature's settings live** — checked, consistent: build, spelling, grammar, the
    editor, review identity, AI and connections each have one home in Settings, and every quick
    choice elsewhere (Compile menu, status bar, Missing Packages' question, the PDF's zoom menu
    for its own view) changes the same setting through the same command. Two exceptions,
    *left*: the browser's Package server and the desktop's Missing packages share a place in
    Building (only one is shown in each build), and the formatted view's options live in its
    toolbar (another agent's area; *proposed* to give it a "… Settings…" entry once it has
    settings of its own). Export has no options.
22. **Same kind of choice, same control** — checked: on/off is a checkbox everywhere (now AI's
    too); two or three short choices a segmented control (Theme, key bindings, PDF for); more, or
    long names, a drop-down (language, engine, missing packages, package server, PDF/X); a number
    a slider with its value. Engine (three choices, one long) is a drop-down by the "long names"
    rule. Written down in design.md.

## States, messages and empty states

23. **Empty side-panel sections had three shapes** — *fixed* (with 3).
    Review said it centred, in bold with a line under it; the outline in small grey text at the
    left; History in small grey text under its buttons. All three now use `widgets::panel_empty`:
    what is missing in bold ("No headings yet", "No history yet", "No comments or suggestions")
    and what to do under it. The panes' empty states (no file open, no PDF yet) were two copies of
    one shape: `widgets::pane_empty`.
24. **Status messages** — checked: 76 messages set from the window; 62 are sentences ending in a
    full stop, three were not ("Exported to {path}", "Comment added", "Moved to the trash: …") and
    now are (*fixed* with 6); the rest end with an engine's `{error}`, which
    brings its own stop, or are a focus announcement ("Menu bar"). Failures start "Could not …" and say what to do; a
    build's state is "Compiled", "Compiled with 2 errors", "Failed: …"; AI's "No answer: …", "No
    rewrite: …". Errors show in the status bar in the error colour with its icon, and in a dialog
    under the field they concern (Import, Add a Connection, Save to…): one place each, the same
    in each. *Left*: two engine messages end "{e}." where four end "{e}" (Could not write …), not
    seen as such by users, in the engine crate.
25. **Badges** — Accounts' was the only words-and-icon badge; the count badges (the build log's,
    the rail's) are a different thing (a number on an icon) and stay. The Where to Keep It cards
    say "· Default" in their line, as the design asks (a card's line, not a badge).

## Tests

In `crates/ui-egui/tests/it/consistency.rs`: every kind of dialog has Close ending its title's row
and its primary action right-most in its footer; Settings grows with the window and fills it;
the side panel keeps its width across its six sections in the pseudo locale; a quick menu's last
entry opens its settings (Compile → Building; spelling → the spelling settings, and only those);
every checkbox in every Settings section is a setting row with its name shown. In `contrast.rs`:
the badge tints keep text at 4.5:1 and are as visible in the dark theme as in the light. The panel's
width, the height, the tint and the selected tab were each seen to fail before their fix. The
existing keyboard and accessibility tests (keyboard_paths, reading_order, phone_dialogs, regions)
pass, with the PDF toolbar's stop list less the removed icon.

## What works well

- The newest dialogs set a clear house style, and it took little to bring the older ones to it:
  their bodies are already short forms with one action.
- One command behind every control kept the quick menus and Settings in step: the links added
  here only had to name the section.
- The phone's sheets already gave every dialog Close and a bottom bar; the desktop now matches.
