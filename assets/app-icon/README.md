# TeXCraft app icon

**Creature:** a blackbird, TeXCraft's mascot, in profile, looking right.

**Style:** the Crafting Apps' engraving portrait: a tight head-and-shoulders crop on a full-bleed field, the body running off the tile's left edge. A thin rim in the field's colour runs along the tile's edge, over the drawing.

**Palette:**

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | the bird (it is a black bird) |
| Paper | `#efe9dc` | the engraving's light lines: wing feathers, beak, eye |
| TeXCraft gold (app colour) | `#d6bc25` | the full-bleed field and the rim |

**Tile:** `viewBox="0 0 512 512"`, a rounded square with `rx=112` that clips everything. Windows and Linux use the tile edge to edge. macOS icons put it on Apple's grid (the tile is 824/1024 of a transparent canvas).

**Provenance:** the project owner's original drawing, `source/blackbird.png` (1535 × 1024, kept unchanged). The master `texcraft.svg` places it on the tile and maps its black and white to ink and paper with an SVG filter, so the drawing is not traced. Licence: [LICENSE.txt](LICENSE.txt).

## Changing it

Edit `texcraft.svg`: the field's colour, the `<image>`'s position and size (the framing), or the filter's tables (the threshold between ink and paper). Then run `cargo xtask icons`, which renders every file below from it with resvg. Small sizes are drawn at 1024 pixels or more and averaged down, so the hatching becomes tone.

## Files

| File | What | Used by |
|---|---|---|
| `texcraft.svg` | master (the tile, the field, the drawing placed and filtered) | source for everything below |
| `source/blackbird.png` | the owner's drawing | `texcraft.svg` |
| `texcraft-1024.png` | full tile, 1024 px | docs, store listings |
| `texcraft-macos-512.png` | tile with Apple's margin, 512 px | the window and Dock icon on macOS |
| `texcraft.ico` | Windows icon, 16 to 256 px | Windows packaging |
| `hicolor/<size>/apps/io.gitlab.severin_lemaignan.TeXCraft.png` | Linux icon theme, 16 to 512 px | Linux packaging; the 256 is the window icon on Windows and Linux, the 64 the mark in the top bar |
| `hicolor/scalable/apps/io.gitlab.severin_lemaignan.TeXCraft.svg` | the master with the drawing embedded | Linux icon theme |

A `.icns` for macOS needs Apple's `iconutil` and is made with the macOS packaging.
