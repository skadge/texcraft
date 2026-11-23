# Roadmap: review and working together

> **Status:** plan, 2026-11-22. F1 and R1 are done; the rest is to do. The design is in [`collaboration.md`](collaboration.md); this page orders the work. Each milestone ends with something a user can use. Each lists its deliverables, its tests, and how we know it is done.

## Decisions (operator, 2026-11-22)

| # | Question | Decision | What it means for the work |
|---|---|---|---|
| 1 | The suite's contracts (RFC-0020) | Follow RFC-0020's principles, so a later move to the suite's contracts is easy. | TeXCraft's own traits copy the RFC's shapes: a live session, `Texts`, presence, invites, conversations. Every RFC-0020 constraint and conformance case applies, as far as a serverless design allows; departures are written down where they happen. No app code names a transport or a CRDT library. Moving the traits to craft-libs is a later, separate step, not part of this roadmap. |
| 2 | The CRDT | yrs. | yrs is the one engine, behind `Texts`. Its binary format and sync protocol are Yjs's, so sessions with retex's web editor stay possible. A differential test against a second engine is kept as RFC-0020 asks. |
| 3 | Servers | No service to maintain. A small relay may be hosted for testing only. | Production runs on peers and public infrastructure only: public Nostr and MQTT relays for meeting, free STUN, direct connections, and the public relays themselves as the slow last resort for those who cannot connect directly. A relay we host is a test tool behind the same transport trait, never a requirement, and nothing depends on it. |
| 4 | Where review data lives | In a hidden folder next to the sources, added to git. | `.texcraft/review/`, one file per thread or suggestion, committed with the project. The build ignores it. |
| 5 | Identity | No accounts to start. | A display name and a per-person key made on first use. Accounts can be added behind the same author field later. |
| 6 | Order | Review first; it is useful on its own, without collaboration. | Milestones R1 to R6 (review) come before L1 to L6 (live editing). Review must be complete and pleasant for one person alone, with no network at all. |

## Overview

| Milestone | What a user gets | Depends on |
|---|---|---|
| **F1** Edits, not texts | Nothing visible: the base for everything below | Phase 2's buffer |
| **R1** Comments | Comment on any passage, threads in a margin, resolve | F1 |
| **R2** Suggested changes | Suggest mode, accept and reject, preview with suggestions | R1 |
| **R3** Review on the PDF | Comment from the PDF; comments shown on the PDF | R1, SyncTeX |
| **R4** Review that lasts | Re-anchoring after outside edits, unanchored threads, history | R1 |
| **R5** Review for others | Export as a list and as PDF annotations; review by agents | R1–R4 |
| **R6** Review with git | Open, commit and pull a repository; review travels with it | R4 |
| **L1** The shared document | Live editing between two windows on one machine (tests, demo) | F1, R1, R2 |
| **L2** Sessions | Invites, presence, others' carets, follow | L1 |
| **L3** On one network | Live editing between machines on a local network | L2 |
| **L4** Anywhere | Live editing over the internet, with no server of ours | L3 |
| **L5** Live with git | A session keeps the repository: auto-commit, others' pushes merged | L4, R6 |
| **L6** With the browser | TeXCraft's web build in sessions; sessions with retex's web editor | L4, plan phase 6 |

Later, outside this roadmap: cloud backends and named-people sharing (`craft-cloud`, `craft_ui::share`), one-time-key invites, notifications of mentions, and moving the traits to craft-libs.

## Crates

| Crate | Layer | What it holds |
|---|---|---|
| `texcraft-review` | L0 (pure, builds for wasm32) | The model (threads, messages, suggestions, authors), anchors (quote selectors, re-anchoring), the `.texcraft/review/` file format (read, write, version), merging two review states. No I/O: the engine reads and writes the files. |
| `texcraft-collab` | L1 | The traits shaped like RFC-0020's (`Session`, `Texts`, `Presence`, `Invites`, `Conversations`), the yrs engine behind them, the project as one shared document, the sync protocol, the encrypted envelope, invites and roles. No sockets. |
| `texcraft-net` | L1, native and web | Transports behind one trait: loopback (tests), a test relay client, local network (mDNS and TCP), WebRTC (str0m on the desktop, the browser's own on the web), meeting over Nostr and MQTT. |
| engine, ui-egui | existing | Commands (`review.*`, `session.*`), the buffer bridge, the margin, suggest mode, presence, carets. |

`cargo xtask layers` gets the new crates. Nothing below `ui-egui` depends on egui.

---

## F1: Edits, not texts

**Status:** **done** (2026-11-22). `file.edit` takes several edits in character offsets of the text before the call, with an optional `origin`; the session's edit log (`Session::edits_since`) is what the editor and the review anchors follow. `texcraft-buffer` keeps each change with its origin (`Buffer::take_edits`) and takes others' edits through `Buffer::receive`, which rebases the undo and redo histories over them. Where a peer's text lands among redone letters, when both were typed at one place, is a guess (no tombstones until the CRDT).

**Why.** Review anchors, suggestions and the CRDT all need to know what changed, not only the new text.

**Deliverables:**
- `file.edit {path, edits: [{start, end, text}]}` (character offsets, as the editor's) in the engine. `file.write` stays for whole files and computes a minimal diff.
- The buffer emits each transaction's edits with an **origin**: local typing, undo or redo, remote (a peer), review (accepting a suggestion), external (a file changed on disk, a git pull).
- Undo groups skip remote and external edits; they are undone only by their author.
- The session keeps per-file edit notifications that subscribers (review anchors, the CRDT bridge) read.
- The editor sends edits through `file.edit` instead of whole texts.

**Tests:**
- A sequence of `file.edit` calls gives the same text as applying them to a string.
- Undo after a remote edit undoes only the local change, and the remote text stays.
- `file.write` of a slightly changed text produces a small edit, not a whole replacement.

**Done when:** the editor and the agents use edits, and all existing tests pass.

---

## R1: Comments

**Status:** **done** (2026-11-22), with these limits: anchors are ranges in the session's text that follow its edit log (not buffer marks, which live in the interface); on load a passage is found at its saved range or by its exact quote nearest its line, and context and fuzzy matching wait for R4; the editor has no context menu yet, so Comment is in the Edit menu, on Cmd+Alt+M and `:comment`; the key pair is not used to sign yet. Format: [`review-format.md`](review-format.md).

**Deliverables:**
- **`texcraft-review`:**
  - `Thread { id, anchor, messages, state }`, `Message { id, author, time, text, edited, deleted }` and `Author { name, key }`.
  - `Anchor` with a live range (two marks in the buffer) and a saved quote selector (exact text, 32 characters before and after, line hint).
  - The file format: `.texcraft/review/<id>.json`, with a `version` field, documented in `docs/review-format.md`. Ids are random 128-bit values (ULIDs, time-ordered).
- **Identity:**
  - On first use, TeXCraft asks for a display name (prefilled from the system's user name) and makes an ed25519 key pair (RustCrypto, Apache-2.0/MIT), kept with the preferences.
  - A Settings entry changes the name. The key is shown as a short fingerprint.
- **Engine:**
  - Review state per project, loaded with the project and written when changed (each thread to its own file).
  - Commands: `review.list {path?, state?, author?}`, `review.comment {path, start, end, text}`, `review.reply`, `review.edit`, `review.delete`, `review.resolve`, `review.reopen`.
- **Interface:**
  - Comment (Cmd+Alt+M, the Edit menu, the editor's context menu, `:comment` in Vim) on a selection opens a composer in the margin.
  - **The margin:** a column right of the editor with each thread aligned to its passage, shifted down to avoid overlaps, and a thin line to the highlight. Selecting either selects the other. It folds into a Review panel (a rail entry) when the pane is narrower than about 760 px.
  - Highlighted passages (a soft tint of the author's colour, a stronger tint for the selected thread), and a gutter marker per thread.
  - A thread shows the author's badge (the `craft_ui::presence` colour), name, relative time, text, Reply, Resolve, and a menu with Edit and Delete for one's own messages.
  - Next and previous thread (F8 and Shift+F8); the filter (open, resolved, mine).
  - Counts in the status bar and per file in the file tree.
- **Accessibility:**
  - Each thread is a group named by its quote and author.
  - The highlight carries a description ("commented by …").
  - New replies and resolutions are announced.
  - The whole flow works by keyboard.

**Tests:**
- Review model round-trip: write, read, compare; an unknown newer `version` is refused with a message.
- Anchors follow local edits before, inside and after them (marks). A passage deleted entirely leaves an empty anchor that is flagged, not dropped.
- kittest: commenting on a selection shows the thread in the margin, Resolve folds it, the filter hides it.
- Screenshots of the margin, light and dark, checked for the look.

**Done when:** one person can comment on their own document, close and reopen it, and find the threads where they left them.

---

## R2: Suggested changes

**Deliverables:**
- **`texcraft-review`:** `Suggestion { id, author, time, anchor, replacement, state, thread? }`.
- **Suggest mode,** a toggle beside Source and Formatted (and `:set suggest` in Vim):
  - Typing does not change the file. It creates or extends a suggestion at the caret: consecutive typing extends the same suggestion.
  - Deleting creates a deletion suggestion over the deleted range.
  - Replacing a selection is one suggestion.
- **Drawing:**
  - Insertions are drawn in the text, underlined in the author's colour, without being in the buffer. The view lays them out as virtual text.
  - Deletions are struck through.
  - A suggestion's card in the margin shows the change ("Replace “colour” with “color”"), Accept, Reject, and Reply (which opens a thread on it).
- **Commands:** `review.suggest {path, start, end, text}`, `review.accept {id}`, `review.reject {id}`, `review.accept_all {path?, author?}` and `review.reject_all`.
  - Accepting applies the edit as one undoable change with the review origin.
  - Accept All is one undoable change.
- **Overlapping suggestions:** accepting one marks those it overlaps as outdated, shown and still rejectable, never applied silently.
- **Preview with suggestions,** a View option: the build gets the sources with the open suggestions applied as overlaid files, without writing them.
- **Building:** the normal build ignores open suggestions.

**Tests:**
- Typing in suggest mode leaves the file unchanged, and accepting gives the text as if typed.
- Accept All in a file with independent suggestions equals applying them in any order. Overlaps are reported as outdated.
- Undo of an accept restores the text and reopens the suggestion.
- kittest of suggest mode, and screenshots.

**Done when:** a co-author can suggest wording changes in a file, and the author can accept or reject each, with a correct PDF either way.

---

## R3: Review on the PDF

**Deliverables:**
- **Commenting from the PDF:** in the PDF pane, select a region (drag with Alt) or choose Comment from the context menu on a word.
  - SyncTeX's inverse search turns the region into a source range. The anchor stores the source quote and the page and rectangle.
  - When SyncTeX finds no source (a figure, a float), the thread anchors on the nearest source line and keeps the region.
- **Showing comments on the PDF:**
  - Markers in the PDF margin at each thread's place (forward search), in the author's colour.
  - Clicking one selects the thread and its passage.
  - The markers follow rebuilds.
- **A reading mode,** PDF only with the Review panel: a reviewer reads the PDF and comments without seeing the source.

**Tests:**
- Through the engine with a fake SyncTeX (as `tests/it/sync.rs`): a region gives the expected source quote, and the marker of a thread returns to its region.
- kittest: a comment made on the PDF appears in the margin of the editor.

**Done when:** a reviewer who never opens the source can comment on the PDF, and the author sees those comments on the right lines.

---

## R4: Review that lasts

**Deliverables:**
- **Re-anchoring on load:**
  - First, the exact text with its context near the line hint.
  - Then the exact text alone, nearest the hint.
  - Then the best fuzzy match: a bitap or diff score above a threshold, with an agreed tolerance for whitespace changes and moved lines.
  - A thread found by fuzzy match is marked "moved"; the margin says so until someone looks.
- **Unanchored threads:** a section at the top of the panel with each quote, the file, and Re-anchor (select a passage and attach). Never dropped.
- **Watching for outside changes:** a file changed on disk while open (another editor) is reloaded as an external edit, and anchors follow when the change is small. Otherwise they are re-found by quote.
- **History in a thread:** who resolved and reopened it, and when. Edited messages keep their previous text, shown on request.
- **Deleted text:** a thread whose passage was deleted (by its author or by accepting a suggestion) stays, marked "passage removed", with the quote.

**Tests:**
- A mutation sweep over the examples and retex's samples: random edits that keep or destroy a quote. Every anchor either lands on text equal to its quote, lands on a near match marked moved, or goes to Unanchored. None lands on wrong text.
- Outside edits: write the file from the test as another editor would; anchors follow.

**Done when:** a project edited in another editor, or rewritten heavily, keeps every comment, either in place or clearly unanchored.

---

## R5: Review for others

**Deliverables:**
- **Export:**
  - A review report as Markdown or PDF (built by retex from a generated document): threads grouped by file and section, with quotes, messages and states.
  - The built PDF with the comments as PDF text annotations, through craft-libs' `xfdf` (or direct annotation writing), for readers outside TeXCraft.
- **Importing PDF annotations:** comments a reader made on the PDF in another reader become threads, anchored through SyncTeX. A PDF reviewer needs no TeXCraft.
- **Agents (MCP):**
  - `review.*` available to agents within their roots.
  - Comments by agents carry the agent's name and are marked as written by an agent.
  - A documented "review this chapter" flow for an AI reviewer that comments and suggests, never edits.
- **Mentions:** `@name` in a message highlights it, and the filter "mentioning me" works locally. Notifications wait for cloud backends.

**Tests:**
- Export of a project with threads gives a report naming every open thread.
- The annotated PDF has one annotation per anchored thread, at its page.
- Importing an annotated PDF (a fixture made by us, with provenance) gives threads at the right source lines.
- An MCP test: an agent comments and suggests, and refuses to write outside its roots.

**Done when:** a supervisor without TeXCraft can review the PDF in their usual reader and the author gets their comments in place; an AI reviewer can leave comments and suggestions through MCP.

---

## R6: Review with git

**Deliverables** (plan phase 7 item 2, with review in it):
- **Repositories:**
  - Open a folder that is a git repository: branch, changes, last commit in the status bar.
  - Commit (message, staged files) and push.
  - Pull with a three-way merge per file, and a conflict view with a choice per hunk.
- **Library:** gitoxide (`gix`, MIT/Apache-2.0) for local repositories. HTTPS remotes with the user's credentials (credential helper); SSH through the user's agent.
- **Review files in git:**
  - `.texcraft/review/` is staged with the sources. TeXCraft adds it to the commit by default, and says so.
  - Two people reviewing different passages never conflict (one file per thread).
  - The same thread changed on both sides is merged by the review crate: messages united by id; the state from the later change; anchors re-found.
- **Forges:** GitLab and GitHub through their HTTP APIs, as retex's GitLab work does: open a project by URL, sign in with OAuth (PKCE, no secret), merge requests later.

**Tests:**
- A repository in a temporary folder: two clones review different passages, push and pull, and both see all threads.
- The same thread replied to on both sides is merged with both replies.
- A conflicting source change shows the conflict view; choosing a side gives a clean file.

**Done when:** two co-authors who never use live editing can write and review a paper through git, with comments and suggestions travelling with the repository.

---

## L1: The shared document

**Deliverables:**
- **`texcraft-collab` traits,** shaped like RFC-0020's: `Session` (join, leave, state, ordered commands with scope locks and a lock-free text path), `Texts` (open a text, apply local edits, receive remote ones), `Presence` (ephemeral latest-wins state per peer and kind) and `Invites`. A `Conversations` hook stays absent for now: chat belongs to the host.
- **The yrs engine behind `Texts`:**
  - The project document: `files` (path to `Y.Text`; binary files as SHA-256 references), `review` (threads and suggestions as maps), `meta` (main file, shared build options, the project id).
  - The project id is a stable UUID kept in `.texcraft/project.json`, never a path (RFC-0020 constraint 5).
- **The buffer bridge:** local edits (F1) become yrs edits; remote yrs updates become remote-origin buffer transactions. Live anchors switch to yrs sticky indices while a session runs, and back to marks and quotes afterwards.
- **Per-person undo** (yrs `UndoManager` tracking the local origin).
- **Structural changes:**
  - Renaming, deleting and adding files go through ordered commands with a scope lock per path (the RFC's two models).
  - Text typed into a file another peer deleted is kept as a recoverable copy and shown to its author (RFC-0020's recovery rule).
- **The sync protocol:** yrs's state-vector exchange (sync steps 1 and 2) for joining and catching up. Updates are idempotent, and replay never applies twice.
- **A loopback transport and two windows on one machine** (`--join` of a local session), for development and demos.

**Tests** (each a named conformance case from RFC-0020 where one exists):
- **Convergence:** 2 to 5 replicas, random concurrent edit traces, delivery shuffled with duplicates and drops; identical texts, review maps and anchors. Property tests with fixed seeds.
- **Differential:** the same traces through loro (dev-dependency only); identical final texts.
- **Delete while typing:** the delete wins everywhere, and the typist's text is kept locally.
- **Replay:** a snapshot plus updates already in it gives no double apply. Reconnecting with the same identity keeps it.
- **Undo:** two peers type; each undo removes only one's own text.

**Done when:** two TeXCraft windows on one machine edit the same project at once and always agree.

---

## L2: Sessions

**Deliverables:**
- **The encrypted envelope:**
  - A room key: 256 bits, random.
  - Every frame encrypted with AES-256-GCM (RustCrypto) and signed with the sender's per-session ed25519 key.
  - The room's public name derived from the key with HKDF.
- **Invites:**
  - Two links per session: edit and review-only, as `texcraft://join#room=…&k=…&role=…`.
  - The desktop app registers the `texcraft:` scheme (Linux `.desktop`, macOS Info.plist, Windows registry through the installer).
  - Share › Invite to Edit Live and Invite to Review copy the links.
  - The link is the permission; revoking means moving to a new key (Share › Reset Links), and the UI says that plainly.
- **Roles:**
  - A review-only peer's updates are accepted only on the `review` map. Every peer enforces this; frames are signed with keys the link's role authorises.
  - Suggest mode is forced for review-only peers.
- **Presence** (ephemeral, latest-wins per peer and kind, never saved; RFC-0020 surface 1):
  - Badges in the top bar (`craft_ui::presence`).
  - Others' carets and selections in their colour, with a name flag that fades when idle.
  - Files others have open, marked in the file tree.
  - Their place in the PDF (forward search), as a mark.
  - Follow a peer (click their badge): your view tracks their file and caret until you move.
- **Session status:** connected, reconnecting, offline with your changes kept, waiting for someone. Changes made offline merge on reconnect.
- **Commands:** `session.share`, `session.join {link}`, `session.leave`, `session.peers`, `session.follow`.
- **Replica ids:** random 64-bit per session, as Yjs; a clash is detected on join (two peers announcing one id) and the newcomer re-keys. This departs from RFC-0020 constraint 3 (ids granted by a relay), since no relay is assumed; it is recorded as such.

**Tests:**
- The envelope round-trips. A frame with a wrong key or tampered bytes is refused. A review-only peer's edit to a source is refused by every other peer.
- Presence is never written into the project.
- An id clash on join is detected and resolved.
- kittest of the presence bar and carets.

**Done when:** a session can be shared by link with edit or review rights, and everyone sees who is where.

---

## L3: On one network

**Deliverables:**
- **`texcraft-net` local transport:**
  - mDNS announces the room's public name (never the key).
  - Peers connect over TCP with the envelope.
  - A mesh: each peer connects to every other, which is fine up to about ten.
- **Joining without the project:** the joiner receives the project's files from a peer (sources through the CRDT, binary files by hash, in chunks) and chooses where to keep it.
- **Leaving and rejoining:** the project stays on disk; rejoining syncs by state vectors.

**Tests:**
- Two processes on loopback addresses with mDNS find each other, join and converge.
- A process killed during a session: the others continue. Restarting it rejoins without double apply.
- Large binary files transfer intact (hash checked) without blocking the text.

**Done when:** two laptops on the same Wi-Fi can write together with nothing else running.

---

## L4: Anywhere

**Deliverables:**
- **Meeting over public relays:**
  - The room's public name is published as a topic. Offers and answers (WebRTC session descriptions and candidates), encrypted, go through public Nostr relays (`nostr` crate) and public MQTT brokers, several of each at once.
  - The relay list is shipped with the app, updatable from settings, and kept current per release.
- **Direct connections:**
  - WebRTC data channels with `str0m` (sans-IO) on the desktop, driven by TeXCraft's own I/O thread.
  - STUN from free public servers.
  - No TURN by default (a service to maintain); a TURN server can be configured in settings by those who have one.
- **When direct fails** (symmetric NATs): the envelope's frames go through the meeting relays themselves. This is slow and limited in size, enough for text and presence, not for large figures, which then wait for a direct path or are announced as missing.
- **Waiting for someone:** a link opened while nobody is online says so. If the joiner can reach the project's repository (R6), it opens from the last commit and syncs when someone arrives.
- **The test relay** (decision 3): a small relay binary in the repository (`xtask` or `tools/test-relay`), WebSocket, passing encrypted frames by room. Used only by tests and by the operator for trials, behind the same transport trait. Documented as not a service.

**Tests:**
- An in-process relay (the test relay's code) with two peers that cannot connect directly: frames pass and peers converge.
- Signalling over a local Nostr relay test double: offers are exchanged encrypted, and the relay never sees plaintext (asserted on its logs).
- Manual checks against real public relays and two networks (home and mobile), listed in the release checklist, with results recorded.

**Done when:** two people on different networks write together, with no server of ours, and the app says clearly when it cannot connect.

---

## L5: Live with git

**Deliverables:**
- **The committer** (`collaboration-severin.md`):
  - In a session on a repository, one participant commits: the earliest joiner still present, recomputed when someone leaves.
  - It commits every 10 seconds when there are changes, with a message naming the session's participants ("TeXCraft session with A, B"), and pushes when the user allowed it.
  - The other participants never commit during the session.
- **Pushes from outside:**
  - The committer fetches every 30 seconds. A new commit not made by the session is merged three-way against the session's base: the diff becomes remote edits in the CRDT, so everyone sees it.
  - What cannot merge (a binary file, a file deleted while someone types) is shown as a conflict with both versions kept.
- **Two sessions on one repository:** a commit made by another TeXCraft session is recognised by its message trailer, and its participants are offered to join each other's session.

**Tests:**
- A session of two peers on a temporary repository: commits happen only from the committer, at most one per interval; the committer leaving hands the role over.
- An outside commit during a session appears in both peers' texts. A conflicting one shows the conflict view.

**Done when:** a live session keeps the repository up to date, and a co-author using their own editor and git stays in the loop.

---

## L6: With the browser

**Deliverables:**
- **TeXCraft's web build in sessions:** the browser's `RTCPeerConnection` through web-sys, signalling over WebSocket to the same public relays, storage in IndexedDB.
- **Sessions with retex's web editor:** agree with retex on the wire format:
  - the room naming;
  - the envelope (key derivation, cipher, signing);
  - the yrs/Yjs document layout (file map, binary references);
  - the presence fields.
  
  Record it in a shared note in both repositories, and add a test session between the two.

**Tests:**
- A wasm test of the transport against the test relay.
- A two-client test, TeXCraft desktop and TeXCraft web, through the test relay, run in CI where a browser is available.

**Done when:** someone in a browser and someone in TeXCraft on their desktop write together.

---

## Risks

| Risk | What we do |
|---|---|
| Public relays disappear or limit use | Several kinds at once; the list in settings, updated per release; LAN and git keep working without them. |
| Users who cannot connect directly | Frames through the meeting relays (slow); TURN in settings for those who have one; git as the fallback for working together. |
| Re-anchoring lands on wrong text | Quote verification on every anchor; "moved" marking; the mutation sweep in R4 is a release gate. |
| The link is the only permission | Said plainly in the Share dialog; Reset Links; review-only links. |
| yrs as a dependency | MIT, maintained, used widely. The differential test against loro catches engine defects. `Texts` keeps it replaceable. |
| Departures from RFC-0020 (self-assigned ids, no relay sequencing) | Written down where they occur (L2, L4), and revisited when the suite's contracts land. |
| Scope: review becomes a product of its own | R1–R4 are the core. Export and import (R5) and forges (R6) can slip without blocking live editing. |

## New dependencies (all to check against the licence allowlist when added)

| Crate | Licence | For |
|---|---|---|
| `yrs` | MIT | The CRDT (L1) |
| `loro` | MIT | Differential tests only (L1, dev-dependency) |
| `ed25519-dalek`, `aes-gcm`, `hkdf`, `sha2` | Apache-2.0 OR MIT / BSD-3-Clause | Keys, envelope, room names (R1, L2) |
| `ulid` | MIT | Review ids (R1) |
| `gix` | MIT OR Apache-2.0 | Git (R6) |
| `mdns-sd` | Apache-2.0 OR MIT | Local discovery (L3) |
| `str0m` | MIT OR Apache-2.0 | WebRTC on the desktop (L4) |
| `nostr` | MIT | Meeting over Nostr relays (L4) |
| `rumqttc` | Apache-2.0 | Meeting over MQTT brokers (L4) |
