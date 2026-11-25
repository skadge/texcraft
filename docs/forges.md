# Projects on git forges

Roadmap item C5 (docs/ROADMAP.md). TeXCraft follows the design retex worked out for its browser
editor (retex's `docs/design/gitlab.md`): no server of ours and no git client in the page. The
forge's own web API is enough to read a branch and commit to it, and GitLab's answers browsers
(CORS) as well as the desktop. GitLab comes first; GitHub later.

## Step 1: GitLab with a personal access token (done)

What it does:

- **Open from GitLab…** (the start page, File menu; `forge.open {url, token, ref?}`): a project's
  address (its page, a page under it, its clone address, or `group/project` on gitlab.com; other
  instances by their address, HTTPS only) and a personal access token. TeXCraft reads the project,
  the branch's head (the default branch unless one is given), the commit's whole tree (page after
  page) and each file's blob, eight at a time, each checked against its git blob id.
  - Hidden files (`.gitignore`, `.gitlab-ci.yml`), symbolic links and submodules stay on the
    branch as they are, as a folder's hidden files stay on disk. Files over 10 MB are skipped and
    said; a project over 200 MB is refused (its files are all in memory).
  - **On the desktop** the project opens in memory, linked to its branch, in no folder: commits go
    to GitLab, the window asks before closing with changes that are not committed, and Save
    Project As can still copy it into a folder. A clone into a folder would be a second copy to
    keep in step with the branch (and with C8's history in it) without git to do it; the in-memory
    project is the simplest honest form until push and pull for folders come (step 4).
  - **In the browser** it is kept in the library under the project's name, with its link (instance,
    project, branch, base commit, the files' blob ids at the base): a reload keeps the changes and
    counts them from the same base.
- **The token** is kept by the session only, per instance: never in the library, the preferences,
  a log or a command's result, and the dialog says so. After a restart (or a reload) the commit
  asks for it again. A token needs the `read_api` scope to open a project, `api` to commit
  (GitLab's `write_repository` is for git over HTTP only); a project access token narrows `api`
  to one project.
- **What changed**: a file is changed when the git blob id of its bytes differs from its base's,
  added when the base has none, deleted when the base has one the project does not; a deleted and
  an added file with the same blob id are a move. No copy of the base is kept. `forge.status`
  says it; the top bar shows the project, the branch and how many files changed.
- **Commit to GitLab…** (File menu, the top bar; `forge.commit {message, token?}`) lists the
  changed files and makes one commit of them on the branch (`POST …/repository/commits` with
  `create`, `update`, `delete` and `move` actions, contents in base64), then makes it the base.
  Before it the branch's head is read:
  - a protected branch, or a role that cannot push, is refused before anything is sent;
  - if the branch moved since the base, its tree is compared with the base's: commits that
    changed other files are kept (the commit goes on top of them) and what they changed is taken
    into the project; commits that changed files changed here too refuse the commit, naming the
    files, with nothing sent (the merge is step 2). The same change made on both sides is no
    conflict.
  - a commit made by someone else between the check and the commit is found by the new commit's
    parent, and taken in the same way.
- **Errors in words**: a token GitLab does not accept (401), a scope or role that does not allow
  it (403, with the scopes needed), a project or branch it has not or the token cannot see (404),
  the rate limit (429, with when to try again from `Retry-After`), GitLab's own message for other
  refusals, and the network.

How it is built:

- `crates/forge` (`texcraft-forge`): the API's logic as plain async Rust over a `Transport`
  (request in, response out): the desktop's blocking HTTP client (ureq, already in the tree
  through retex-texlive) on a thread of its own, the browser's `fetch` through web-sys, or a
  GitLab in memory for tests (`fake`). `task` runs an operation in the background (its own thread,
  or the page's event loop) and hands back its outcome.
- The engine's commands stay synchronous: `forge.open` and `forge.commit` take `background` and
  return at once; `Session::poll_forge` gives the outcome, which the window puts on the status
  line. Without `background` they wait (the command line, agents).
- Review threads (`.texcraft/review/`) were hidden files in step 1, neither loaded nor committed;
  step 2 carries them.

## Step 2: keeping up with the branch, conflicts, merge requests (done)

What it does:

- **Update from GitLab** (File menu; `forge.pull {token?, resolve?, background?}`) takes in the
  branch's new commits. Their tree is compared with the base's blob ids (no compare API, whose
  diffs can be cut short):
  - a file changed only on the branch is taken (or removed);
  - a file changed on both sides the same way is left alone;
  - a text file changed on both sides is merged line by line (`texcraft_forge::merge`: a Myers
    diff of the base against each side, from the `similar` crate, put together as diff3 does).
    Changes to different lines merge; the same change on both sides is no conflict; lines are
    compared without their line ending, and the merged file keeps the local one (`\r\n` stays).
  - changes to the same lines or to neighbouring lines (as git), a file deleted on one side and
    changed on the other, a file added on both sides differently, and a binary file changed on
    both sides are **conflicts**. Then nothing is taken in, the base stays, and `forge.status`
    lists the files (`conflicts`, with why) until a pull or commit gets through.
- **Conflicts** are settled per file with `resolve: {path: "mine" | "theirs"}` on a pull or a
  commit: keep mine commits the project's version over the branch's, take theirs drops the change
  made here. The commit dialog lists the files in conflict with Keep mine and Take theirs for each;
  a pull that finds conflicts opens the same choices. No conflict markers in the editor.
- **A new branch with a merge request** (`forge.commit {message, new_branch}`; the commit dialog's
  "Commit to a new branch and open a merge request"): the changes since the base are committed to a
  new branch started from the base (`POST …/repository/commits` with `start_sha`, so no merge is
  needed), then `POST …/merge_requests` opens a merge request into the branch, where GitLab shows
  any conflict; its link is shown. The project then follows the new branch, so later commits go
  to the merge request. A merge request that cannot be opened is said, and the commit stays on the
  new branch. A name git would refuse, or an existing branch, is refused before or by GitLab.
- **A protected branch, or a role that cannot push** (the branch's `can_push`, or a commit's 403)
  refuses the commit with nothing sent (`forge.status`'s `cannot_push`), and the commit dialog
  opens again offering only a new branch with a merge request. Forking a project one cannot push
  to at all is out of scope.
- **A commit** catches up first as a pull does: merged files are committed on top of the branch's
  head; conflicts refuse the commit unless `resolve` settles them.
- **The periodic check**: while a project linked to a branch is open and a token was given in the
  session (GitLab's limits without one are low), the window runs `forge.pull` every two minutes
  and when it gets the focus back (a browser tab too), not while a dialog is open or another
  operation runs. It is quiet: new commits taken in are said on the status line; conflicts are
  shown in the top bar (`2 files in conflict`) and said once, and the next commit offers the
  choices.
- **Edits made while an operation ran**: updates are computed from the files as they were when it
  started; a file edited since gets the update merged into the edits, or keeps the edits (said as
  kept) when they do not merge.
- **Review threads** (`.texcraft/review/*`, exactly that folder; other hidden files stay on the
  branch) are loaded from the branch into the review, committed with the project (their quotes
  renewed from the text, as a save does), counted among the changes, and pulled and merged like
  any file; a thread changed on the branch replaces its copy in the review.

How it is built: `texcraft_forge::pull`, `commit` (with `Choices`), `commit_to_new_branch` and
`merge::merge3`; `GitLab::create_commit` takes a `start_sha`, `GitLab::create_merge_request`; the
fake GitLab has protected branches, `start_sha` and merge requests. The engine keeps the last
refusal with a way out (`Session::forge_problem`), reads and writes the review's thread files
through the forge's paths, and the window's `forge::check` runs the periodic pull.

Limits: merges are by line; a change on the branch made between the check and the commit is
written over by the commit's actions for the files it sends (GitLab's commit API has no
compare-and-swap that was checked against gitlab.com; the commit's parent says it happened); the
choice is per file, not per change.

## Step 3: signing in

OAuth with PKCE (a non-confidential application registered on gitlab.com), the token renewed with
its refresh token and kept in the system's secret store (desktop) or the browser's storage;
`read_api` first, `api` asked for at the first commit. The branch checked when a stored project
opens again.

## Step 4: more forges and folders

- GitHub: its API has the same calls (trees, blobs, the Git Data API for commits) and answers
  browsers, but its OAuth token endpoint does not, so either tokens pasted by the user or a small
  token exchange.
- Push and pull for project folders (real git, on the desktop).
- Files over the limits fetched when the engine reads them; a diff of the changes before
  committing; conflict markers or a side-by-side view to merge a file by hand.
