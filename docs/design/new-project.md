# New Project: where to keep it

> **Status:** built (next release) · **Roadmap:** C11 (docs/ROADMAP.md) · **Written:** 2026-11-27

Starting a project from a template asks one more thing after the template: where the project
lives. On this computer (a folder, as before) or in the browser, or on a forge, by making a new
repository there, with on the desktop a folder that is its clone. The project then opens linked to
it, and Sync works at once.

This design is our own, from docs/design.md, docs/design/import-dialog.md and
docs/design/connections.md: the step reuses Import's cards and Settings › Accounts' connections, so
the three read as one family.

## The steps

1. **Template** (the gallery, unchanged): categories, search, the cards, the details and
   **Create Project…** (its ellipsis says a question follows). The blank project stays a card of
   its own that makes an empty project at once.
2. **Where to keep it**, in the same dialog (no dialog over a dialog): its title "Where to Keep
   It", a Back arrow before it (Alt+Left, Cmd+[ on a Mac) to the gallery with the template still
   chosen, and the template's name under the title.

The start page's template cards open the dialog at step 2 for their template. The example project
(Help › Open the Example Project) keeps its one question (a folder, or a name in the browser): it
is for trying TeXCraft, not for keeping.

## The cards

One row of Import's cards (`import::cards_row`; on a phone `import::list_row`), with the arrow keys
of Import's grid (`import::arrows`):

| Card | Icon | Title | Line |
|---|---|---|---|
| This computer (desktop) | `folder-open` | This Computer | "A folder of your choice" |
| In the browser (web) | `folder-open` | This Browser | "Kept in this browser" |
| Each forge connection that can make repositories (`accounts.list`'s rows with `can_create`) | the service's mark, or `server` | GitLab, GitHub, or the server's host | "@ada" (signed in or a token), "Not signed in", or "Signed in, expired" |
| Add a Connection… | `plus` | Add a Connection… | "GitLab, GitHub or your server" |

- **Chosen card:** the 2 px accent outline and `accent_soft` fill of the gallery's chosen
  template, and the accessible node is selected. The default connection's line ends in
  "· Default".
- **Preselected:** the default connection (Settings › Accounts › Set as Default); otherwise the
  last place chosen (`new_project_place` in the preferences: `local` or the connection's address);
  otherwise This Computer / This Browser. A place that is gone falls back the same way.
- **Overleaf:** no card. Overleaf has no API that makes projects, so one muted line under the
  cards says "To start on Overleaf, create the project there, then Import it."
- **Add a Connection…** opens Settings' Add a Connection dialog in place of this one; when it
  ends (connected, signed in, or closed) the dialog comes back at step 2, and the new connection's
  card is chosen.

## The fields under the cards

**This Computer:** **Folder**: the full path and **Change…** (the folder picker for the parent),
default `<the Documents folder, or the one Import's Change… chose>/<name>`, the name from the
template's title, made a folder name, `-2` added while one exists. Checked in place as Import's
Into row: a folder that exists and is not empty says so and offers the next free name.

**This Browser:** **Name**: the project's name in the browser's library, the template's title by
default; a name taken says so.

**A forge:**

- **Repository**: the name, from the template's title made one the forges take (letters, digits,
  `.`, `-`, `_`; spaces become `-`; accents left out). Checked as typed: first its form, then,
  half a second after typing pauses, on the forge (`forge.name_available`): "Checking…", then
  "ada/paper is free" (`circle-check` in `success`) or "ada/paper exists already: choose another
  name." (`error`, with the icon). The check is a hint; the forge's own refusal when the
  repository is made is what counts.
- **Owner**: you, or a group (GitLab) or organisation (GitHub) in which you can make repositories:
  the list Save Project to GitLab/GitHub… shows (`forge.owners`).
- **Who can see it**: Private (the default), Internal (GitLab only: everyone signed in to that
  server), Public, as radio buttons, with one line saying what the choice means.
- **Folder** (desktop): as This Computer's, its name following the repository's until edited.
- Not signed in to a server added: "Not signed in to git.example.org." and **Sign In**; read-only
  GitLab sign-in: **Allow Commits on GitLab** (as Save Project to GitLab…).

**Footer:** Cancel and **Create Project** (accent), off until the fields are complete (each
field says why under itself). Enter in a field is Create Project.

## The order of operations

**Desktop, on a forge** (`project.from_template` with `forge`):

1. Checked first, nothing made yet: the template (downloaded first if it is not kept, as before),
   the folder (new or empty), the name's form, a key that can write.
2. **The repository is made** (`forge.create_repository`'s thread, owner resolved by
   `forge.owners`'s request). A taken name or a missing permission ends here: the dialog says so
   and stays open, nothing is written.
3. **The template is written** into the folder (`project.from_template`'s own copy), the project
   opens, its first commit is made with a `.gitignore` of build files (Save Project to…'s
   `prepare_push`), `origin` is set to the new repository.
4. **The history is pushed** (`git.push`'s path); once it is, the branch follows `origin/main`.

**Browser, on a forge:** the repository is made, then the template's files are its first commit
through the forge's API (Save Project to…'s browser path); then the project is made in the
library, linked to it.

So there is one implementation of each part: the New flow is "make the repository" and "write the
template", joined by the push Save Project to… already makes.

## Progress and result

- The footer's place takes Import's progress: a thin bar and one line, a live region said at each
  phase: "Making ada/paper on GitLab…", "Writing the template and its first commit…", "Sending the
  project to GitLab…". Escape and Cancel do nothing while it runs (a repository being made cannot
  be taken back halfway); the × is off.
- At the end the dialog closes, the project opens, and the status line says "Created ada/paper on
  GitLab." (or GitHub); the top bar has Sync.

## Failures

| When | Said | What stays |
|---|---|---|
| The name is taken (the forge says so) | under Repository, "{name} exists already: choose another name." | the dialog, open; nothing made or written |
| No permission to make repositories (403), a group not allowed | under the cards, the forge's words, and the permission needed | the dialog, open; nothing made |
| The token or sign-in is refused (401) | as Save Project to… says it, with Sign In | the dialog, open |
| The network fails before the repository is made | "Could not reach {host}…" | the dialog, open; Create Project tries again |
| The template cannot be written after the repository was made | "Created ada/paper on GitLab, but the template could not be written into {folder}: {why}. The repository is empty: try again into another folder, or delete it on GitLab." | the dialog, open (on a new Folder the same name is refused, so the hint says to choose another name or delete it) |
| The push fails after the repository was made | the dialog stays with "Created ada/paper on GitLab and the project is in {folder}, but sending it did not work: {why}." **Try Again** (`git.push`) and **Close**; Sync also sends it later | the local project, open, with `origin` set; the repository, never deleted silently |

## Commands

| Command | Params | What |
|---|---|---|
| `project.from_template` | `{template, folder, forge?: {service, instance?, owner?, name, visibility?, token?}, background?}` | As before without `forge`. With it: the repository first, then the template, its first commit, `origin`, the push (desktop); in the browser the template as the repository's first commit and the project linked. Result: `operation: "new_project"`, `folder`, `main`, `project` (`ada/paper`), `web_url`, `pushed`, `push_error?`. Agents use a connection already made (a token kept, or `token`); they never sign in. |
| `forge.name_available` | `{service, instance?, owner?, name, token?, background?}` | Whether `owner/name` is free on the forge: `{available, project}`. |
| `forge.owners` | as now | the Owner list |
| `accounts.list` | as now | the cards |

## Rejected alternatives

- **Where first, then the template:** the template is what people come for; the place is a
  property of the project, decided once the project is chosen.
- **Writing the template before making the repository**, removing it on a refusal: a refusal is
  the common failure, and it would replace the open project for nothing.
- **An Overleaf card that opens Overleaf's page:** it would look like TeXCraft makes the project
  there; the one line says what to do, honestly.
- **Deleting the repository when the push fails:** the user may not expect a deletion, and the
  forge may keep a deleted repository's name; the project is kept and Sync sends it.
