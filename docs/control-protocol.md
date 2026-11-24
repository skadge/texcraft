# Control protocol

> **Last updated:** 2026-11-22 · **Change:** `ui.screenshot`

Agents, scripts and tests drive a running TeXCraft window through a loopback connection. It reaches the same commands the menus run (AGENTS.md rule 14). For agents without a window, `texcraft-cli mcp` offers the same commands over MCP, confined to its automation roots.

## Starting it

The channel is off unless the launch asks for it:

```sh
texcraft --control            # the first free port from 7841
texcraft --control 9000       # exactly this port
TEXCRAFT_CONTROL_PORT=9000 texcraft
```

It listens on 127.0.0.1 only. The transport is craft-control's: one JSON object per line in each direction, lines up to 4 MiB.

## The token

Each launch makes a random token (64 hex digits), or takes `TEXCRAFT_CONTROL_TOKEN`. It is written to `<data folder>/TeXCraft/control/<port>.token`, readable by the user only; `current.port` names the port of the last window started. On Linux the data folder is `~/.local/share`. The files are removed when the window closes normally.

The first line of a connection carries the token:

```json
{"token": "3f9c…"}
```

It is answered with `{"ok": true, "result": {"app": "TeXCraft", "version": "…", "protocol": "docs/control-protocol.md"}}`. A connection without the right token is refused and closed.

## Requests

```json
{"id": 1, "method": "engine.execute", "params": {"command": "build.compile", "params": {"wait": true}}}
```

Each reply carries the request's `id` and either `"ok": true` with a `result`, or `"ok": false` with an `error` in words.

| Method | Parameters | Result |
|---|---|---|
| `engine.commands` | none | Every command: `id`, `label`, `menu`, `shortcut`, `params` (a JSON schema, for engine commands), `enabled` and `disabledReason`. Interface commands (view, edit, help) are marked `"interface": true`. |
| `engine.execute` | `command`, optional `params` | Runs an engine command, or an interface command as its menu entry runs it. The result is the command's own. A disabled command answers with its reason as the error. |
| `document.inspect` | none | The project: folder, main document, files, open files, the shown file, unsaved changes, revision, and `build` with the last build's result and diagnostics. |
| `document.revision` | none | `document` (the project's folder, or `untitled`) and its `revision`, which grows with every change. |
| `ui.screenshot` | optional `path` | The window's next frame as a PNG: written to `path` (the result gives the path and the size), or returned inline as base64 in `data` with its `mime`, `width` and `height`. |
| `ui.inspect` | none | What the window shows: `layout`, `side`, `log`, the open `dialog`, `theme`, the caret's `cursor`, the `status` line, the `preview` page, page count and zoom, and whether it is `building`. |

craft-control's own methods (`control.capabilities`, `control.running`, `control.cancel`) work as craft-control documents them.

## Not yet

Synthetic input (`ui.key`, `ui.click`) and the web page's `window.texcraft` API are planned (docs/ROADMAP.md). Until then, `ui.inspect` and `ui.screenshot` are the ways to check the window's state.
