# Link to a Repository

> **Status:** built (next release) · **Roadmap:** C10 (docs/ROADMAP.md) · **Written:** 2026-11-28

An open project is linked to a git repository that already exists (on GitLab, GitHub, a
self-managed server, Overleaf's git, or any git host over HTTPS), and the two histories are
brought together once: the project's sent into an empty repository, or the repository's merged
with the project's. After that Sync works with it. Save Project to GitLab/GitHub… makes a new
repository; this is for one that exists.

This design is our own, from docs/design.md (Patterns), docs/design/import-dialog.md and
docs/design/connections.md: the dialog is Import a Project's, with its cards and pages, so the
two read as one family.

## The name

Git calls it adding a remote. A writer thinks of linking the project to a repository, so the
label is **Link to a Repository…** (File menu, next to Save Project to GitLab or GitHub; the
History panel; All Commands, where "Add a Remote" finds it too). The command ids keep git's
words: `git.add_remote`, `git.check_remote`, `git.remove_remote`. Plain descriptive words
(docs/feature-names.md).

## The dialog

Import a Project's dialog, its Online cards only, the default connection first, then the others
in Import's order: GitLab, GitHub, Overleaf, GitLab Server, Git Address. The cards' lines are
Import's ("Signed in as ada", "Sign in or use a token"). The title is "Link to a Repository";
a card's page is "Link to GitLab", "Link to a Git Address"…, with Back ("Back to sources", as in Import).

Each page has Import's fields (sign-in or token, the project with its suggestions and the branch
list for a forge; the address checked as typed, with a user name and a token when the server asks,
for Overleaf and a git address), but no Into: the project stays where it is. Under them, once the
repository and its branch are known, **What happens**, checked on the server
(`git.check_remote`, half a second after typing pauses), before anything changes:

| The repository | Said |
|---|---|
| has no commits | "The repository is empty: your project's history will be sent to it." |
| has commits of another history | "The repository has N versions: they will be merged with your project's." |
| …and files with the same names but other contents | "Both have files with the same names: you'll choose which to keep for each." and the files, by name |
| has this project's history already | "The repository has this project's history: Sync brings the two up to date." |
| (browser) GitHub, empty | "GitHub's web API cannot write into an empty repository: add a file to it on GitHub first, or use the desktop app." Link is off. |

When the project has a remote named `origin` already, a **Name** row asks for this one's
(suggested from the host: `gitlab`, `github`, `overleaf`, or the host's first word), with one line:
"Sync will use this repository from now on; origin stays as it is."

Footer: Cancel and **Link** (accent), off until the check said what happens. While it runs, the
footer's place is the progress: "Linking and bringing the histories together…".

At the end the dialog closes and the status line says "Linked to ada/paper (main): your history
is sent." or "Linked to ada/paper (main): took in 3 files." The top bar has Sync.

## What linking does

**Desktop** (a project folder with its git repository):

1. The remote is set (`origin`, or the name chosen), its address with the user name given in it,
   and the branch follows the remote's branch (the default one, or the one chosen).
2. A Sync: a version of the project's changes, then a fetch of the remote's branch, then:
   - no such branch (an empty repository): the history is pushed, and the branch follows it;
   - the remote's history is the project's or an older one: the project's newer versions are pushed;
   - another history: they are merged. With no common version, the merge's base is empty, so for
     each file: the same on both sides, or on one side only, is kept; the same name with other
     contents is a conflict, settled per file with Sync's dialog (keep mine, take theirs). The
     merge is a commit with both histories as parents, and it is pushed.
3. Never a forced push, never a file dropped without a choice; the version made first keeps the
   project's state before the merge in the history.

A link that does not get through (a credential refused, the server unreachable) is undone: the
remote and the upstream are put back as they were. Files in conflict found on the way (the
repository changed since the check) keep the link, and Sync's dialog settles them. In the window
the link runs in the background: a failure after it started is said in the dialog ("The project is
linked, but bringing the two together did not work: …"), and Link or Sync tries again.

**Browser** (GitLab and GitHub, through their web APIs; other git servers do not answer web pages,
so Overleaf and Git Address show Import's "Desktop app only" page): the branch is read, and its
files and the project's are brought together the same way, per file (conflicts settled before
anything changes, with the same dialog). The files only on the branch come into the project, the
project is linked to the branch (as Save Project to… links), and a Sync sends the project's files
as a commit. An empty GitLab repository gets the project's files as its first commit.

## Changing or removing the link

In the History panel, next to Push and Pull: **Unlink…** asks first ("Unlink the project from
origin? Neither the project nor the repository changes: the project just stops syncing with it.")
and removes the remote (`git.remove_remote`; in the browser, the link to the branch). Linking
again to another repository is Link to a Repository….

## Commands

| Command | Params | What |
|---|---|---|
| `git.check_remote` | `{url, service?, instance?, branch?, username?, token?, name?, background?}` | What linking would do, nothing changed: `empty`, `branch`, `branches`, `versions` (commits there), `related`, `clashes` (paths, same name, other contents), `only_there`, `only_here`, `same`, `name_taken`, `suggested_name`, `remotes`. Desktop: the objects are fetched (no ref moves). |
| `git.add_remote` | `{url, name?, branch?, service?, instance?, username?, token?, remember?, resolve?, background?}` | Links and brings together, as above. Desktop: Sync's outcome with `added: {name, url, branch}`. Browser: `operation: "add_remote"`, `taken`, then `project.sync` sends. |
| `git.remove_remote` | `{name?}` | Removes the remote (the upstream's by default) and its tracking refs; in the browser, the link. |
| UI: `git.link_dialog` | `{source?, address?}` | Opens the dialog. |

## Rejected alternatives

- **"Add a Remote…" as the label:** git's word; most writers do not know it. It stays an alias.
- **A dialog of its own:** Import's cards and pages already ask for exactly this; a copy would
  drift from them.
- **Refusing unrelated histories** (git's default): the common case here is a repository made on
  the forge with a README, and the project's own history; refusing would send people to the
  command line.
- **Taking the repository's files over the project's** (or the other way) for files with the same
  names: either loses work without a choice.
