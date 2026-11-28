# Import a Project

> **Status:** built (next release; see the ROADMAP's C11 row for what is left) · **Roadmap:** C11 (docs/ROADMAP.md) · **Written:** 2026-11-26

One dialog brings an existing project into TeXCraft from wherever it lives: GitLab, GitHub,
Overleaf (through its git clone address), a self-managed GitLab server, any git address over
HTTPS, a folder, or a zip file. It replaces three entry points that each do part of this today
(Open Folder…, Open from GitLab or GitHub…, and the start page's drop of a zip) with one place to
look, without taking away the shortcuts people already use.

This design is our own. It draws on general interaction-design practice, WAI-ARIA's patterns and
docs/design.md; no other product's dialog was looked at (standards/clean-room.md).

## Goals

1. **One place to look.** "I have a project somewhere else" has one answer: Import a Project….
2. **No user guide needed.** Each source asks only what it must, with a default for everything
   else, checks what is typed as it is typed, and says what is wrong and what to do in words.
3. **The project works at once.** After a git import the project is open, linked to where it came
   from, and Sync (`project.sync`, in progress on `feat/autosave-sync`) works with no further
   setup: `origin` is set, the branch follows it, and the credential is kept.
4. **Honest in the browser.** Sources the browser cannot reach stay visible, marked unavailable,
   with the reason and a way out, never a button that fails later (AGENTS.md rule 15).
5. **Quiet.** Icons with terse labels, little text, the accent only on the one action.

Not goals: adding a remote to an open project (C10), importing a single file into the open project
(Add Files…, Import a Zip File stay as they are), cloud storage (C6).

## Where it opens from

| Entry | Today | With this design |
|---|---|---|
| Start page header | New Project · Open Folder… (desktop) · Open from GitLab or GitHub… | New Project · **Import…** · Open Folder… (desktop). Open from GitLab or GitHub… goes: it is a card now. |
| File menu | Open Project Folder… (Cmd+O) · Open from GitLab or GitHub… | Open Project Folder… (Cmd+O, unchanged) · **Import a Project…** (Cmd+Shift+O). Open from GitLab or GitHub… leaves the menu. |
| `forge.open` (the UI command) | opens the forge dialog | opens Import a Project at the GitLab or GitHub card, the address filled in. The id stays; its old label stays as a hidden search alias in All Commands (standards/naming.md rule 6). |
| A link proposing a server (`?gitlab=…`) | Open dialog after the question | Import a Project at the GitLab Server card, the project filled in. |
| A folder or zip dropped on the start page | opens it / imports it | unchanged: a drop already says where it comes from. |
| File › Import a Zip File (`file.unzip`) | unzips into the open project | unchanged: it adds files to this project; the Zip File card makes a new one. |
| Empty start page (first run) | the example's hint names Open Folder… | the hint names Import… instead ("…or Import… for a project you have elsewhere"). |

Open Folder stays a button of its own on the desktop because reopening one's own folder is the
everyday case, and a dialog in between would slow it down. Its card in the dialog is there so that
"where is my project?" has one complete answer.

## The dialog

- **Size:** 720 px wide at most (docs/design.md's dialogs), as tall as its content; in a narrow
  window, a sheet (`sheet.rs`). The width does not change between pages, so nothing jumps.
- **Pages:** the grid of sources first; choosing a card replaces the grid with that source's page
  in the same dialog (no dialog over a dialog). The title becomes "Import from GitLab" with a Back
  arrow before it. Back returns to the grid with focus on the card that was chosen.
- **Footer** of a source page: Cancel and the one primary button, **Import** (accent), on the
  right; the primary button is off until the page is complete, and the field that is not says
  why under itself. The grid page has no footer: a card is the action, and Close is the title
  bar's ×.
- **Escape** closes the innermost thing open: a drop-down or suggestion list, else the dialog
  (nothing typed is kept, nothing was done). Back is Alt+Left (Cmd+[ on macOS) and the arrow
  button. While an import runs, Escape asks nothing and does nothing; Cancel stops the import.

## The grid of sources

Two groups, each with a small muted heading, cards flowing in up to four columns:

**Online**

| Card | Icon (ours) | Title | Line (state) |
|---|---|---|---|
| GitLab | `git-host` (below) | GitLab | "Signed in as ada" · "Sign in or use a token" |
| GitHub | `git-host` | GitHub | "Signed in as ada" · "Sign in or use a token" · browser: "Personal access token" |
| Overleaf | `git-host` | Overleaf | "Clone address and token" · browser: "Desktop app only" |
| GitLab Server | `server` | GitLab Server | "git.example.org" (one added) · "2 servers" · "Add your server" |
| Git Address | `link` | Git Address | "Any HTTPS address" · browser: "Desktop app only" |

**On this computer** (on the desktop) / **On this device** (in the browser)

| Card | Icon | Title | Line |
|---|---|---|---|
| Folder | `folder-open` | Folder | "Opens it where it is" · browser: "Copied into this browser" |
| Zip File | `file-archive` | Zip File | "Unzipped into a new folder" · browser: "Unzipped into this browser" |

- **Card:** 160 × 76 px at four columns (12 px gaps), `surface` with a hairline border and an 8 px
  radius like the template cards; the icon 18 px in `accent` at the top left, the title
  (SemiBold 13 px) and the line (12 px, `text_muted`) under it. Hover is the usual 6 % tint;
  focus the 2 px accent ring. The signed-in line has a small `circle-check` in `success`
  before it (so the state is not colour alone: the words say it).
- **Unavailable card** (browser: Overleaf, Git Address): the icon and title in `text_muted`, the
  border dashed, the line "Desktop app only" with an `info` icon. It stays focusable and
  clickable: choosing it opens its page, which explains (below). A disabled-looking card that
  does nothing would be a dead end.
- **Order** is the reading order and the keyboard order: the most common sources first.
- **Long languages** (German +40 %): the title and the line may each take two lines; a row is as
  tall as its tallest card, so the grid stays aligned. When a card would be narrower than 140 px,
  the grid drops a column (4 → 3 → 2), then becomes the phone's list.
- **First run / nothing set up:** every card is there; the lines say what each will ask for
  ("Sign in or use a token", "Add your server"). Nothing is hidden because it is not set up yet.

## Each source, step by step

Common to the git sources on the desktop:

- **Into** (where the project lands): one row, the folder's full path and **Change…**. Default:
  the parent folder used last for a new project or import (the Documents folder the first time)
  and the repository's name, e.g. `~/Documents/paper`. As the source is chosen, the name follows
  it until the user edits the path. Checked in place: a folder that exists and is not empty says
  "A folder named paper is there already." with **Use paper-2** as a button; a parent that cannot
  be written says so.
- **Branch:** a drop-down, the default branch chosen and marked "(default)", protected ones
  "(protected)", "Another branch…" to type one (the existing `forge_pick` drop-down). A repository
  with one branch shows its name as text, not a drop-down.
- **Import** clones: an empty repository is made in the folder, the branch is fetched with gitwire
  (`texcraft-gitwire`'s fetch), checked out, `origin` set to the HTTPS address with
  `branch.<name>.remote/merge` so the branch follows it. The credential used is kept for the
  remote's server (the keychain, as sign-ins are). Then the project opens.
- **Progress** takes the footer's place: a thin bar and one line, "Receiving 1.2 of 3.4 MB",
  "Writing 184 files", with **Cancel**. Cancel stops it and removes the folder TeXCraft made
  (only that folder, and only if it made it). The line is a live region, said at each phase, not
  each byte.
- **Feedback (operator, #49):** Import closes the dialog at once and a card for the project
  appears at the head of the start page's recent projects: a spinner, "Importing…" and the
  phase (a live region), Cancel. A failure stays on the card in words, with Try Again (the page
  as it was) and Dismiss. If another project was opened meanwhile, the import does not replace
  it: it joins the recent projects. The footer's progress stays for the dialog opened again
  while a clone runs.
- **After:** the dialog closes; the project is open with its main document; the status line says
  "Imported paper (main) into ~/Documents/paper"; the top bar shows the branch and **Sync**. If no
  main document is obvious (several files with `\documentclass`), the file tree marks none and the
  usual Set as Main Document applies; nothing asks.
- **Errors** stay on the page, under the field they concern (or above the footer for the
  connection), in `error` with the `circle-exclamation-point` icon, and are announced:

  | What happened | Said |
  |---|---|
  | 401 | "{host} did not accept the token. Check it is a token, not your password, and that it has not expired." |
  | 403 | "This account cannot read {project}." (with the permission needed, as forge errors do today) |
  | 404 / not a repository | "No git repository at this address. Copy the address again from the project's page." |
  | Network | "Could not reach {host}. Check the connection, then try again." · **Try Again** |
  | Too large | "This repository is {size}; TeXCraft imports up to 200 MB." |
  | Folder | "Could not write to {folder}: {reason}." |

In the browser a git source cannot clone (no git protocol reaches the forges from a page); GitLab
and GitHub come through their web APIs into the library, as `forge.open` does today; the project
is kept under the repository's name (asked only if a project of that name exists), and Commit and
Update (or Sync once it covers browser projects) work as now.

### GitLab, GitHub

1. **Account.** Signed in: "Signed in as Ada Lovelace (ada)" with a small **Switch** link. Not
   signed in: **Sign In with GitLab** (primary within the row, not the accent: Import keeps it),
   and "Use a token instead" (the existing `sign_in_or_token`). GitHub in the browser shows the
   token field directly, with one line why.
2. **Project.** The existing combo box: type to search the account's projects, a list under the
   field (path, Private/Internal/Public in words, "active 3 days ago"), Down/Up/Enter, a pasted
   address works too. Focus starts here when signed in.
3. **Branch.** Filled from the project, default chosen.
4. **Into** (desktop).
5. **Import.**

Defaults: gitlab.com or github.com; the default branch; `~/Documents/<name>`. An address on the
other service ("this is a GitHub address") offers **Switch to GitHub** under the field.

### Overleaf

1. **Clone address.** Paste the address the service gives for git. A whole `git clone …` command
   pasted is accepted (the command word is dropped). Checked as typed: HTTPS, on the service's git
   host; anything else says "This is not an Overleaf git address. It starts with https://git."
   (the exact host to be confirmed, see open questions).
2. **Git token.** A password field with Show. One line under it: "Made in your Overleaf account
   settings, under git. Your password does not work here." **Remember on this computer** is
   ticked (the keychain), so Sync can push later.
3. **Into** (desktop). The name is the project's id until the clone reads the main file's
   `\title`; the user can type a better one before Import.
4. **Import.** One branch: no drop-down. The address and token are checked first
   (`git.check_address`), then cloned.

After: Sync pushes and pulls with the same token. The service rejects some things git allows (for
example, other branches); its message comes back in its own words after "Overleaf refused the
change:".

In the browser the card's page says: "Overleaf's git server does not answer web pages, so the
browser cannot reach it." and offers two ways out: **Get the Desktop App** (the download page) and
**Import a Zip File…** with the hint "Download the project as a zip on Overleaf, then import it
here."

### GitLab Server

1. **Server.** A drop-down of the servers added (Settings › Accounts), each with its state
   ("signed in as ada", "not signed in"); "Add a Server…" last. With none added, the page opens on
   the add step instead: the Server address field and **Check Server**, then the registration
   (`forge_server.rs`'s steps, shown inside this page, with Back to the server field).
2. Then as GitLab: account, project, branch, into.

GitHub Enterprise servers added in Settings are listed here too, named as such, since the flow is
the same (see open questions for the card's title).

In the browser: available; a server that does not answer web pages is said at the check, as today
("ask its administrator, or use the desktop app").

### Git Address

1. **Address.** Any `https://` address. Checked once typing pauses (half a second) or the field is
   left, anonymously (`git.check_address`, git's ref advertisement): the line under the field says
   "Checking…" (a small spinner), then one of:
   - "Public repository · 3 branches" (`success` tick): the branch drop-down appears.
   - "This repository needs signing in." The **User name** and **Password or token** fields appear
     below (and only then), with Remember on this computer.
   - an error (the table above). An SSH address (`git@host:owner/name.git`) says "TeXCraft uses
     git over HTTPS." with **Use https://host/owner/name.git**. Plain `http://` is refused but on
     this computer.
   - an address on gitlab.com, github.com or a server added works, using the sign-in already
     there; a line offers "This is a GitLab project: use the GitLab card for its suggestions."
2. **Branch**, **Into**, **Import**.

In the browser: the same explanation page as Overleaf ("Most git servers do not answer web
pages…"), with the desktop app and Import a Zip File….

### Folder

No page: choosing the card opens the system's folder picker straight away (`project.open`), and
the dialog closes when a folder is chosen (it stays if the picker is cancelled). In the browser it
opens the browser's folder picker, and the files become a project in the library
(`project.import`, as a dropped folder does today), named after the folder.

### Zip File

No page: the file picker for `.zip`, then `project.import`; on the desktop the project is
unzipped into `~/Documents/<zip name>` and opens there (Save Project As can move it); in the
browser it is kept in the library under the zip's name. A zip with one top folder is unwrapped
(its contents become the project), as the drop does.

## Keyboard and screen reader

- **Opening** moves focus to the first card (or, opened at a source, to its first empty field).
  Closing gives focus back to what opened the dialog.
- **Grid:** a group named "Online" and one named "On this computer". Each card is a button and a
  Tab stop (seven stops; few enough that a roving tab index would hide more than it saves).
  Arrow keys also move between cards in two dimensions, Home and End to the first and last;
  focus moves with `request_focus`. Enter or Space chooses.
- **Names:** a card's name starts with its title, then its line: "GitLab, signed in as ada",
  "Overleaf, desktop app only" (and the node is marked unavailable, while still focusable),
  "Folder, opens it where it is".
- **Pages:** the dialog's name follows the page ("Import from GitLab"). Back is a button named
  "Back to sources". Fields have their labels as names; the line under a field is its
  description, so it is read with the field.
- **Live region** (polite): the address check's result, the number of suggestions, each import
  phase ("Receiving files", "Writing files"), the error, and "Imported paper". Errors are also
  text on the page, so they can be read again.
- **Focus after an error** stays where it is; after a failed import, on the field the error
  concerns.
- egui_kittest tests when built: arrows move focus through the grid, Enter opens a page, Back
  returns focus to the card, Escape closes and returns focus, an unavailable card is reported as
  such (standards/accessibility.md).

## Phone (narrow window)

- A sheet over the window: the top bar holds × (or Back on a page) and the title; content
  scrolls; Import sits in the bottom bar, always on screen.
- The grid becomes a list: one row per source, 56 px tall, icon, title and line, a
  `chevron-right` at the end, the group headings kept. Same order, same names.
- Fields take the full width; Into's Change… moves under the path.

## Icons

Ours, drawn in the style of `assets/icons` (24-unit grid, 2-unit stroke drawn at 1.75 px, round
caps and joins, no fill). Where Lucide (ISC, already our source) has the shape, take it from
Lucide's release and record it in ATTRIBUTION.md; otherwise draw it.

| Name | Shape | Used for |
|---|---|---|
| `git-host` | a rounded square (a repository on a server) with a git branch inside: two nodes joined, one branching off. Lucide's `folder-git-2` or `square-git-branch` shape | GitLab, GitHub, Overleaf |
| `server` | two stacked rounded rectangles with a dot each (Lucide `server`) | GitLab Server |
| `link` | in the set already | Git Address |
| `folder-open` | in the set already | Folder |
| `file-archive` | a page with a zipper of short dashes down its middle (Lucide `file-archive`) | Zip File |
| `circle-check`, `info`, `circle-exclamation-point`, `arrow-left`, `chevron-right`, `chevron-down` | in the set already | states, Back, rows |
| `import` | a tray with an arrow coming in from above (Lucide `download` is close; ours keeps the arrow entering the tray) | the start page's Import… button, the menu entry |

No service's logo, colour or lookalike: the three hosted services share one neutral glyph, and
their names, in text, tell them apart. A leaf, a fox, a cat or anything that echoes a service's
mark is out (standards/clean-room.md, Assets 3).

## Commands (the automation interface)

Everything the dialog does is a command, so agents and the command line can import too.

| Command | Params | Result | Notes |
|---|---|---|---|
| `import.sources` | — | `sources: [{id, available, why?, account?, servers?}]` | What the grid shows: `id` one of `gitlab`, `github`, `overleaf`, `gitlab_server`, `git`, `folder`, `zip`; `why` when unavailable here ("browser: no CORS"). Never a token. |
| `git.check_address` | `{url, username?, token?}` | `{reachable, needs_sign_in, branches, default_branch, host: gitlab\|github\|overleaf\|other, https_url}` | The page's check; `https_url` is the SSH address rewritten. With `background`, polled like forge lookups. |
| `project.clone` | `{url, folder, branch?, service?, instance?, username?, token?, remember?, background?}` | `{folder, branch, remote, files, skipped}` | Desktop only (in the browser it says so and names `forge.open`). Refuses a non-empty folder. Opens the project. Sets `origin` and the upstream. |
| `project.cancel_clone` | — | — | Stops a clone under way and removes the folder it made. |
| `forge.open` | as now | as now | The browser's GitLab and GitHub import. |
| `project.open`, `project.import` | as now | as now | Folder, Zip File. |
| `forge.projects`, `forge.branches`, `forge.sign_in`, `forge.check_server`, `forge.add_server` | as now | as now | The GitLab, GitHub and Server pages. |
| UI: `project.import_dialog` | `{source?, address?}` | — | Opens the dialog, at a source if given (File › Import a Project…). |

Tokens remembered (Remember on this computer, ticked by default; in the browser Remember in this
browser, its local storage, or Remember in this tab, the tab's session storage, as Settings ›
Accounts › Remember sign-ins in this browser says) are kept per host (`texcraft.git.<host>`), the forges' personal
access tokens as Overleaf's and any git server's, listed in Settings › Accounts under "Tokens
kept on this computer" by service and host with **Replace…** and **Forget** (`git.credentials`,
`git.keep_credential`, `git.forget_credential`), and used by the next import, the suggestions,
Push, Pull and Sync for that host. Never in a result, a log or the preferences.

## Words

The dialog's title and menu entry are plain words: "Import a Project" (start page: "Import…").
GitLab, GitHub and Overleaf appear as the names of the services the project comes from, as
"Open from GitLab or GitHub" does today (standards/naming.md rule 3: the services and protocols we
connect to, once the integration works); Overleaf's row goes into docs/feature-names.md when this
is built. Our own labels: Git Address, GitLab Server, Folder, Zip File, Into, Remember on this
computer.

## Rejected alternatives

- **A tab per source** (or a source list on the left with a pane on the right): seven tabs do not
  fit a phone, and the left list duplicates what the grid says; the grid also reads well as a
  first screen.
- **One address field that guesses the source** from what is pasted: elegant for git addresses,
  but it hides folders, zips and the server setup, and a first-time user does not know what to
  paste. The Git Address card does accept any forge's address, so the shortcut exists.
- **Disabled (inert) cards in the browser:** a dead control without a reason is the dead end
  docs/design.md's "honest controls" forbids; a focusable unavailable card with a page explaining
  the way out is clearer.
- **Hiding Overleaf and Git Address in the browser:** the user would not learn that the desktop
  app can do it.
- **Keeping the desktop's in-memory open** of GitLab and GitHub projects as the import: a folder
  with `origin` is what Sync and the History panel work with; the in-memory path stays for agents
  (`forge.open`) and the browser.
- **Service logos or brand colours on the cards:** not ours to use (standards/clean-room.md,
  standards/naming.md rule 2), and the names are clearer.
- **Two-letter monograms** (GL, GH, OL) as icons: they read as stand-ins for the logos and do not
  translate.
- **A wizard with Next buttons:** each source has at most four fields; one page with defaults is
  shorter than three steps.

## Open questions for the operator

1. **Overleaf's details.** The clone address's host (`git.overleaf.com`?), the user name git
   expects with a token, and whether git access depends on the account's plan must be confirmed
   from a real clone with an account we own (not from their pages, under our clean-room reading).
   Also: should the card be named "Overleaf" at all (rule 3 of naming.md looks satisfied; your
   call), and should it accept a self-hosted instance's git address (likely just the Git Address
   card)?
2. **GitHub Enterprise.** One "GitLab Server" card that also lists GitHub Enterprise servers, or
   a neutral "Your Server" title? The latter is more honest but less clear.
3. **Start page.** Is "Import…" next to Open Folder… enough, or should Open Folder… move into the
   dialog on the desktop too (fewer buttons, one more click)?
4. **History depth.** Clone the whole history (Sync and History need it; gitwire has no shallow
   fetch) — with the 200 MB cap — or offer "Only the latest version"?
5. **Where clones land.** `~/Documents/<name>` by default, or a TeXCraft folder
   (`~/Documents/TeXCraft/<name>`) that also collects new projects?
6. **Desktop GitLab/GitHub without a folder.** Keep "Open without saving to a folder" as a quiet
   link for people who want the old in-memory behaviour, or drop it?
7. **Shortcut.** Cmd+Shift+O for Import a Project… is free in our table; is it wanted?

## Decisions (operator, 2026-11-26)

1. The card is named "Overleaf".
2. GitHub Enterprise servers: not in this dialog for now.
3. Both stay: the Import dialog and Open Folder… on the desktop.
4. Clones fetch the full history (gitwire has no shallow fetch), within the size limit.
5. Clones go into the operating system's documents folder (the XDG documents folder on Linux,
   Documents on macOS and Windows; through the `dirs` crate or the platform's API), with a
   Change… choice; a folder chosen there becomes the default, kept in the preferences
   (`clone_folder`).
6. No "open without saving to a folder": a git import always makes a local clone, and the dialog
   says so.
7. File › Import a Project… gets Ctrl+Shift+O (Cmd+Shift+O on macOS).
8. **The cards show the services' own logos** (GitLab, GitHub, Overleaf), not a neutral glyph:
   the operator's decision, taken as fair use to identify the service a card connects to, to be
   removed if it ever becomes a problem. This departs from standards/naming.md rule 2 ("SHOULD
   NOT use other companies' logos or icons"), a SHOULD: the reason is recorded here, in
   docs/feature-names.md and in the commit that adds them. The marks come from the Simple Icons
   project's release (CC0-1.0, on licences.md), at a pinned version from its own repository, as
   single-colour SVGs drawn in the text colour; ATTRIBUTION.md records each file, its version
   and checksum, and that each is its owner's trademark, shown only to name that service. Our own
   features keep our own words ("Import a Project", "Sync").
