# The review files (`.texcraft/review/`)

TeXCraft keeps a project's review beside its sources, in the hidden folder `.texcraft/review/`, so it travels with the project (in git, in an archive) and outlives TeXCraft: the files are small JSON a script can read. The build never reads `.texcraft/`, and the project's file list does not show it. Projects without a folder (the browser, a new unsaved project) keep their review in memory until they are saved.

This page describes format **version 1**, written by `texcraft-review` (`crates/review/src/format.rs`).

## Files

- One file per thread, named after the thread's id: `.texcraft/review/01JD3Q8Z5X6V7W8Y9ZABCDEFGH.json`. Two people commenting on different passages never change the same file, so they never conflict in git.
- A file changes only when its thread does: a new message, an edit, a resolution, a rename of its source file, or a save of the project that moved its passage.
- Pretty-printed UTF-8 JSON with a final newline.

## A thread

```json
{
  "version": 1,
  "kind": "thread",
  "id": "01JD3Q8Z5X6V7W8Y9ZABCDEFGH",
  "anchor": {
    "path": "chapters/intro.tex",
    "start": 1204,
    "end": 1229,
    "quote": {
      "exact": "the quick brown fox jumps",
      "prefix": "As every typist knows, ",
      "suffix": " over the lazy dog.\n",
      "line": 37
    }
  },
  "state": { "state": "open" },
  "messages": [
    {
      "id": "01JD3Q8Z5X6V7W8Y9ZABCDEFGJ",
      "author": { "name": "Ada", "key": "3f2a9c0177de4b10…" },
      "time": "2026-11-22T14:03:11.204Z",
      "text": "Is this the sentence we want?"
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `version` | The format version. A file with a higher version than the reader knows is **refused with a message** (and left alone), never read partly. A missing version is an error. |
| `kind` | `thread`. (Suggestions, R2, will be `suggestion`.) |
| `id` | A [ULID](https://github.com/ulid/spec): 48 bits of milliseconds and 80 random bits, in Crockford base 32. Ids sort by creation time and do not repeat across people. |
| `anchor.path` | The source file, as a project path (`/` separated, relative to the project folder). |
| `anchor.start`, `anchor.end` | The passage when the file was written: character offsets (Unicode scalar values, from 0) in the source as it was then; `end` is excluded. |
| `anchor.quote.exact` | The passage's text. |
| `anchor.quote.prefix`, `.suffix` | Up to 32 characters before and after it. |
| `anchor.quote.line` | The line it started on, from 1: a hint for finding it again. |
| `anchor.placement` | Absent when the anchor is on its passage. `removed`: the passage was deleted; the anchor is empty where it was, and the quote keeps the text. `unanchored`: the passage was not found when the file was read. |
| `state` | `{"state": "open"}`, or `{"state": "resolved", "by": <author>, "at": <time>}`. |
| `messages` | Oldest first. |
| `messages[].author` | `name`: the display name, which may repeat. `key`: the author's ed25519 public key in hex, which tells two people of the same name apart; empty for an author without one (an agent, for now). |
| `messages[].time`, `edited` | RFC 3339 times in UTC; `edited` only after the text changed. |
| `messages[].deleted` | `true` once its author deleted it; the text is then empty, and the message keeps its place. |

Unknown fields are ignored, so a later minor addition does not need a new version.

## Finding the passage again

While a file is open, the anchor follows every edit (`file.edit`, typing, an agent's change): text typed before it moves it, text typed inside it grows it, and text typed just before or after it stays outside. A passage deleted entirely leaves an empty anchor marked `removed`; the thread is never dropped. Saving the project renews each quote from the text.

When a project is opened, each anchor is placed on the file as read:
1. at its saved range, when the text there equals `quote.exact`;
2. otherwise at the occurrence of `quote.exact` nearest `quote.line`;
3. otherwise nowhere: the thread is `unanchored` and keeps its quote.

Matching by the context and fuzzy matching, for files edited heavily elsewhere, come with milestone R4 (docs/collaboration-roadmap.md).

## Identity

No accounts: on the first comment TeXCraft asks for a display name (prefilled from the system's user name) and makes an ed25519 key pair. Both are kept in the preferences (`config.toml`); **the secret key is stored there in hex, unencrypted**, until TeXCraft uses the system's key store. Only the public key goes into review files. Settings › Review changes the name and shows the key as a short fingerprint (its first 16 hex digits). Signing messages with the key is not done yet; for now the key only tells authors apart.
