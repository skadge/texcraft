# Working together: live editing and review

> **Status:** plan, 2026-11-22. Nothing built yet. It details `docs/plan.md` phase 7, items 3 to 5, and replaces their one-line sketches once the operator has settled the decisions at the end.

Overleaf's two collaborative features are the ones people choose it for:
- several people typing in the same project at once;
- reviewing: comments on a passage, and suggested changes the author accepts or rejects.

TeXCraft needs both. It should also keep what makes it TeXCraft: a desktop app that works offline, projects that are ordinary folders (often git repositories), and no server of ours if we can avoid one.

## 1. What exists to build on

**In the suite (craft-libs `main`, 33be356, and craft-contrib):**

| Piece | State | What TeXCraft takes from it |
|---|---|---|
| RFC-0020, live collaboration | Accepted. Its contracts (a live-session contract, presence, invites, a conversation contract, the `Texts` API) are not in craft-libs yet. | The contract shapes and the hard constraints (§4). Text through a mature CRDT behind `Texts`, not `craft-crdt`. Chat belongs to the host, never the editor. |
| `craft-cloud` (RFC-0024) | In craft-libs: `Files`, `Sharing`, `Contacts`, `Notifications`, `Accounts`, `Conversations`, each optional. | Cloud projects, sharing with named people, notifications of comments and mentions. |
| `craft_ui::presence` | In craft-libs: a row of badges, one colour per peer derived from its id. | The top bar's "who is here", and the same colour for each peer's caret. |
| `craft_ui::share` | In craft-libs: the suite's share dialog behind a `ShareBackend`. | Share with named people when a cloud backend exists. A link-only invite is TeXCraft's own (§5.3). |
| `craft-suite-account` (RFC-0025) | In craft-libs. | Accounts, when a backend wants them. Live editing must not require one. |

Nothing in the suite covers comments, suggestions or review. That part is new. If it proves general, it should move to craft-libs (rule 9).

**In retex:**
- `docs/design/collaboration.md` plans editing together between browsers with no server: Yjs, peers found through public relays (Nostr, MQTT), WebRTC between them, end-to-end encryption with the key in the link's fragment, and "waiting for someone who has the project" when nobody is online.
- `docs/design/gitlab.md` (in progress) opens GitLab repositories in the browser and commits back.
- `docs/design/collaboration-severin.md` sets the project model:
  - one project is one git repository;
  - in a live session, one participant commits, every few seconds;
  - people who prefer their own editor work through git, and the session pulls their pushes.

**In TeXCraft:**
- `texcraft-buffer` has transactions, grouped undo and marks that follow edits, designed for this.
- `file.edit` with several edits (plan phase 2, item 1) is still to do. It is the first prerequisite.
- SyncTeX navigation exists, so a comment made on the PDF can be anchored in the source.

## 2. Principles

1. **The `.tex` files stay the truth.** Live editing and review never put anything in the sources the author did not type. Review data lives beside them (§6.3), not as `\todo` markup.
2. **It works alone, then with others.** Review is useful with nobody else online: comment, suggest, resolve, commit. Live editing is a layer on top of it.
3. **No server of ours by default.** Peers meet through public relays and talk directly, encrypted end to end (retex's design). A store or relay is an option a deployment adds behind the same contracts, never a requirement.
4. **One protocol with retex's browser editor.** Someone in TeXCraft and someone in retex's web editor can be in the same session. This decides the CRDT (§5.1).
5. **Contracts first, engines behind them** (RFC-0020). TeXCraft depends on traits shaped like the suite's. The CRDT, the transport and the store are replaceable.
6. **Nobody's typing disappears** (RFC-0020's recovery rule). Text that cannot be placed, and a comment whose passage was deleted, are kept and shown, never dropped.
7. **Everything goes through commands** (rule 14). Agents can review: list, comment, suggest, resolve. An AI reviewer posting comments through MCP is a first-class use.

## 3. What the user sees

**Live editing:**
- **Joining.** Share › "Invite to edit live" gives a link (§5.3). Opening it in TeXCraft, or in retex's web editor, joins the project.
- **Who is here.** The top bar shows the presence badges (`craft_ui::presence`), each with the file and line the person is on. Clicking a badge follows that person.
- **Others' carets and selections** appear in their colour, with their name on a small flag that fades when idle. The file tree marks files others have open.
- **The PDF.** Each participant builds their own PDF (the engine is deterministic, so everyone gets the same one). Others' places can show as marks in the PDF, through SyncTeX.
- **Undo** undoes only your own changes.
- **Status.** A clear state in the status bar: connected, reconnecting, offline with changes kept, or waiting for someone who has the project.

**Review** (works alone, live, or through git):
- **Comments.**
  - Select text and choose Comment (Cmd+Alt+M; `:comment` in Vim). In formatted mode, the same on the shown text. In the PDF, select a region or double-click with Alt: the comment anchors on the source passage SyncTeX finds, and keeps the region as a fallback.
  - A margin beside the editor shows the threads aligned with their passages, which are highlighted. In a narrow window it becomes a Review panel on the right.
  - A thread has replies, @mentions (notified through `craft-cloud` when a backend exists), resolve and reopen, and edit or delete of one's own messages.
- **Suggested changes.**
  - In Suggest mode (a toggle beside Source/Formatted; `:set suggest`), typing does not change the file. It records suggestions: insertions underlined in the author's colour, deletions struck through, in place.
  - Each suggestion has Accept and Reject. Accept All and Reject All work per file and per author.
  - Accepting applies the edit as an ordinary undoable change.
  - "Preview with suggestions" builds the PDF with them applied, through the build's overlay of unsaved files, without touching the sources.
- **Navigating.**
  - Next and previous comment or suggestion.
  - A filter: open, resolved, mine, mentioning me, by author.
  - Counts in the status bar and per file in the tree.
  - Everything reachable by keyboard and announced (rule 21).
- **History.** Resolved threads stay, folded, with who resolved them. Review can be exported: a list (Markdown or PDF), or comments as PDF annotations on the built PDF, through craft-libs' `xfdf` for readers outside TeXCraft.

## 4. The suite's hard constraints, applied

RFC-0020's constraints were written for a relay. With peers and no server, they become:

| RFC-0020 | In TeXCraft |
|---|---|
| A document is served by one relay. | A session is one room, keyed by a stable project id (never a path). Relays only carry encrypted frames. |
| Sequence state is reconciled with a persistent counter on join. | Text is lock-free CRDT, so it needs no sequencing. The durable state is the last commit plus the CRDT's state vector (§6.2). A joining peer syncs by state vectors (yrs sync step 1 and 2), which is idempotent. |
| CRDT replica ids are granted by the relay, never self-assigned. | Without a relay, ids are random 64-bit values per session, as Yjs does. Collisions are improbable but not impossible, so we detect a clash on join (two peers announcing the same id) and re-key. This departs from the RFC; the departure goes in the contracts PR. |
| Opaque frames pass through unparsed. | Frames are encrypted end to end, so relays cannot parse them anyway. |
| Structural commands win over text in removed scopes. | Deleting or renaming a file while someone types in it: the delete wins everywhere, and the typist's text is kept locally as a recoverable copy and shown to them. |

## 5. Live editing: the design

### 5.1 The CRDT: yrs

**yrs** (MIT), the Rust port of Yjs, is the recommended engine behind the `Texts` contract:
- **It shares Yjs's binary format and sync protocol,** so TeXCraft and retex's web editor (planned on Yjs) can share a room. Loro and diamond-types cannot do that.
- **Its relative positions** (`StickyIndex`) anchor carets, selections, comments and suggestions so they follow concurrent edits.
- **Its `UndoManager`** tracks origins, which gives each person an undo of their own changes.
- **It has awareness** (Yjs's presence state) and builds for wasm32, so the web build runs the same code.
- **RFC-0020 names it** among the mature libraries. A differential test against a second engine (loro) is part of the contracts work (§9).

**The project as one document:**
- `files`: a map from path to `Y.Text` for text files. Binary files (figures) are not merged: the map holds the SHA-256 of their contents, the bytes are sent whole to the peers that lack them, and replacing a file is a new hash, last writer wins (as retex's note).
- `review`: comment threads and suggestions (§6.1).
- `meta`: main document, project settings that are shared (the build options), the project id.

**The buffer stays the editor's model.** Local transactions go to the CRDT as yrs edits. Remote updates come back as ordinary transactions on the buffer, marked remote, so undo, marks, the lint and the build keep working unchanged. This is where `file.edit` (phase 2) is needed: edits, not whole texts.

### 5.2 Transport and discovery

Behind a `Transport` trait, so a hosted relay can replace any of this:
1. **Direct.** WebRTC data channels:
   - on the desktop, `str0m` (MIT/Apache-2.0, sans-IO WebRTC in Rust);
   - in the browser, the browser's own `RTCPeerConnection` through web-sys.
2. **Meeting (signalling).** Offers and addresses go through public relays, several kinds at once for redundancy (Nostr relays first, through the `nostr` crate; MQTT brokers second), with the room name derived from the key (HKDF). This follows retex's design, so both can meet in one room once the wire format is agreed with retex.
3. **When direct fails** (symmetric NAT, 10–25% of connections):
   - Free STUN for the attempt.
   - TURN as an option a deployment configures.
   - Failing that, fall back to relaying the encrypted frames through the signalling relays themselves, which is slow but enough for text.
4. **On a local network**, peers find each other with mDNS and connect directly, with no relay.

**Encryption.** Every frame is encrypted with the room key (AES-256-GCM, RustCrypto `aes-gcm`, Apache-2.0/MIT), whatever the transport.

### 5.3 Invites and permission

- **The link is the permission** (retex's model): `texcraft://join#room=…&k=…`, or the web editor's URL with the same fragment. The desktop app registers the URL scheme.
- **Edit or comment.** Two keys per room give two links: one to edit, one to review only. A comment-only peer's updates are accepted only on the `review` map. Every peer enforces this, since frames are signed with a per-session key the link's role authorises.
- **Revoking** means moving the project to a new key (re-invite). The UI says that plainly.
- **RFC-0020's one-time-key invites** (redeemed once, idempotent, expiry) need a backend that remembers redemption. They come with a cloud backend through the suite's invite contract, not in the serverless mode.

### 5.4 When nobody is online

The project lives on every participant's disk, and in git:
- **Opening a link with nobody online** shows "waiting for someone who has this project". If the project is a repository the joiner can reach, it opens from the last commit instead, and syncs when someone arrives.
- **Persistence is git** (`collaboration-severin.md`):
  - In a session, the participant who joined first (the lowest id among those present, recomputed when someone leaves) commits every 10 seconds when there are changes. The message names the session's participants.
  - Others do not commit, which avoids divergent commits.
  - Without git, each participant's folder is saved as now.
- **Someone working outside TeXCraft pushes.** The committer pulls, merges the commit's diff into the CRDT as a remote edit (three-way, against the commit the session started from), and the change appears in everyone's editor. A conflict that the CRDT cannot express (a binary file, a deleted file someone is typing in) is shown as such, with both versions kept.
- **Two TeXCraft users editing the same repository without a shared session** see, on pull, that the last commit came from another TeXCraft session, and are offered to join it (`collaboration-severin.md`'s last point).

## 6. Review: the design

### 6.1 The model

A **thread**:
- an id;
- an anchor;
- messages, each with an author, time (`chrono`), text with @mentions, and edited and deleted marks;
- a state: open, or resolved by someone at some time.

A **suggestion**:
- an id, an author and a time;
- an anchor (the range it replaces; empty for a pure insertion);
- the replacement text;
- a state: open, accepted or rejected, by whom;
- optionally a thread, for discussing it.

Authors are display names plus a stable per-person key (an ed25519 public key made on first use and kept in the preferences), so names can repeat without confusion. With a backend, an account identity can replace it.

### 6.2 Anchors

- **Live:** a pair of yrs `StickyIndex` values in the file's text. They follow every concurrent edit.
- **Saved:** a quote selector, the standard way to re-find a passage in a changed text:
  - the exact text;
  - 32 characters of context before and after;
  - the line as a hint;
  - for PDF comments, also the page and rectangle.
- **Re-anchoring on load:**
  - first the exact text with its context near the hint;
  - then the exact text alone;
  - then a fuzzy match above a threshold.
  - A thread that cannot be placed goes to an "Unanchored" list with its quote. It is never dropped.
- **Editing outside TeXCraft.** Someone who edits a file in another editor and commits changes the text under the anchors. Re-anchoring on the next load is what makes this survive.

### 6.3 Where review data lives

Live, it is in the project's CRDT document (`review`), so everyone sees comments as they are written. Saved, it is in the project, beside the sources:
- **`.texcraft/review/<thread-id>.json`**, one file per thread or suggestion. One file each means two people commenting on different passages never conflict in git. A thread's file changes only when that thread does.
- **The format is documented**, with a version number. It is small JSON a script can read, so a project's review survives TeXCraft.
- **The build never reads `.texcraft/`.**
- **Projects that should not carry review data** can keep it outside the folder instead (a setting): in the user's data folder, keyed by the project id, and then not shared through git.

### 6.4 In the interface

- **In the editor:**
  - highlighted passages;
  - markers in the gutter;
  - suggestions drawn inline (insertions underlined, deletions struck through) and kept out of the text the build sees.
- **In formatted mode,** the same on the shown text, once formatted editing exists (phase 3).
- **In the PDF,** comment markers at their places through SyncTeX, so a reviewer can read the PDF and see the threads.
- **The margin** shows threads beside their passages, shifted to avoid overlaps, with a line to the highlight. Selecting a highlight selects its thread and the reverse. It folds into a panel when the pane is narrow.
- **Commands for agents and automation:**
  - `review.list`, `review.comment`, `review.reply`, `review.resolve`, `review.reopen`, `review.delete`;
  - `review.suggest`, `review.accept`, `review.reject`;
  - `review.export`;
  - `session.share`, `session.join`, `session.leave`, `session.peers`.

## 7. Phases

Each phase ends with something usable, in this order:

| Phase | What | Needs |
|---|---|---|
| **C1. Edits, not texts** | `file.edit` with several edits. The buffer emits edits with their origin. A remote transaction kind that undo skips. | Phase 2's buffer (done) |
| **C2. Review, alone** | The review model, anchors with quote selectors and re-anchoring, `.texcraft/review/` files, comments and threads in the editor and the margin, resolve, filters, navigation, the `review.*` commands. Suggest mode with accept and reject, and the preview with suggestions through the build's overlay. Comments on the PDF through SyncTeX. Export to a list and to PDF annotations (`xfdf`). | C1 |
| **C3. Git** | Opening a repository, committing, pulling with a three-way merge, conflicts shown (plan phase 7 item 2). Review files travel with the repository, so review works between people who only use git. | C2 for the review part |
| **C4. The CRDT, in process** | The project as a yrs document behind a `Texts`-shaped trait. The buffer bridged both ways. Per-person undo. Live anchors as sticky indices. Tests: two and three sessions in one process exchanging updates in random orders converge (texts, review, binary references); a differential test against loro; the RFC-0020 recovery cases (delete while typing, double apply, replay after restart). | C1, C2 |
| **C5. Live, on one network** | The transport trait, encryption, invites with edit and comment links, presence (badges, carets, selections, follow), mDNS discovery, the session status, the committer role and auto-commit. | C3, C4 |
| **C6. Live, anywhere** | Signalling over Nostr and MQTT relays, WebRTC with str0m, STUN and optional TURN, relayed frames as the last resort, "waiting for someone", joining from the last commit. | C5 |
| **C7. With the browser** | The same in TeXCraft's web build. An agreed wire format with retex's web editor, and a test session between the two. | C6, plan phase 6 |
| **C8. Cloud backends** | `craft-cloud` projects, the share dialog with named people, notifications of mentions, one-time-key invites through the suite's contract, a hosted relay or store as an option. | RFC-0020's contracts in craft-libs |

C2 and C3 are worth shipping before any live editing. Asynchronous review over git is how many papers are actually written.

## 8. Tests and quality

- **Convergence.** Random concurrent edit traces on 2 to 5 replicas, delivered in random orders with duplicates and drops, must give identical texts, review maps and anchors. Run as property tests with a fixed seed list.
- **Differential.** yrs against loro on the same traces, as RFC-0020 asks of any engine.
- **Recovery cases**, named in RFC-0020:
  - delete a file while another peer types in it;
  - replay after a restart;
  - a snapshot that already contains the operations being replayed;
  - reconnecting with the same identity.
- **Re-anchoring.** Mutation sweeps on real documents, checking that anchors either land on their quote or go to Unanchored, never somewhere wrong.
- **Interface.** kittest tests for the margin, suggest mode and accepting, with screenshots for the look. The accessibility tree is checked for threads and suggestions.
- **Network.** A loopback transport and an in-process relay for tests. Real-relay tests are manual and listed in the release checklist.

## 9. Decisions for the operator

1. **The RFC-0020 contracts PR.** The live-session contract and the `Texts` API are accepted but not written. TeXCraft would be their first real user.
   - **Recommended:** TeXCraft writes them, with the in-process mock and the differential oracle the RFC asks for. Until then TeXCraft builds against the same shapes in its own crate, and the PR moves them to craft-libs.
   - The other options are to wait for someone else, or to keep TeXCraft's own trait.
2. **The CRDT.** yrs, for interoperability with retex's web editor (§5.1), or loro.
3. **No server of ours,** as above: public relays and direct connections. Or a small relay we run, which is simpler and more reliable but is a service to operate. Recommended: serverless by default, with the transport trait open to a relay later.
4. **Where review data lives:** `.texcraft/review/` in the project (shared through git; recommended), or outside the project by default.
5. **Identity without accounts:** display names with per-person keys, as above, or accounts from the start.
6. **The order:** review (C2, C3) before live editing (C4 to C6), as recommended, or live editing first.
