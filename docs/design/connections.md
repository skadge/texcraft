# Connections (Settings › Accounts)

> **Status:** built (next release) · **Roadmap:** C5 and C11 (docs/ROADMAP.md) · **Written:** 2026-11-27

One place lists every remote service TeXCraft can reach for the user, how it reaches it, and lets
them add, sign in to again, disconnect or remove it, and choose the default. Import a Project,
Save Project to GitLab/GitHub… and the coming New Project "Where" step read the same list, so a
connection made in one shows in all of them.

This design is our own, from docs/design.md and docs/design/import-dialog.md (the two should feel
like one family: the same marks, card shapes, state lines and words).

## The model

A **connection** is one place projects live, and what TeXCraft keeps to reach it. There are two
kinds, from what already exists (no new store):

| Kind | One per | What reaches it | Where it is kept |
|---|---|---|---|
| **Forge** | gitlab.com, github.com, each self-managed GitLab or GitHub Enterprise Server | a sign-in (OAuth grant), or a personal access token | the grant: the store's `texcraft.forge.<host>`; the token: `texcraft.git.<host>`; the server itself (address, application id): the preferences (`forge_servers`) or the organisation's `texcraft-forges.toml` |
| **Git server** | Overleaf's git server, any other HTTPS git host | a token (git's password) | `texcraft.git.<host>`; the host, not the token, in the preferences (`git_hosts`) |

The store is the system's keychain on the desktop, the tab's session storage in the browser, or
the session's memory when neither works. A token typed for one operation and not remembered is a
connection for this session only.

A forge row exists when it is signed in, has a token, is being signed in to, or is a server added;
gitlab.com and github.com appear only once connected (Add a Connection… offers them). A git server
row exists when a token is kept or given for it.

**State** of a row, in words and a badge (never colour alone): Signed in · Token · Signed out ·
Expired (the access token ran out and cannot be renewed: sign in again) · Signing in…. A GitLab
sign-in that can only read says "Read only until the first commit" (committing asks for `api`).

**The default** is one forge connection (gitlab.com, github.com or a server added), the one that
can create repositories and host projects. It is kept in the preferences as its address
(`default_connection`); none is chosen until the user chooses one. Git servers cannot be the
default (Overleaf has no API to create projects).

## The page

Settings › Accounts keeps its name (the operator's choice: "accounts" is what people look for).

- **Header:** one muted line saying what connections are for, and **Add a Connection…** (accent).
- **Empty:** a card with the `link` icon, "Connect TeXCraft to where your projects live", two
  lines (import, sync and save projects; nothing is shared until you connect), and a large
  **Add a Connection…**.
- **Groups:** "GitLab and GitHub" (forges) first, then "Other git servers" (Overleaf, any host).
- **Row:** a card (8 px radius, hairline border, `surface`) like Import's: the service's mark (the
  Simple Icons marks of the Import cards; ours, `server`/`link`, for a server or host), the name
  (GitLab, GitHub, the server's host) and its address under it; at the right the badges (Default,
  then the state). A second line: the account's initial in a circle (the forges' APIs give an
  avatar URL, but fetching it would be one more request per Settings open; the initial is enough)
  and "Ada Lovelace (@ada)", then "· Opens and commits" or "· Read only until the first commit",
  then where it is kept ("Kept in the system's keychain", "Kept in this tab", "For this session").
  Then the row's actions, right-aligned, as buttons whose names end with the row's host:
  - **Set as Default** (forges that are not the default), **Sign In**, **Sign In Again** (expired,
    or read only: "Allow Commits"), **Replace Token…** (a token row), **Disconnect…**,
    **Remove Server…** (servers the user added; a server the organisation provides says so).
- **Disconnect…** asks first, in a dialog naming what goes: "Disconnect from gitlab.com? TeXCraft
  signs out (GitLab revokes the sign-in; on GitHub, remove TeXCraft under Settings › Applications)
  and removes the token from the keychain. Projects linked to it stay as they are; their next Sync
  asks you to sign in." **Disconnect** (danger) / Cancel. Remove Server… asks the same, and also
  forgets the server's application.
- **Problems:** a keychain that cannot be read, and entries the organisation's file left out, in
  `error` under the list.

**Add a Connection…** opens a dialog with the same cards as Import's grid (title and line): GitLab,
GitHub, GitLab Server, GitHub Enterprise Server, Overleaf, Other Git Server. Then:

- GitLab, GitHub: the existing sign-in part (Sign In with GitLab, GitHub's code) or "Use a token
  instead", whose token is checked against the forge (`/user`) and kept.
- GitLab Server, GitHub Enterprise Server: the existing server dialog (check, register, add),
  then sign in from its row.
- Overleaf, Other Git Server: host (Overleaf's is fixed) and token, kept.

## Browser

The same list and dialog. Sign-ins are kept in the tab's session storage, and the rows say so.
GitHub signs in with a token only (its device flow does not answer web pages), and GitHub
Enterprise, Overleaf and other git servers are shown in Add a Connection… as unavailable, "Desktop
app only", with Import's reason ("git servers do not answer web pages"): focusable, and their page
explains.

## Who uses it

- **Import a Project:** a card for a connected service says "Signed in as @ada" (as before) and
  goes straight to the project search; the default connection's card has the keyboard when the
  dialog opens and says "Default" on its line; a GitLab Server card opens on the default server.
  A card without a connection offers the sign-in inline, and the sign-in lands in the same store,
  so the row appears in Settings.
- **Save Project to GitLab/GitHub…:** the server list shows each connection of the service with
  who is signed in, the default first and chosen.
- **New Project's "Where" step** (next): `accounts.list`'s rows with `can_create`, and `default`.

## Commands

| Command | What |
|---|---|
| `accounts.list` | the rows (`id`, `kind`, `service`, `instance`, `host`, `state`, `user`, `can_commit`, `scopes`, `kept`, `default`, `can_default`, `can_create`, `provided`, `removable`, `sign_in`), `default`, `kept`, `signing_in`, `store_error`, `server_problems`, and `add`: the kinds that can be added here, with the commands that add them. Never a token. |
| `accounts.add` | a token connection: `{kind: gitlab\|github\|server\|overleaf\|git, host?, token}`; a forge's token is checked (`/user`) in the background first. Signing in stays `forge.sign_in`; servers stay `forge.check_server` and `forge.add_server`. |
| `accounts.disconnect` | `{host}` or `{instance}`: signs out (revoking where the service allows), forgets the token kept and the session's; asks nothing (the interface asks). |
| `accounts.set_default` | `{instance}` (or `null`): the default connection. |
| `forge.remove_server` | as before, now also forgetting a token kept for the server and the default if it was. |

## Existing users

Nothing moves: the grants, tokens, servers and hosts stay where they are and under the same names,
so every sign-in kept shows in the list after the update, with no new sign-in. "Forget" becomes
Disconnect… (it also signs out); "Replace…" stays. No default is chosen for them.
