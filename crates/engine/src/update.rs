//! Updates (docs/ROADMAP.md, A8): which release is the latest, whether it is newer than this copy,
//! and what this copy can do about it, as the way it was installed allows.
//!
//! The latest release comes from GitLab's releases API (the project's releases, where CI publishes
//! them; docs/releasing.md), and from the GitHub mirror's when gitlab.com cannot be reached. The
//! browser build asks its own site for `version.txt`, which the build writes beside the page.
//! Requests carry nothing about the user: a plain GET, with the transport's `User-Agent`.
//!
//! What can be done, by install ([`plan`]):
//!
//! | Install | Action |
//! |---|---|
//! | AppImage | download the release's AppImage beside this one, check its SHA-256 against the release's `SHA256SUMS`, put it in this one's place (this one is kept as `.old` until the new one starts), restart |
//! | Flatpak | none: updates come through Flatpak; the release's bundle is linked |
//! | Windows, macOS | none: the release's zip for this system is linked (the zips are unsigned, and replacing a running unsigned program is not done) |
//! | built from source, unknown | the release page |
//! | browser | reload the page |
//!
//! The checking, comparing and downloading come from craft-update (storytold/craft-libs) where it
//! has the piece: version comparison, GitHub's release format, `SHA256SUMS` parsing, verified
//! downloads, starting a program. Its own self-update flow expects the suite's asset names and
//! GitHub releases only, which TeXCraft's releases do not have; reading GitLab's releases belongs
//! in craft-update too, and moves there with its own change.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The GitLab project the releases are made in.
pub const GITLAB_PROJECT: &str = "severin-lemaignan/texcraft";
/// The GitHub mirror, which has the same releases and the Windows and macOS zips.
pub const GITHUB_REPO: &str = "skadge/texcraft";
/// GitLab's latest release (not an upcoming one).
pub const GITLAB_LATEST: &str = "https://gitlab.com/api/v4/projects/severin-lemaignan%2Ftexcraft/releases/permalink/latest";
/// The mirror's latest release, asked when gitlab.com cannot be reached.
pub const GITHUB_LATEST: &str = "https://api.github.com/repos/skadge/texcraft/releases/latest";
/// The browser build's version, beside its page (`cargo xtask version-worker` writes it).
pub const WEB_VERSION_FILE: &str = "version.txt";
/// How often the automatic check runs.
pub const DAY: i64 = 24 * 60 * 60;
/// The largest release answer read (the notes are a changelog section).
const MAX_ANSWER: u64 = 1 << 20;
/// The longest release notes kept.
const MAX_NOTES: usize = 64 * 1024;

/// The update settings the shell keeps with the preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// Check once a day, shortly after the start.
    pub check: bool,
    /// When a check last got an answer, in Unix seconds.
    pub last_check: Option<i64>,
}

impl Default for Saved {
    fn default() -> Self {
        Self { check: true, last_check: None }
    }
}

/// Whether the automatic check is due at `now` (Unix seconds): on, and no answer in the last
/// day. A last check in the future (a clock set back) counts as long ago.
pub fn due(saved: &Saved, now: i64) -> bool {
    saved.check && saved.last_check.is_none_or(|last| now < last || now - last >= DAY)
}

/// One file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// Lowercase hex SHA-256, when the feed gives it (GitHub's `digest`).
    pub sha256: Option<String>,
}

/// Where a release was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Feed {
    GitLab,
    GitHub,
    /// The browser build's own site.
    Site,
}

/// The latest release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    /// `0.20.0`.
    pub version: String,
    /// `v0.20.0`.
    pub tag: String,
    /// The release notes, as the feed has them (Markdown).
    pub notes: String,
    /// The release's page.
    pub page: String,
    /// RFC 3339, when the feed says.
    pub published: Option<String>,
    pub assets: Vec<Asset>,
    pub feed: Feed,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The version in a tag (`v0.20.0` → `0.20.0`), if it is one.
fn tag_version(tag: &str) -> Option<String> {
    let v = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    let ok = !v.is_empty()
        && v.len() <= 64
        && v.starts_with(|c: char| c.is_ascii_digit())
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b));
    ok.then(|| v.to_owned())
}

/// Whether `url` may be shown and opened: `https` on `host`, with no space, control or shell
/// character (the system's opener may pass it through a shell) and no `..`.
fn link_on(url: &str, host: &str) -> bool {
    url.strip_prefix("https://").and_then(|rest| rest.strip_prefix(host)).is_some_and(|rest| rest.starts_with('/'))
        && !url.contains("..")
        && url.chars().all(|c| c.is_ascii_graphic() && !"\"'`<>|;&$\\^{}".contains(c))
}

/// `text` cut to at most `max` bytes, on a character's boundary.
fn capped(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// Reads GitLab's answer for a release (`GET /projects/:id/releases/permalink/latest`).
pub fn parse_gitlab(body: &[u8]) -> Result<Release, String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("GitLab's answer about the latest release cannot be read: {e}"))?;
    let tag = v["tag_name"].as_str().unwrap_or_default();
    let version = tag_version(tag).ok_or_else(|| format!("GitLab's latest release has no version in its tag ({})", capped(tag, 40)))?;
    let fallback = format!("https://gitlab.com/{GITLAB_PROJECT}/-/releases/{tag}");
    let page = v["_links"]["self"].as_str().filter(|u| link_on(u, "gitlab.com") && u.contains(GITLAB_PROJECT)).map_or(fallback, str::to_owned);
    let assets = v["assets"]["links"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let name = link["name"].as_str()?;
            let url = link["url"].as_str().filter(|u| link_on(u, "gitlab.com"))?;
            Some(Asset { name: name.to_owned(), url: url.to_owned(), sha256: None })
        })
        .collect();
    Ok(Release {
        version,
        tag: tag.to_owned(),
        notes: capped(v["description"].as_str().unwrap_or_default(), MAX_NOTES),
        page,
        published: v["released_at"].as_str().map(str::to_owned),
        assets,
        feed: Feed::GitLab,
    })
}

/// Reads GitHub's answer for a release (`GET /repos/:repo/releases/latest`), with craft-update's
/// reader: only the mirror's own download links are kept, with GitHub's checksums.
#[cfg(not(target_arch = "wasm32"))]
pub fn parse_github(body: &[u8]) -> Result<Release, String> {
    let text = std::str::from_utf8(body).map_err(|_| "GitHub's answer about the latest release is not text".to_owned())?;
    let r = craft_update::parse_github_release(text, GITHUB_REPO)?;
    let version = tag_version(&r.tag).ok_or_else(|| format!("GitHub's latest release has no version in its tag ({})", capped(&r.tag, 40)))?;
    Ok(Release {
        version,
        published: r.published_at.and_then(|s| chrono::DateTime::from_timestamp(s, 0)).map(|d| d.to_rfc3339()),
        assets: r.assets.into_iter().map(|a| Asset { name: a.name, url: a.url, sha256: a.sha256 }).collect(),
        tag: r.tag,
        notes: r.notes,
        page: r.html_url,
        feed: Feed::GitHub,
    })
}

/// Reads the browser build's `version.txt`: the version, on the first line.
pub fn parse_site(body: &[u8]) -> Result<Release, String> {
    let text = String::from_utf8_lossy(body);
    let line = text.lines().next().unwrap_or_default().trim();
    let version = tag_version(line).ok_or_else(|| "The site's version file has no version in it".to_owned())?;
    Ok(Release {
        tag: format!("v{version}"),
        version,
        notes: String::new(),
        page: String::new(),
        published: None,
        assets: Vec::new(),
        feed: Feed::Site,
    })
}

/// Whether `latest` is newer than `running` (versions or tags). A build from source between
/// releases (`0.20.0-dev`) is offered only a strictly higher release, as craft-update decides.
pub fn is_newer(latest: &str, running: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use craft_update::version::{offers_update, parse};
        parse(latest).zip(parse(running)).is_some_and(|(l, r)| offers_update(&l, &r))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches(['v', 'V'])).ok();
        parse(latest).zip(parse(running)).is_some_and(|(l, r)| l > r)
    }
}

/// How this copy was installed, which decides what an update can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// An AppImage: the file (`APPIMAGE`).
    AppImage(PathBuf),
    /// A Flatpak (`FLATPAK_ID`, `/.flatpak-info`).
    Flatpak,
    /// The Windows zip, unpacked.
    Windows,
    /// The macOS app.
    MacOs,
    /// Built from source, or a copy not known.
    Source,
    /// The browser build.
    Browser,
}

impl Install {
    /// This copy's install, as its program and environment say.
    pub fn detect() -> Self {
        #[cfg(target_arch = "wasm32")]
        return Self::Browser;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let exe = std::env::current_exe().ok();
            // A Cargo build tree: its target folder carries Cargo's cache marker.
            let built_here = exe.as_deref().is_some_and(|e| e.ancestors().skip(1).any(|d| d.join("CACHEDIR.TAG").is_file()));
            if cfg!(target_os = "linux") && craft_update::process::in_sandbox() {
                return Self::Flatpak;
            }
            if cfg!(target_os = "linux")
                && let Some(file) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|f| f.is_file())
            {
                return Self::AppImage(file);
            }
            if built_here {
                return Self::Source;
            }
            if cfg!(windows) {
                return Self::Windows;
            }
            let in_bundle = exe.as_deref().is_some_and(|e| e.ancestors().any(|d| d.extension().is_some_and(|x| x == "app")));
            if cfg!(target_os = "macos") && in_bundle {
                return Self::MacOs;
            }
            Self::Source
        }
    }

    /// Its name for agents.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AppImage(_) => "appimage",
            Self::Flatpak => "flatpak",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Source => "source",
            Self::Browser => "browser",
        }
    }
}

/// What an update of this copy does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Download `asset` beside the AppImage, check it against its checksum (its own, or the
    /// line of `sums`), put it in the AppImage's place.
    ReplaceAppImage { asset: Asset, sums: Option<Asset> },
    /// Nothing here: Flatpak updates it. The release's bundle, when it has one.
    Flatpak { bundle: Option<Asset> },
    /// Nothing here: the user downloads `asset` (the zip for this system) and puts it in place.
    Download { asset: Asset },
    /// Only the release page: no file of the release fits this copy.
    ReleasePage,
    /// Reload the page.
    Reload,
}

impl Action {
    /// Its name for agents.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ReplaceAppImage { .. } => "replace_appimage",
            Self::Flatpak { .. } => "flatpak",
            Self::Download { .. } => "download",
            Self::ReleasePage => "release_page",
            Self::Reload => "reload",
        }
    }
}

/// The mirror's download link of the file `name` of release `tag`.
fn github_download(tag: &str, name: &str) -> Asset {
    Asset { name: name.to_owned(), url: format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}/{name}"), sha256: None }
}

/// What updating `install` to `release` does, on a machine of architecture `arch`
/// (`std::env::consts::ARCH`: `x86_64`, `aarch64`). The release files are named as CI names them
/// (docs/releasing.md): `TeXCraft-<version>-<arch>.AppImage` and `.flatpak` on GitLab and GitHub,
/// `TeXCraft-<version>-x86_64-windows.zip` and `-aarch64-macos.zip` on GitHub only.
pub fn plan(install: &Install, release: &Release, arch: &str) -> Action {
    let v = &release.version;
    match install {
        Install::Browser => Action::Reload,
        Install::AppImage(_) => {
            let name = format!("TeXCraft-{v}-{arch}.AppImage");
            let sums = release.asset("SHA256SUMS").cloned();
            match release.asset(&name) {
                Some(asset) if asset.sha256.is_some() || sums.is_some() => Action::ReplaceAppImage { asset: asset.clone(), sums },
                _ => Action::ReleasePage,
            }
        }
        Install::Flatpak => Action::Flatpak { bundle: release.asset(&format!("TeXCraft-{v}-{arch}.flatpak")).cloned() },
        Install::Windows if arch == "x86_64" => {
            Action::Download { asset: github_download(&release.tag, &format!("TeXCraft-{v}-x86_64-windows.zip")) }
        }
        Install::MacOs if arch == "aarch64" => Action::Download { asset: github_download(&release.tag, &format!("TeXCraft-{v}-aarch64-macos.zip")) },
        Install::Windows | Install::MacOs | Install::Source => Action::ReleasePage,
    }
}

/// The release page for `install`: the mirror's for Windows and macOS (their files are there),
/// else the release's own.
pub fn page_for(install: &Install, release: &Release) -> String {
    match install {
        Install::Windows | Install::MacOs => format!("https://github.com/{GITHUB_REPO}/releases/tag/{}", release.tag),
        _ => release.page.clone(),
    }
}

/// `release` and its action, for agents.
pub fn describe(release: &Release, action: &Action) -> Value {
    let link = |a: &Asset| json!({"name": a.name, "url": a.url});
    let file = match action {
        Action::ReplaceAppImage { asset, .. } | Action::Download { asset } | Action::Flatpak { bundle: Some(asset) } => link(asset),
        Action::Flatpak { bundle: None } | Action::ReleasePage | Action::Reload => Value::Null,
    };
    json!({
        "version": release.version,
        "tag": release.tag,
        "published": release.published,
        "page": release.page,
        "feed": release.feed,
        "notes": release.notes,
        "action": action.name(),
        "file": file,
    })
}

/// The release notes as plain text for a dialog: headings, bullets, emphasis, links and code
/// marks of the Markdown taken out, the words kept.
pub fn notes_text(markdown: &str) -> String {
    let mut out = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let line = if let Some(heading) = trimmed.strip_prefix('#') {
            heading.trim_start_matches('#').trim().to_owned()
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            format!("{}• {item}", " ".repeat(indent))
        } else {
            line.to_owned()
        };
        out.push(plain_inline(&line));
    }
    let mut text = out.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_owned()
}

/// `[words](link)` → `words`; `**`, `__` and backticks removed.
fn plain_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = after.find("](") else { break };
        let target = after.get(close + 2..).unwrap_or_default();
        let Some(end) = target.find(')') else { break };
        out.push_str(rest.get(..open).unwrap_or_default());
        out.push_str(after.get(..close).unwrap_or_default());
        rest = target.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "").replace('`', "")
}

/// A request for the latest release, with nothing about the user in it.
fn get(url: &str) -> texcraft_forge::Request {
    texcraft_forge::Request {
        method: "GET",
        url: url.to_owned(),
        headers: vec![("Accept".to_owned(), "application/json".to_owned())],
        body: None,
        max_body: MAX_ANSWER,
    }
}

/// What a check found: the latest release, or none published.
pub type Found = Option<Release>;

/// Asks for the latest release through `transport`: GitLab's, else the mirror's when gitlab.com
/// cannot be reached or fails (desktop); the site's `version.txt` (browser, `site` true). `now`
/// keeps a cache in between from answering for the site's file.
pub async fn latest(transport: &dyn texcraft_forge::Transport, site: bool, now: i64) -> Result<Found, String> {
    if site {
        let response =
            transport.send(get(&format!("{WEB_VERSION_FILE}?t={now}"))).await.map_err(|e| format!("The site could not be reached: {e}"))?;
        return match response.status {
            200 => parse_site(&response.body).map(Some),
            404 => Ok(None),
            status => Err(format!("The site answered HTTP {status} for its version")),
        };
    }
    let gitlab = match transport.send(get(GITLAB_LATEST)).await {
        Ok(r) if r.status == 200 && !r.too_large => return parse_gitlab(&r.body).map(Some),
        Ok(r) if r.status == 404 => return Ok(None),
        Ok(r) => format!("gitlab.com answered HTTP {}", r.status),
        Err(e) => format!("gitlab.com could not be reached: {e}"),
    };
    log::info!("updates: {gitlab}; asking the GitHub mirror");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let github = match transport.send(get(GITHUB_LATEST)).await {
            Ok(r) if r.status == 200 && !r.too_large => return parse_github(&r.body).map(Some),
            Ok(r) if r.status == 404 => return Ok(None),
            Ok(r) => format!("github.com answered HTTP {}", r.status),
            Err(e) => format!("github.com could not be reached: {e}"),
        };
        Err(format!("{gitlab}; {github}"))
    }
    #[cfg(target_arch = "wasm32")]
    Err(gitlab)
}

/// Downloading and putting a new AppImage in place (the desktop).
#[cfg(not(target_arch = "wasm32"))]
pub mod appimage {
    use std::path::{Path, PathBuf};

    use super::Asset;

    /// Fetches the release's files: craft-update's HTTPS client, or a fake in tests.
    pub trait Fetch: Send + Sync {
        /// A small text file (the checksums).
        fn text(&self, url: &str) -> Result<String, String>;
        /// The file at `url` into `dest`, checked against `sha256` before it is put there;
        /// `progress` hears (bytes so far, bytes in all or 0).
        fn download(&self, url: &str, dest: &Path, sha256: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String>;
    }

    /// craft-update's client: resumes a dropped transfer, and checks the SHA-256 before the file
    /// is put at its place.
    pub struct Http(craft_update::Http);

    impl Http {
        pub fn new() -> Result<Self, String> {
            craft_update::Http::new("TeXCraft").map(Self)
        }
    }

    impl Fetch for Http {
        fn text(&self, url: &str) -> Result<String, String> {
            self.0.get_text(url)?.ok_or_else(|| "The release's checksums are missing".to_owned())
        }

        fn download(&self, url: &str, dest: &Path, sha256: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
            let stop = craft_update::Stop::default();
            self.0.download(url, dest, 0, sha256, &stop, &mut |t| match t {
                craft_update::http::Transfer::Bytes { done, total } | craft_update::http::Transfer::Reconnecting { done, total } => {
                    progress(done, total)
                }
            })
        }
    }

    /// `file` with `suffix` added to its name (`TeXCraft.AppImage.old`).
    pub fn beside(file: &Path, suffix: &str) -> PathBuf {
        let mut name = file.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
        name.push(suffix);
        file.with_file_name(name)
    }

    /// Why nothing was downloaded: the release has no checksum for the file.
    pub const NO_CHECKSUM: &str = "The release publishes no checksum for its AppImage, so nothing was downloaded.";
    /// Why nothing was replaced: the file downloaded is not the one the release lists.
    pub const MISMATCH: &str = "The download does not match the release's checksum, so nothing was replaced.";

    /// The SHA-256 of `asset`: its own, or its line in the checksums file `sums`.
    fn checksum(fetch: &dyn Fetch, asset: &Asset, sums: Option<&Asset>) -> Result<String, String> {
        if let Some(sha) = &asset.sha256 {
            return Ok(sha.clone());
        }
        let sums = sums.ok_or(NO_CHECKSUM)?;
        let text = fetch.text(&sums.url)?;
        craft_update::release::parse_sha256sums(&text)
            .into_iter()
            .find(|(_, name)| *name == asset.name)
            .map(|(sha, _)| sha)
            .ok_or_else(|| NO_CHECKSUM.to_owned())
    }

    /// Downloads `asset` beside the AppImage `current`, checks it, and puts it in `current`'s
    /// place ([`replace`]). On any failure `current` is as it was.
    pub fn update(fetch: &dyn Fetch, asset: &Asset, sums: Option<&Asset>, current: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let sha = checksum(fetch, asset, sums)?;
        let new = beside(current, ".new");
        if let Err(e) = fetch.download(&asset.url, &new, &sha, progress) {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }
        replace(&new, current, &sha)
    }

    /// Puts the AppImage `new` in `current`'s place once its SHA-256 is `sha256`: the old one stays
    /// as `current.old` (a link to it, or a copy where links are refused) until the new one
    /// starts ([`forget_old`]), and the new one takes the name in one rename, so `current` is
    /// always a whole AppImage. The running copy keeps working: it is mounted from the old file.
    /// A file that does not match is removed and nothing is replaced.
    pub fn replace(new: &Path, current: &Path, sha256: &str) -> Result<(), String> {
        let matches = craft_update::http::sha256_file(new).is_ok_and(|got| got.eq_ignore_ascii_case(sha256));
        if !matches {
            let _ = std::fs::remove_file(new);
            return Err(MISMATCH.to_owned());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("The new AppImage cannot be made executable: {e}"))?;
        }
        let old = beside(current, ".old");
        let _ = std::fs::remove_file(&old);
        if std::fs::hard_link(current, &old).is_err() {
            std::fs::copy(current, &old).map_err(|e| format!("The AppImage cannot be kept as {}: {e}", old.display()))?;
        }
        std::fs::rename(new, current).map_err(|e| {
            let _ = std::fs::remove_file(new);
            let _ = std::fs::remove_file(&old);
            format!("The AppImage cannot be replaced: {e}")
        })
    }

    /// Removes the copy an update kept (`current.old`): the new one has started.
    pub fn forget_old(current: &Path) {
        let old = beside(current, ".old");
        if old.is_file() {
            match std::fs::remove_file(&old) {
                Ok(()) => log::info!("updates: removed {}", old.display()),
                Err(e) => log::warn!("updates: {} could not be removed: {e}", old.display()),
            }
        }
    }

    /// Starts the AppImage `file`, without the environment this one's runtime set.
    pub fn start(file: &Path) -> Result<(), String> {
        craft_update::process::spawn_detached(craft_update::process::command(file)).map_err(|e| format!("The new version did not start: {e}"))
    }
}
