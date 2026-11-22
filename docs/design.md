# TeXCraft design language

> **Last updated:** 2026-11-22 · **Change:** first version

TeXCraft keeps the arrangement users of online LaTeX editors know:
- files on the left, the source in the middle, the PDF on the right;
- the build button above the PDF, the log in the PDF's place;
- a rail of sections at the far left.

The drawing is our own. The aim is a calm, precise tool: a quiet frame around two documents (the source and the paper), with colour reserved for meaning (the action, errors, warnings, success).

Every value below is a token in `crates/ui-egui/src/theme.rs`. Widgets never use a literal colour or size.

## Principles

1. **The documents are the brightest things on screen.** The frame (top bar, rail, panels, toolbars) sits one step back from the editor and the paper, separated by hairlines rather than boxes.
2. **One accent.** It marks the primary action (Compile), the current selection and keyboard focus. Errors, warnings and success have their own colours and always come with an icon or a word.
3. **Quiet until needed.** Row actions appear on hover and focus. Counts appear only when not zero. A panel with nothing to show says what would appear there and how to get it.
4. **Exact alignment.** Everything sits on a 4 px grid, and toolbars share one height so their baselines line up across the window.
5. **Honest controls.** A feature that does not work yet is either absent or shown disabled with the reason in its tooltip. It never fakes success.

## Colour

Two themes, light and dark, following the system by default. Neutrals are slightly cool and low in chroma.

| Token | Light | Dark | Use |
|---|---|---|---|
| `frame` | `#F4F5F7` | `#17191C` | Top bar, rail, side panel, toolbars |
| `surface` | `#FFFFFF` | `#1E2024` | Editor, dialogs, cards |
| `canvas` | `#E6E8EC` | `#2A2D32` | Behind the PDF pages |
| `raised` | `#FFFFFF` | `#25282D` | Menus, popovers, tooltips |
| `separator` | `#E1E4E8` | `#2C3036` | Hairlines between regions (decorative) |
| `control_border` | `#727985` | `#7D8490` | Outlines of fields and buttons (3:1) |
| `text` | `#1B1E23` | `#E7E9EC` | Body text |
| `text_muted` | `#5B626C` | `#A2A8B1` | Secondary text, inactive icons |
| `accent` | `#2557D6` | `#7AA7FF` | Primary action, selection, focus |
| `on_accent` | `#FFFFFF` | `#0D1830` | Text on the accent |
| `accent_soft` | accent at 10 % | accent at 16 % | Selected rows, active toggles |
| `error` | `#B0261A` | `#FF8A80` | Errors |
| `warning` | `#7F5200` | `#F2C45A` | Warnings |
| `success` | `#17683B` | `#6FD39A` | A clean build |

Hover is the text colour at 6 % over the surface, and pressed is 10 %. Selection in text is the accent at 22 % (light) or 30 % (dark).

Contrast is checked by a test:
- Text tokens reach 4.5:1 on `frame`, `surface` and `raised`.
- `control_border` and the accent reach 3:1.
- `separator` is decorative and exempt.

Syntax colours are tuned per theme and reach 4.5:1 on `surface`.

## Type

| Role | Face | Size / line | Weight |
|---|---|---|---|
| Interface | Inter | 13 / 18 px | Regular; SemiBold for titles |
| Small labels, counts, status bar | Inter | 12 / 16 px | Regular |
| Dialog titles | Inter | 16 / 22 px | SemiBold |
| Source | Monospace | 13.5 / 21 px | Regular |

Faces come from storytold/craft-fonts through the optional `CRAFT_FONTS_DIR`: Inter from its Latin catalog, and Liberation Mono for code until craft-fonts offers a face drawn for code (to be proposed there). Without the checkout, egui's bundled faces are used and every size stays the same.

## Space and shape

- The grid unit is 4 px. Gaps are 4, 8, 12, 16 or 24 px.
- Radii: 6 px for controls, 8 px for menus, cards and dialogs, fully round for badges and the build button.
- Fixed heights:

  | Element | Height |
  |---|---|
  | Top bar | 44 px |
  | Pane headers and toolbars | 36 px |
  | Tabs | 32 px |
  | Status bar | 24 px |
  | Rail | 48 px wide, 36 px buttons |
  | File rows | 26 px |

- Icons are 16 px in toolbars and rows and 18 px on the rail. All are from the Lucide set, drawn with a 1.75 px stroke and tinted with `text_muted`, or with `text` when active.
- Menus and popovers float on `raised` with a 1 px `separator` border and a soft shadow (0 6 20 px, black at 14 % light, 40 % dark).

## Regions

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ◆ File Edit Insert View Build Help        project ▾        [⊟ ⊞ ⊡]           │ top bar 44
├──┬───────────────┬─────────────────────────────┬─────────────────────────────┤
│▣ │ FILES     + ⋯ │ main.tex  intro.tex ×       │ (▶ Compile ▾)  ≣ 2   ⤓   │ 36
│  │ ▾ sections    │ Source │Formatted   B I ∑ … │      ‹ 1 / 3 ›   − 100% + │
│◫ │   intro.tex   ├─────────────────────────────┼─────────────────────────────┤
│  │ main.tex  ●   │  1 \documentclass{article}  │   ┌───────────┐             │
│  │ refs.bib      │  2 \usepackage{amsmath}     │   │  paper    │             │
│  │───────────────│  …                          │   │           │             │
│  │ OUTLINE       │                             │   └───────────┘             │
│⚙ │ 1 Introduction│                             │                             │
├──┴───────────────┴─────────────────────────────┴─────────────────────────────┤
│ ● Compiled 0.4 s · 0 errors        Ln 12, Col 4 · LaTeX · retex             │ status 24
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Top bar.**
  - On the left: the app mark and the menus.
  - In the centre: the project name, with its menu (rename, download source, copy) and an unsaved-changes dot.
  - On the right: the layout switch (editor and PDF, editor only, PDF only).
  - Sharing and history buttons appear when those features exist (plan, phase 6).
- **Rail.** Files and Outline at the top; Settings and Help at the bottom.
  - The active section has an accent bar on its inner edge.
  - Clicking the active section hides the side panel.
- **Side panel.** Files, as a tree with folders that fold, a type icon per file, the main document marked, and row actions on hover. The outline sits below, in a resizable split.
- **Editor pane.**
  - Tabs, then the editor toolbar (Source/Formatted switch, formatting buttons).
  - The editor has a gutter with line numbers and markers for errors and warnings.
  - The current line has a soft band.
- **Preview pane.**
  - A round Compile button with a menu segment: automatic build, build options, clearing build files.
  - The log button with its count, and Download.
  - Page navigation and zoom on the right.
  - Pages sit on `canvas` with a soft shadow.
- **Log.** It replaces the pages and has a "Back to PDF" button.
  - Filter tabs with counts (All, Errors, Warnings, Notes).
  - One card per message: an icon and its severity in words, the message, the place as a link, and help when retex gives it.
  - The raw log sits at the end.
- **Status bar.**
  - On the left: the build state (with a small progress ring while building) and its last result.
  - On the right: the caret's line and column, and the engine.
- **Dividers.** 1 px lines with an 8 px grab area. The dividers next to the editor have a small collapse handle at the midpoint. The divider between editor and PDF will carry the two navigation buttons (plan, phase 4).

## Motion

Hover and press colours change over 80 ms, and panels fold over 140 ms. Nothing moves when the system asks for reduced motion.

## Accessibility

Each region is reachable with F6. Every control has a translated name starting with its visible label. Focus is a 2 px accent ring outside the control. Build results are announced through a live region. Colour is never the only signal.
