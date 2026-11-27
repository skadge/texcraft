# Releasing TeXCraft

A release is a tag `vX.Y.Z` on `main`. CI checks it, builds the Linux packages and the browser
build, uploads them to the project's package registry and makes the GitLab release. Tagging,
pushing the tag and putting the browser build on Pages are the operator's decisions; agents prepare
the commits only (AGENTS.md rule 4).

## Checklist

1. **Version.** Set `version` in `[workspace.package]` of `Cargo.toml` (every crate inherits it) and
   run `cargo check --workspace` so `Cargo.lock` follows. Versions before 1.0 bump the minor number
   for a release with new features.
2. **Progress log and roadmap.** Join the fragments of `docs/progress.d/` into `docs/progress.md`
   (RFC-0017): each fragment's text unchanged, newest first by the date of the commit that added it,
   then delete the fragment files. Until the craft-libs joiner (`cargo xtask progress --write`) is in
   this repository, this is done by hand. In `docs/ROADMAP.md`, mark the features the release
   completes `✅ X.Y.Z`, and update its status line.
3. **Changelog.** Add a `## X.Y.Z (date)` section at the top of `CHANGELOG.md`, written for users
   from the fragments, grouped by theme, with the known limits. The section's body becomes the
   GitLab release's description.
4. **Metainfo.** Add `<release version="X.Y.Z" date="...">` at the top of `<releases>` in
   `packaging/linux/io.gitlab.severin_lemaignan.TeXCraft.metainfo.xml` and check it:
   `appstreamcli validate --no-net packaging/linux/io.gitlab.severin_lemaignan.TeXCraft.metainfo.xml`.
5. **README.** Point "Downloads" at the releases page once the first release is out.
6. **Pins.** `CRAFT_FONTS_REV` in `.gitlab-ci.yml` and the craft-fonts `commit` in
   `packaging/flatpak/io.gitlab.severin_lemaignan.TeXCraft.yml` are the same commit. The tools' SHA-256 pins in
   `.gitlab-ci.yml` are filled (see "Pinned downloads").
7. **Checks.** `cargo xtask ci`, and `packaging/ci/release-notes.sh vX.Y.Z` prints the section (it
   fails when the tag, `Cargo.toml`, the changelog and the metainfo disagree).
8. **Try the packages from `main`** before tagging: start the `appimage`, `flatpak` and `web` jobs
   by hand in the latest `main` pipeline, download their artifacts, and do the manual checks below.
9. **Tag** `git tag -a vX.Y.Z -m "TeXCraft X.Y.Z"` and push the tag to `origin` (GitLab). Push it to
   the `github` mirror too if the mirror carries tags.
10. **Watch the tag's pipeline**, then check the release page and its four links.
11. **Pages** (optional): start the `pages` job of the tag's pipeline by hand to put the browser
    build at the project's Pages address.

## Windows and macOS (experimental)

GitHub Actions on the mirror (`.github/workflows/experimental-builds.yml`) builds them on each tag that GitLab's `mirror` job pushes, and attaches a Windows zip and an ad-hoc-signed macOS app to the tag's GitHub release. GitHub's hosted runners for those systems are free for the public mirror and spare the GitLab runner. The mirror's token cannot push workflow files: a change to `.github/workflows/` is pushed to GitHub over SSH (`git push github main`) before GitLab's mirror job pushes the same commit. Start a run by hand with `gh workflow run experimental-builds -R skadge/texcraft --ref main`.

## What CI does on a tag

| Stage | Job | What it does |
|---|---|---|
| check | `check` | `cargo xtask ci`: fmt, clippy, tests, layering, the wasm32 build. |
| check | `release-notes` | `packaging/ci/release-notes.sh`: the tag matches `Cargo.toml`, the changelog and the metainfo; writes the release description and `VERSION`. |
| package | `appimage` | Release build on Debian 12 with the craft-fonts faces (`CRAFT_FONTS_REQUIRED=1`), then `packaging/appimage/build-appimage.sh` with the pinned appimagetool and runtime. `TeXCraft-X.Y.Z-x86_64.AppImage`. |
| package | `flatpak` | `flatpak-cargo-generator` makes `cargo-sources.json` from `Cargo.lock`, `flatpak-builder` builds `packaging/flatpak/io.gitlab.severin_lemaignan.TeXCraft.yml` offline, the Flathub linter reports (not fatal), `flatpak build-bundle` writes `TeXCraft-X.Y.Z-x86_64.flatpak`. |
| package | `web` | `trunk build --release` in `apps/texcraft-web` (the `web` profile), packed as `texcraft-web-X.Y.Z.tar.gz`. |
| publish | `publish` | Uploads the three files and `SHA256SUMS` to the generic package registry, package `texcraft`, version `X.Y.Z`. |
| release | `release` | The GitLab release `TeXCraft X.Y.Z` with the changelog section and links to the uploaded files. |
| release | `pages` | By hand: the browser build on GitLab Pages. |

On `main`, `appimage`, `flatpak` and `web` can be started by hand; their files stay job artifacts
for 30 days and nothing is uploaded or released. Branches and merge requests run `check` only.

The packages embed the interface faces from storytold/craft-fonts (cloned inside the jobs at the
pinned commit) and the interface translations: until TeXCraft's cutover to storytold/craft-i18n,
its catalogs are `i18n/<locale>.tsv` in this repository, so every build embeds them and no job
clones craft-i18n for them. Setting `CRAFT_I18N_DIR` (absolute) to a craft-i18n checkout adds its
shared catalogs (`locales/<locale>/common.tsv`); `CRAFT_I18N_REQUIRED=1` makes a missing checkout
an error, for when the cutover moves the catalogs there.

## Pinned downloads

The jobs download their tools; each download is pinned by version (in the URL) and by SHA-256,
checked by `packaging/ci/fetch-pinned.sh` before anything runs it:

- `APPIMAGETOOL_URL` / `APPIMAGETOOL_SHA256`: appimagetool 1.9.1.
- `APPIMAGE_RUNTIME_URL` / `APPIMAGE_RUNTIME_SHA256`: the static type2 runtime, release 20251108 (a
  dated release, not the rolling "continuous" one). Moving to a newer one means downloading it,
  checking it, and pinning its SHA-256 here.

The SHA-256 variables start empty: the first run stops and prints each file's checksum. Check it
against the release page (and, for appimagetool, its signature), then write it into
`.gitlab-ci.yml`.

Trunk is built from crates.io at `TRUNK_VERSION` (Cargo checks the crates' checksums); it fetches
the wasm-bindgen that matches `Cargo.lock` and binaryen's wasm-opt itself. The Flatpak tools come
from Debian (flatpak-builder) and from Flathub's signed `org.flatpak.Builder` (the cargo source
generator and the linter); the job prints the Builder's version and commit.

## What the update check reads

TeXCraft's update check (`crates/engine/src/update.rs`, docs/ROADMAP.md A8) reads releases in
the shape this pipeline makes them, so keep that shape:

- GitLab's `GET /projects/severin-lemaignan%2Ftexcraft/releases/permalink/latest` (the latest
  release, not an upcoming one): `tag_name` `vX.Y.Z`, the changelog section as `description` (the
  What's New dialog shows it as text), `_links.self`, and the asset links named
  `TeXCraft-X.Y.Z-x86_64.AppImage`, `TeXCraft-X.Y.Z-x86_64.flatpak` and `SHA256SUMS` (with a
  line for the AppImage), their URLs on gitlab.com.
- When gitlab.com cannot be reached, the mirror's `GET /repos/skadge/texcraft/releases/latest`:
  the same tag, and the same files attached (GitHub's checksums are used there).
- Windows and macOS copies link `https://github.com/skadge/texcraft/releases/download/vX.Y.Z/`
  `TeXCraft-X.Y.Z-x86_64-windows.zip` and `TeXCraft-X.Y.Z-aarch64-macos.zip`.
- The browser build compares its version with `version.txt` beside its page, which
  `cargo xtask version-worker` (Trunk's post-build hook) writes.

An AppImage updates itself only from a release whose AppImage has its line in `SHA256SUMS`. To
try an update by hand, set `TEXCRAFT_PRETEND_VERSION` to an older version (craft-update's
convention) before starting a released AppImage: it is then offered the latest release.

## The packages

### AppImage

- Built on Debian 12, so it needs glibc 2.36 or newer: Debian 12, Ubuntu 24.04, Fedora 37 and
  later; not Ubuntu 22.04 (glibc 2.35). The job prints the oldest glibc the binaries need. For
  Ubuntu 22.04, build on `ubuntu:22.04` with rustup instead.
- Bundles no libraries: the binaries link only glibc and open the display and graphics libraries
  at run time (libwayland-client or libX11, libxkbcommon, libvulkan or libEGL, and the system's
  driver), which every desktop has. The runtime is the static type2 runtime, so libfuse2 is not
  needed; FUSE 3 (`fusermount3`) is, or run it with `--appimage-extract-and-run`.
- Run through a link named `texcraft-cli`, it is the command-line tool:
  `ln -s TeXCraft-X.Y.Z-x86_64.AppImage texcraft-cli; ./texcraft-cli mcp --automation-read-root DIR --automation-write-root DIR`.
- The build worker is the app started again from `std::env::current_exe()`, which is
  `usr/bin/texcraft` inside the mounted image; the mount lasts while the app runs.
- TeX Live: the binaries carry retex's bundle of TeX Live 2026 packages (retex's feature `bundled`, about 9.7 MB), unpacked once into `~/.cache/retex/bundle/`; an installed TeX Live of the same year comes first, and other packages are downloaded into `~/.cache/retex/texlive/`, as for a source build.

### Flatpak

- Runtime `org.freedesktop.Platform` 26.08, the Rust of `org.freedesktop.Sdk.Extension.rust-stable`.
- Permissions (the manifest explains each): Wayland with X11 as a fallback, the GPU, the network
  (TeX Live downloads, templates, the control channel on the loopback) and the home folder
  (projects as folders, opened by path, with files written beside them).
- TeX Live inside the sandbox: the host's TeX Live is not visible; retex builds with its bundle of
  TeX Live packages (compiled in, unpacked into
  `~/.var/app/io.gitlab.severin_lemaignan.TeXCraft/cache/retex/bundle/` at the first build) and
  downloads other packages into `cache/retex/texlive/` there. TeXCraft's settings and kept templates are under
  `~/.var/app/io.gitlab.severin_lemaignan.TeXCraft/` too.
- Building it by hand (with org.flatpak.Builder from Flathub):

  ```sh
  flatpak install flathub org.flatpak.Builder
  flatpak run --command=flatpak-cargo-generator --filesystem="$PWD" org.flatpak.Builder Cargo.lock -o packaging/flatpak/cargo-sources.json
  flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean --install flatpak-build packaging/flatpak/io.gitlab.severin_lemaignan.TeXCraft.yml
  flatpak run io.gitlab.severin_lemaignan.TeXCraft
  ```

  Regenerate `cargo-sources.json` whenever `Cargo.lock` changes; it is not committed.

## Manual checks

With the packages from step 8 (or from the release):

- **AppImage**: `chmod +x TeXCraft-*.AppImage && ./TeXCraft-*.AppImage examples/article`. The window
  has its icon and the Inter interface face; the article builds and its PDF shows; edit a line and
  the rebuild is quick (the worker); double-click in the PDF opens the line. Try it on a Wayland
  and an X11 session if you can, and on a machine without a TeX Live and offline (the built-in packages: the article builds).
  `./TeXCraft-*.AppImage --version` and the `texcraft-cli` link: `texcraft-cli compile examples/article`.
- **Flatpak**: `flatpak install --user TeXCraft-*.flatpak` (it fetches the runtime from Flathub),
  then `flatpak run io.gitlab.severin_lemaignan.TeXCraft ~/path/to/texcraft/examples/article`: same checks,
  plus the article built offline from the built-in packages (`cache/retex/bundle/` in `~/.var/app/io.gitlab.severin_lemaignan.TeXCraft/`),
  Open Folder through the portal, and a template from New Project.
- **Browser**: `tar xzf texcraft-web-*.tar.gz && cd web && python3 -m http.server` (any static
  server), open it in a browser: the loading screen shows progress, the editor draws, files can be
  added and dropped, a template makes a project, and Compile builds it in the page (the first build
  of a visit fetches its TeX files). After the
  `pages` job, the same at the Pages address (a subpath: the build uses relative URLs).
- **Release page**: the description is the changelog section, and the four links download.

## Known limits

- Only x86_64 Linux packages are tested; the Windows zip and the macOS app (Apple Silicon) on the
  GitHub releases are experimental and unsigned.
- In the browser, a document that needs a package outside the site's TeX tree does not build.
- The translation catalogs are new: what they lack shows in English.
- The packages are not signed, and the AppImage carries no AppImageUpdate information: it
  updates itself through TeXCraft's own check (above).
- The Flatpak does not use a host TeX Live.
- The Flatpak job needs a runner that runs containers privileged (bubblewrap's user namespaces).
