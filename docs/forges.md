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
- Review threads (`.texcraft/review/`) are hidden files: they are neither loaded from nor
  committed to the branch yet.

## Step 2: signing in, keeping up with the branch

- OAuth with PKCE (a non-confidential application registered on gitlab.com), the token renewed
  with its refresh token and kept in the system's secret store (desktop) or the browser's storage;
  `read_api` first, `api` asked for at the first commit.
- The branch checked when a project opens again and while it is open; new commits taken in at
  once when they touch no file changed here.
- A three-way merge of text files (base, theirs, ours) line by line; overlapping changes, a file
  deleted on one side or a binary file are conflicts, listed with keep mine or take theirs for
  each.

## Step 3: merge requests

When the branch cannot take the commit (protected, a role that cannot push, or a conflict the user
does not resolve), a commit to a new branch from the base (`start_sha`) and a merge request
(`POST …/merge_requests`), where GitLab shows the conflict.

## Step 4: more forges and folders

- GitHub: its API has the same calls (trees, blobs, the Git Data API for commits) and answers
  browsers, but its OAuth token endpoint does not, so either tokens pasted by the user or a small
  token exchange.
- Push and pull for project folders (real git, on the desktop).
- Files over the limits fetched when the engine reads them; a diff of the changes before
  committing; review threads committed with the project.
