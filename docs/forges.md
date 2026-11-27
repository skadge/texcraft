# Projects on git forges

Roadmap item C5 (docs/ROADMAP.md). TeXCraft follows the design retex worked out for its browser
editor (retex's `docs/design/gitlab.md`): no server of ours and no git client in the page. The
forge's own web API is enough to read a branch and commit to it, and GitLab's and GitHub's answer
browsers (CORS) as well as the desktop. GitLab came first (steps 1 and 2), GitHub second (step 3);
the sync logic is written once, over a `Forge` trait.

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
    project is the simplest honest form. Folders push and pull with git since step 5.
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
  - lines both sides changed differently are merged again **word by word** (C12 step 1): the
    passage is cut into tokens (words, runs of whitespace, line endings, punctuation, and LaTeX's
    `\commands`, control symbols such as `\%`, braces, `$` and a `%` comment to the end of its
    line, each whole) and merged as the lines are, with a Myers diff of its own that stops at 1000
    token edits a side. So edits to different sentences or words of one paragraph line merge, a
    `\cite{…}` added beside a typo fixed elsewhere merges, and an edit inside `$…$` merges with
    prose edited elsewhere. Every change of both sides is in the result once; the same change on
    both sides counts once; line endings are kept as above. Changes to neighbouring lines merge
    too now (git makes them a conflict). `merge::merge_text` gives the passages still in conflict
    (their byte ranges in base, mine and theirs, and their text) for the passage-by-passage view
    of step 2 of C12.
  - changes to the same words or right next to each other, a passage over 200 kB on a side or
    with more than 1000 token edits (not merged by word, to keep the cost bounded), a file
    deleted on one side and changed on the other, a file added on both sides differently, and a
    binary file changed on both sides are **conflicts**. Then nothing is taken in, the base stays, and `forge.status`
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

Limits: merges are by line, then by word where both sides changed the same lines; a word
repeated next to an edit ("the the") can make the edit's place ambiguous and a conflict; on GitLab a change on the branch made between the check and the
commit is written over by the commit's actions for the files it sends (GitLab's commit API has no
compare-and-swap that was checked against gitlab.com; the commit's parent says it happened); the
choice is per file, not per change.

## Step 3: GitHub with a personal access token (done)

What it does — everything steps 1 and 2 do, on GitHub:

- **Open from GitLab or GitHub…** (the start page, File menu; `forge.open {url, service?, token,
  ref?}`): the dialog has a GitLab / GitHub choice, set by itself when the address is on
  gitlab.com or github.com. github.com's addresses are GitHub's: the repository's page or any page
  under it (`/tree/main/…`), `.git` and `git@github.com:owner/repository.git` clone addresses,
  `github.com/owner/repository`. A bare `owner/repository` is GitLab's unless GitHub is chosen
  (`service: "github"`); another host chosen as GitHub is a GitHub Enterprise Server (its API at
  `/api/v3`). gitlab.com's and github.com's addresses are refused for the other service. The
  link remembers its service (`Address::service`); a link saved before is GitLab's.
- **The token**: a fine-grained personal access token given access to the repository, with
  Contents read-only to open it, read and write to commit, and Pull requests read and write to
  open a pull request (a classic token: the repo scope). The dialog says so, per service; it is
  kept as GitLab's is, per instance, for the session only.
- **Reading**: `GET /repos/{owner}/{repo}` (its default branch, and `permissions.push`),
  `GET …/branches/{branch}` (its head, and `protected`), `GET …/git/trees/{commit}?recursive=1`
  (the whole tree in one answer; a tree GitHub says is `truncated`, over 100 000 entries or 7 MB,
  is refused in words rather than opened in part) and `GET …/git/blobs/{sha}` (base64, checked
  against the blob id; one over 10 MB by its `size` is skipped). Requests carry
  `Authorization: Bearer`, `Accept: application/vnd.github+json` and
  `X-GitHub-Api-Version: 2022-11-28`.
- **Committing** through the Git Data API: the parent's tree (`GET …/git/commits/{head}`), a blob
  for each added or changed file (`POST …/git/blobs`, eight at a time, each id checked against
  ours; a moved file's blob is already there), a tree on the parent's (`POST …/git/trees` with
  `base_tree`; `sha: null` deletes a path), the commit (`POST …/git/commits` with the head as its
  parent), then the branch moved to it without forcing (`PATCH …/git/refs/heads/{branch}`,
  `force: false`). That last step is a compare-and-swap: when it is refused (422) and the branch's
  head moved, the new commits are taken in as a pull does (merged, or conflicts) and the commit
  made again on the new head, up to three times; nothing anyone committed is written over. When it
  is refused with the head unchanged (a rule the branch does not show), it is "cannot push".
- **A protected branch** (the branch's `protected`), or no push permission, refuses the commit
  before anything is sent, and the dialog offers a new branch with a pull request, as on GitLab.
  GitHub shows a branch's protection rules only to administrators, so a protected branch counts as
  one the user cannot push to even when its rules would let them; the pull request always works.
- **A new branch with a pull request**: the commit is made on the base, `POST …/git/refs` creates
  the branch (an existing one is refused by GitHub), `POST …/pulls` opens the pull request, and
  its link is shown ("Open the pull request on GitHub").
- **Labels and words say which service**: the menu's Commit to GitHub… and Update from GitHub
  when the open project is GitHub's (Commit to GitLab or GitHub… with none), the dialogs, the
  status line, the top bar's button, the errors. Errors in words: 401, 403 (with the permissions
  needed), GitHub's rate limit (a 403 or 429 with `X-RateLimit-Remaining: 0`, with the minutes
  until `X-RateLimit-Reset`, or `Retry-After`), 404 (a fine-grained token sees only the
  repositories it was given), and GitHub's own message for 409 and 422.

How it is built: `texcraft_forge::Forge` is what the sync needs (project, branch, tree, blob, one
commit to a branch or a new branch, a request to merge); `GitLab` and `GitHub` implement it and
`Client` is either, as the address says. A forge may answer `Made::Moved`, and `commit` then
catches up and tries again (`Error::Busy` after three tries). The transport learned `PATCH`, and
the browser reads GitHub's rate-limit headers. The fake (`fake::Fake`) is a GitLab or a GitHub
over the same commits; the forge tests of opening, pulling, merging, conflicts, committing, a
race, a new branch with a merge or pull request and a protected branch run against both.

A changed or moved file keeps its mode (read from the parent's tree when the commit has one), so
an executable file stays one. Step 4 added signing in on the desktop (the device flow); in the
browser GitHub keeps the pasted token (its OAuth token endpoint does not answer browsers).

## Step 4: signing in (done)

What it does:

- **Sign In with GitLab** (the Open and Commit dialogs, Settings › Accounts; `forge.sign_in
  {service: "gitlab", write?}`): OAuth 2's authorization code with PKCE (RFC 7636, S256), as
  TeXCraft's non-confidential application on gitlab.com: no client secret.
  - **On the desktop** TeXCraft listens on `127.0.0.1:47321` for the one redirect
    (`http://127.0.0.1:47321/callback`, RFC 8252's loopback redirection; std's TCP listener on a
    thread, ten minutes at most, cancellable) and opens GitLab's page in the system's browser.
    A request with another `state`, or to another path, is answered and ignored. The browser tab
    then says TeXCraft is signed in. A port already in use is said, with the token as the way out.
  - **In the browser** the whole page goes to GitLab (a popup would meet COOP and blockers) and
    comes back to the page's own address (`https://texcraft-893d51.gitlab.io/`, or
    `http://127.0.0.1:8768/` served locally). The `state` and the PKCE verifier wait in the tab's
    session storage; on return the shell reads `?code=&state=` (or `?error=`), takes it out of the
    address bar (`history.replaceState`) and hands it to `forge.sign_in_return`, which checks the
    state (used once) and exchanges the code at `/oauth/token` (gitlab.com answers web pages there).
    Projects in the library are kept in IndexedDB already; a project with unsaved changes makes the
    browser ask before the page leaves.
  - **Scopes**: signing in asks for `read_api read_user`. A commit with a sign-in that can only read
    is refused before anything is sent (`forge.status`'s problem `NeedsWrite`), and the Commit dialog
    offers "Allow Commits on GitLab", which signs in again with `api read_user`; the message is kept
    and the dialog comes back once signed in (on the desktop).
  - **Renewal**: access tokens last two hours. Before an operation an expired one (or one within a
    minute of it) is renewed with the refresh token (`grant_type=refresh_token`, with the redirect URI
    and the PKCE verifier GitLab's documentation lists); GitLab rotates the refresh token, so the
    renewed sign-in is kept whatever the operation's outcome. A refused renewal says to sign in again.
  - **Who**: `/api/v4/user` (name, username), shown in the dialogs and in Settings › Accounts.
  - Another GitLab signs in the same way once it is added, with its own application (below,
    "Self-managed servers").
- **Sign In with GitHub** (desktop only; `forge.sign_in {service: "github"}`): the device flow
  (RFC 8628). `POST https://github.com/login/device/code` (`scope=repo`); the dialog shows the
  code to enter, with Copy Code and Open GitHub's Page, and the page is opened once by itself.
  TeXCraft polls `POST https://github.com/login/oauth/access_token`
  (`grant_type=urn:ietf:params:oauth:grant-type:device_code`) every `interval`, slower from
  `slow_down` on; `expired_token` and `access_denied` end it in words; Cancel stops it. Who is
  signed in comes from `GET https://api.github.com/user`. In the browser GitHub keeps the personal
  access token, and the dialog says why: GitHub's token endpoint does not answer web pages.
- **The dialogs** offer Sign In first; "Use a personal access token instead" shows the token field
  (step 1's path, unchanged). A build whose client id is still a placeholder (ending in `_PENDING`)
  shows the button off, with "This build has no GitLab application yet" (or GitHub's), and the
  token field.
- **Where a sign-in is kept** (the access token, the refresh token, their expiry, scopes and user,
  as one entry per instance: `texcraft.forge.gitlab.com`, `texcraft.forge.github.com`):
  - the desktop: the system's keychain, through craft-suite-account's `Keychain` as the AI token
    is, so a restart stays signed in;
  - the browser: the tab's **session storage**, not IndexedDB. Any script of the page could read
    either; session storage keeps the sign-in through a reload and GitLab's round trip but forgets
    it when the tab closes, so a refresh token that works for months does not stay in the browser's
    profile. Signing in again is a redirect and nothing more while the user is signed in to GitLab.
  - the command line and agents: the session only.
  Never in the preferences, the library, a log or a command's result (tokens do not print; tests
  look for them in every result).
- **Disconnect…** (Settings › Accounts, asking first; `accounts.disconnect {host}`, or
  `forge.sign_out {service}` for the sign-in alone): forgets the sign-in at once and,
  on GitLab, revokes the token (`/oauth/revoke`) in the background; a failed revocation is said.
  GitHub lets only an application's server revoke its tokens, so signing out of GitHub says to
  remove TeXCraft under GitHub's Settings › Applications.
- `forge.accounts` lists the sign-ins (no tokens), whether each service can be signed in to here
  and why not, the sign-in under way (GitHub's code) and where sign-ins are kept;
  `forge.cancel_sign_in` stops one.

The applications (their client ids are public):

- GitLab: `GITLAB_CLIENT_ID` in `crates/forge/src/oauth.rs` (gitlab.com, non-confidential, scopes
  `api`, `read_api`, `read_user`; redirect URIs `https://texcraft-893d51.gitlab.io/`,
  `http://127.0.0.1:8768/` and `http://127.0.0.1:47321/callback`).
- GitHub: `GITHUB_CLIENT_ID` in the same file (an OAuth App with the device flow enabled; its
  callback URL `http://127.0.0.1:47321/callback` is unused by the device flow).
- A fork or another deployment sets its own there; an id ending in `_PENDING` turns signing in off.
  A web build served from another address needs that address registered as a redirect URI.

How it is built: `texcraft_forge::oauth` (PKCE, the authorize URL, the exchange, refresh and
revocation, `/user`, the device flow over an injected `sleep`, and the desktop's `Loopback`), all
over the `Transport`; `fake::Script` answers in tests. The engine's `cmd/forge_auth.rs` runs
sign-ins in the background (`Session::poll_forge` gives their outcome), keeps them in the
`Secrets` store the shell gives (`Session::set_forge_secrets`), and gives forge operations a key
(a token, or a sign-in renewed on the operation's thread). The web shell's `TabStorage` and its
return from GitLab are in `apps/texcraft-web/src/main.rs`.

Not yet: GitHub in the browser (it needs a token exchange on a server); the branch checked
when a stored browser project opens again; in the browser, the dialog the user was in when the page
left for GitLab is not opened again on return.

### Connections: Settings › Accounts (done)

One list of every place TeXCraft reaches for the user (docs/design/connections.md): each forge
(gitlab.com, github.com, each server added) with its sign-in or token, and each other git server
(Overleaf's, any host) with its token, read from the stores above; nothing new is stored but the
default connection's address (`default_connection` in the preferences). Each row says who is
signed in, its state (signed in, token, signed out, expired, signing in), where it is kept, and
has Sign In, Sign In Again, Allow Commits, Set as Default, Replace…, Disconnect… and Remove
Server…. Add a Connection… offers GitLab, GitHub, a GitLab server, a GitHub Enterprise Server,
Overleaf and another git server, then the sign-in, the server's registration or a token; a
forge's token is checked (`/user`, who it is for) before it is kept. Import a Project opens on
the default connection's card, and Save Project to… on it.

- `accounts.list`: the rows (never a token), the default, where things are kept, what can be
  added here and with which commands.
- `accounts.add {kind, host?, token, background?}`: a token connection.
- `accounts.disconnect {instance | host}`: signs out (revoking on GitLab), forgets the tokens kept
  and given; linked projects stay, and ask at their next sync.
- `accounts.set_default {instance | null}`.

Everything kept by earlier versions (sign-ins, tokens, servers, hosts) is listed as it is, with
no new sign-in.

### Self-managed servers (done)

A company's or a university's GitLab, or a GitHub Enterprise Server, has no TeXCraft application
until someone registers one there: the user for themselves, or the administrator for everyone.
TeXCraft says exactly what to register, then signs in there as on gitlab.com.

- **Adding a server** (Settings › Accounts › Add a Connection… › GitLab Server or GitHub
  Enterprise Server; or, in the Open dialog, "Add git.example.org for Signing In…" when the project's host
  has no application yet):
  1. The user types the server's address (any page on it will do: its scheme and host are kept;
     HTTPS only, but on `localhost`). gitlab.com and github.com are known already.
  2. TeXCraft checks it (`forge.check_server {address, service}`): GitLab's `GET /api/v4/version`
     (its version, or a 401 with GitLab's JSON, which most servers answer without a token: either
     proves GitLab), or a GitHub Enterprise Server's `GET /api/v3/meta` (its `installed_version`,
     or a 401 with GitHub's JSON in private mode). Another answer is "does not answer as a GitLab
     server", in words. In the browser a request that gets no answer is most often a server that
     does not answer web pages (no CORS for this page's origin): it is said so, with "ask its
     administrator, or use the desktop app".
  3. The dialog shows what to register, each value with a Copy button: where (GitLab: Edit profile
     › Applications for oneself, Admin › Applications for everyone, each with a button opening the
     page; GitHub: Settings › Developer settings › OAuth Apps), the name `TeXCraft`, the redirect
     URIs (the desktop's `http://127.0.0.1:47321/callback`; in the browser also the page's own
     origin and `/`), Confidential left unticked, the scopes `api read_api read_user`; on GitHub the
     homepage and callback URL, and Enable Device Flow.
  4. The user pastes the Application ID (GitHub: the Client ID), and the server is added
     (`forge.add_server {address, service, client_id}`). Adding it again replaces the id.
  Sign In then works for it exactly as for gitlab.com (`forge.sign_in {service, instance}`): the
  desktop's loopback redirect or the browser's round trip (the pending sign-in remembers its
  instance), renewal with the refresh token, revocation on sign out. A GitHub Enterprise Server
  signs in with the device flow on its own host (`/login/device/code`,
  `/login/oauth/access_token`; who is signed in from `/api/v3/user`), on the desktop only.
- **What is kept**: the server (`{instance, service, client_id}`) in the preferences, which never
  hold a token (the client id of a non-confidential application is no secret); its sign-in in the
  keychain under `texcraft.forge.<host>`, as gitlab.com's. A sign-in remembers the application it
  was got by, so it is renewed and revoked with the same one. Settings › Accounts lists each server
  with its own Sign In or Sign Out, and Remove Server, which also signs out of it.
- **A shared link** proposes a server: `https://texcraft-893d51.gitlab.io/?gitlab=https://git.example.org/group/project&client_id=<id>`
  (or `?github=…`) in the browser; on the desktop the same link as the program's argument
  (`texcraft 'https://…?gitlab=…&client_id=…'`). A dialog asks first ("Add a Server from a Link?",
  saying which application, and what it replaces): a link cannot add one silently. Once added, the
  link's project opens in the Open dialog. The query is taken out of the address bar.
- **Servers provided by an organisation** (desktop): `texcraft-forges.toml`, read at start from
  the system-wide folder (`/etc/texcraft/` on Linux and other Unixes,
  `/Library/Application Support/TeXCraft/` on macOS, `%ProgramData%\TeXCraft\` on Windows), then
  from the preferences' folder; the first file to list a server wins:

  ```toml
  [[server]]
  address = "https://git.example.org"
  service = "gitlab"     # or "github" for a GitHub Enterprise Server; gitlab when left out
  client_id = "…"        # the application registered for everyone (Admin › Applications)
  ```

  Those servers show as "Provided by your organisation." and cannot be removed or given another id
  in the interface (nor by `forge.add_server`). An entry that cannot be a server is left out and
  said in Settings › Accounts.
- **Private certificate authorities** (desktop): the forges' HTTPS trusts the system's certificate
  store (rustls-native-certs) beside the bundled Mozilla roots (webpki-root-certs), so a server
  whose certificate comes from the organisation's own authority, installed on its computers, is
  reached. The browser uses its own store.
- `forge.accounts` lists the servers (`provided`, `signed_in`) and the organisation file's
  problems; `forge.remove_server {address}` removes one.

How it is built: `texcraft_forge::servers` (the address, the probe, the registration, the link's
proposal, the organisation's file); the engine's `cmd/forge_auth.rs` keeps the servers
(`Session::restore_forge_servers`, `forge_servers_saved`, `set_forge_provided_servers`) and finds
an instance's client id (`Session::forge_client_id`); the interface's `forge_server.rs` has the
dialogs; the desktop shell reads the organisation's file and the link argument, the web shell the
link's query.

Limits: a GitLab served under a path (`https://example.org/gitlab`) is not supported (the address
is the scheme and host); in the browser, a server that does not allow the page's origin (CORS)
cannot be used, and GitHub Enterprise Server keeps the token there as github.com does.

### Choosing a project and a branch (done)

Once signed in (or with a token typed), the Open dialog finds the project and its branch instead
of asking for them typed out.

- **Project suggestions.** The project field is an editable combo box. As the user types, once
  typing pauses for a quarter of a second, the projects the account reaches whose path holds the
  text are looked up in the background and listed in a popup under the field (so nothing in the
  dialog moves as they come): each with its path, whether it is Private, Internal or Public (in
  words, not by colour), when it was last active ("active 3 days ago") and its default branch.
  Down opens the list and moves through it, Up moves back, Enter chooses, Escape closes the list
  and keeps the text (a second Escape closes the dialog); the keyboard stays in the field, whose
  active descendant is the option chosen; the number of suggestions and the option chosen
  ("owner/paper, 2 of 5") are announced through a live region. A click chooses too. Choosing puts
  the project's address in the field and its default branch in the branch. A full address pasted
  works as before, and a host typed first (`git.example.org/lab/pa`) looks up on that server;
  otherwise the server is the public one, or a Server drop-down offers the servers added for the
  service. With neither a sign-in nor a token, the list says "Sign in to see your projects, or use
  a personal access token." and nothing is asked (rule 15).
  - GitHub: `GET /user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member`,
    page after page up to 500 repositories, read once and kept five minutes in the session, then
    filtered here as the user types (names that start with the text first). When nothing listed
    matches and the text has a `/` or three characters, `GET /search/repositories?q=<text> in:name`
    (or `<name> in:name user:<owner>` for `owner/name`) finds the others.
  - GitLab: `GET /api/v4/projects?membership=true&order_by=last_activity_at&per_page=50&search=<text>`
    (with `search_namespaces=true` for `group/name`). The full answer, not `simple=true`, which
    leaves out the visibility.
  - Only the last lookup typed is answered: one asked while another runs waits for it and replaces
    any other waiting, so typing fast sends two requests, not one per key.
- **Branch list.** Once a project is chosen, or the field is left with an address that reads as
  one, its branches are listed (GitHub `GET /repos/{owner}/{repo}/branches?per_page=100`, GitLab
  `GET /api/v4/projects/:id/repository/branches?per_page=100`, page after page up to 500; the
  default branch from the project) in a drop-down named Branch, the default one chosen and marked
  "(default)", protected ones "(protected)", with "Another branch…" to type one. While they load,
  and when they cannot be listed (with why: not found or no access, the rate limit and when to try
  again, the network), the branch is typed by hand, empty for the default one. The line under the
  branch is always there, so the dialog does not move under the pointer when it changes.
  The Commit dialog asks only for the name of a new branch, which no list offers.
- **For agents**: `forge.projects {service?, instance?, query?, token?}` lists `projects` (path,
  `web_url`, `visibility`, `private`, `default_branch`, `activity` in RFC 3339, `description`),
  with `truncated` and `searched`; `forge.branches {service?, instance?, project, token?}` lists
  `branches` (name, `protected`, `default`) with `default`. `project` is a path on the instance or
  an address. Both work signed in or with a token (kept for the session once it worked, as
  `forge.open` keeps it) and never return the token; with `background` they return at once and
  `Session::poll_forge_lookup` gives the outcome.

How it is built: `texcraft_forge::listing` (`Lister`: the user's repositories, the search, the
branches; `matching`, `find`), the engine's `cmd/forge_pick.rs` (the two commands, the listing
kept, the lookups' slots), the interface's `forge_pick.rs` (the field, its popup, the branch
drop-down).

Limits: GitHub lists at most 500 repositories and a search 50; GitLab's search answers 50 projects;
branches stop at 500 (said, with "Another branch…" to type one). A fine-grained GitHub token lists
only the repositories it was given.

## Step 5: push and pull for folders, Save Project to GitHub or GitLab (done)

A project in a folder is a git repository (C8). On the desktop it now goes to a forge and comes
back from it with git itself: its remotes, push and pull, over git's smart HTTP protocol. And a
project that is on no forge yet gets a repository made for it, in three steps: A, the repository
made; B, set as the folder's remote; C, the history pushed to it.

What it does:

- **Save Project to GitHub…** and **Save Project to GitLab…** (File menu, and the History panel
  when the folder has no remote; `forge.save_to_github`, `forge.save_to_gitlab`, or
  `forge.create_repository {service, instance?, owner?, name, private?, description?, push?,
  message?, token?}` for agents). The dialog asks for:
  - the server: gitlab.com or github.com, or a server added (Settings › Accounts);
  - the owner: the account itself, or an organisation (GitHub, `GET /user/orgs`) or a group in which
    the account is at least a developer (GitLab, `GET /groups?min_access_level=30`), listed once
    signed in or given a token (`forge.owners`);
  - the name, suggested from the folder's (`My Thesis` → `My-Thesis`: letters, digits, `.`, `-`
    and `_`, starting with a letter, a digit or `_`, not ending in `.git` or `.atom`, at most 100
    long), checked as it is typed, with why in words;
  - who can see it: Private (the default) or Public; a description;
  - the sign-in, or a token. Making a repository needs a sign-in that can write: GitLab's read-only
    sign-in offers "Allow Commits on GitLab" (the `api` scope), as the Commit dialog does; GitHub's
    sign-in (`repo`) can. A fine-grained GitHub token needs Administration read and write to make
    repositories, a classic one the repo scope; a GitLab token the api scope.

  Then, on the desktop:
  - **A**: the repository is made empty and private (GitHub `POST /user/repos` or
    `POST /orgs/{org}/repos` with `auto_init: false`; GitLab `POST /api/v4/projects` with its
    `namespace_id`, `visibility` and `initialize_with_readme: false`). A name the owner has already
    is said as such ("ada has a repository named paper on GitHub already: choose another name"),
    and the dialog stays open with nothing changed; a project that has an `origin` already is
    refused before anything is made.
  - before it, the project is made ready to push: saved if it has unsaved changes (a repository
    TeXCraft made commits on save), a `.gitignore` of the build's files written when the folder
    has none (`.retex/` and the endings import leaves out: `*.aux`, `*.log`, `*.synctex.gz`…), and
    what is not committed committed with the message given ("Saved from TeXCraft" by default).
  - **B**: the repository's HTTPS address is set as `origin` in `.git/config`.
  - **C**: the whole history of the branch checked out is pushed to the same branch there, and
    the branch follows it (`branch.main.remote = origin`, `branch.main.merge = refs/heads/main`).
    A push that fails after the repository was made is said in the dialog; `origin` is set, so
    Push in the History panel tries again.
  - The dialog then says where the project is, with Open in Browser for the repository's page.

  In the browser no git protocol reaches the forges (they answer git's endpoints without CORS),
  so the repository is made through the API and the files are its **first commit**: on GitLab one
  commit of `create` actions to `main` of the empty project; on GitHub, whose Git Data API cannot
  write into an empty repository, the repository is made with a README, and a commit of the files
  with no parent (blobs, a tree, the commit) replaces it (the branch moved with `force`). The
  project is then linked to it as if opened from it, so Commit and Update work as in steps 1 to 3.
  The versions kept in the browser stay there: the dialog says the files go as one commit.
- **Remotes** (`git.remotes`, `git.set_remote {name, url}`): written with gix's configuration
  writer into `.git/config` (through `config.lock`, as git does), so the git command line sees
  them, with `remote.<name>.fetch = +refs/heads/*:refs/remotes/<name>/*`. An SSH address of a
  server (`git@github.com:owner/name.git`, `ssh://…`) is reached over HTTPS at the same path;
  plain HTTP only on this computer. `git.remotes` gives the remotes, the upstream, and how many
  commits the branch has that the remote's branch (as last fetched or pushed) has not, and the
  other way.
- **The History panel** shows the remote (its page as a link), "2 commits to push", "1 commit to
  pull", "Up to date with the remote, as last pushed or pulled" or "Not pushed yet", and Push and
  Pull, each off with why when it cannot run; a live region says what runs.
- **Push** (File menu, the History panel; `git.push {remote?, branch?, token?, background?}`):
  the branch's upstream, else `origin` and the branch checked out. The project must be saved and
  committed (it says which first). The server's refs are read; a branch there with commits the
  project does not have is refused before anything is sent: "The remote's branch main has commits
  this project does not have: pull first, then push." Otherwise the objects the server lacks go in
  one pack, and the server's report is read: a ref it refuses (a protected branch, a hook) is said
  with its reason.
- **Pull** (File menu, the History panel; `git.pull {remote?, branch?, token?, background?}`): the
  remote's branch is fetched (anonymously when no key is known for the server, for a public
  repository); then
  - nothing new: "Up to date";
  - the project's branch is behind: a fast-forward, the files on disk and in the editor changed;
  - both have commits the other has not: a merge commit (two parents). A file changed on one side
    takes that side's; a text file changed on both is merged by line, then by word, with the merge
    of step 2 (`texcraft_forge::merge::merge3`); anything else changed on both sides (the same words, a file
    deleted on one side and changed on the other, binary files) refuses the pull, naming the files,
    with nothing changed.
  - A file changed on disk and not committed (outside TeXCraft), or one in the way of a file the
    pull adds, stops the pull before anything is written.
- **Credentials**: the sign-in or the token for the remote's server (gitlab.com, github.com, a
  server added; another host is taken as a GitLab), as HTTP basic authentication: `oauth2` and the
  token for GitLab, `x-access-token` and the token for GitHub. Never in a log, an error or a
  command's result (tests look for them).

How it is built:

- `crates/gitwire` (`texcraft-gitwire`), new, below the engine: git's smart HTTP protocol, the
  client side, protocol version 0 (stateless), over any blocking HTTP client (`Http`; the engine
  gives it the forges' ureq transport, with the same certificates). gix 0.91 has no push, and its
  fetch needs its own HTTP stack (curl, or reqwest), so the crate speaks the protocol itself and
  uses gix for the repository:
  - `pktline`: packet lines (`0000` flush), side-band channels (data, progress, error).
  - `advertise`: `GET …/info/refs?service=git-receive-pack` (or `git-upload-pack`), checked to be
    git's advertisement (its content type), the refs and capabilities read, an empty repository's
    `capabilities^{}` understood.
  - `pack`: the objects the server lacks (the walk from the commit to send stops at the commits the
    server has; the objects of those at the edge are not sent again) and a version 2 pack of them,
    whole objects zlib-compressed with no deltas (text projects are small), its SHA-1 at the end.
  - `push`: `POST …/git-receive-pack` with `<old> <new> <ref>\0report-status side-band-64k agent=…`,
    a flush, the pack; the report read (`unpack ok`, `ok <ref>` or `ng <ref> <why>`).
  - `fetch`: `POST …/git-upload-pack` with `want <tip> side-band-64k ofs-delta no-progress`, up to
    256 `have` lines from the branch and its remote-tracking ref, and `done` in one request; the
    `NAK`/`ACK` lines skipped, the pack (no thin pack, so every delta's base is in it) indexed into
    `objects/pack` with gix-pack's `Bundle::write_to_directory`.
  - `fake::Server`: a git server in memory over a bare repository, built from the same pieces, for
    tests (credentials required, refusals, side-band or not).
  It knows nothing of TeXCraft: it SHOULD move to storytold/craft-libs (AGENTS.md rule 9), in a PR
  of its own there.
- `texcraft-history`: remotes and the upstream in the configuration, refs, ahead and behind,
  `fast_forward` and `merge` (a three-way merge of the trees, the text merge given by the caller)
  that move the work tree's files with the branch and write the index, so `git status` is clean
  after them.
- `texcraft-forge`'s `create`: the owners, the name's check and suggestion, the repository made,
  the browser's first commit; the fake forge makes repositories (`Fake::account`).
- The engine's `cmd/git.rs` (commands, the background task, `Session::poll_forge` gives the
  outcomes: `git_push`, `git_pull`, `create_repository`, `owners`), `cmd/git/desktop.rs` (the
  folder's side); the interface's `forge_save.rs` (the dialog) and the History panel's remote.

Limits: HTTPS only (SSH addresses are reached over HTTPS); one branch at a time, the one checked
out; no force push from the interface; packs without deltas, so a large history pushes more bytes
than git would; a pull's merge is per file, by line then by word, as in step 2, with no conflict markers (a
file that does not merge is settled with git or by keeping one side); in the browser the history
goes as one commit.

### New Project on a forge (done)

New Project's second step, "Where to Keep It" (docs/design/new-project.md), offers each forge
connection that can make repositories beside This Computer / This Browser. On a forge the
repository is made first with Save Project to…'s code (`Creator::create`; a taken name or a
missing permission ends it there, nothing written), then on the desktop the template is written
into the folder, committed with a `.gitignore` (`prepare_push`), `origin` set and the branch
pushed with `git.push`'s path, the upstream set once it is; in the browser the template's files
are the repository's first commit (`Creator::first_commit`) and the library's project is linked
to it. A push that fails keeps the project and the repository and says why (`push_error`); the
repository is never removed. Commands: `project.from_template {template, folder, forge: {service,
instance?, owner?, name, visibility?, token?}, background?}` (operation `new_project`), and
`forge.name_available {service, instance?, owner?, name}` (GitLab `GET /projects/:path`, GitHub
`GET /repos/:owner/:name`: found means taken). Visibility: private (default), internal (GitLab
only), public. Overleaf has no API to make projects: the step says to make it there, then import.

## Step 6: more

- Files over the limits fetched when the engine reads them; a diff of the changes before
  committing; conflict markers or a side-by-side view to merge a file by hand.
- Several branches of a folder: switching, and pushing a new one.
