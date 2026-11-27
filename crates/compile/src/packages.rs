//! Missing TeX Live packages in the browser (docs/ROADMAP.md, W8): what the engine's worker
//! ([`crate::web_engine`]) answers when retex's module asks for a file of its package repository.
//!
//! retex's module (built with its `download` feature, `cargo xtask retex-wasm`) looks up a file
//! its tree lacks in TeX Live's package database, and fetches the package that holds it from a
//! repository that is a directory, named by `RETEX_REPOSITORY`: the module has no network. The
//! worker serves that directory ([`REPOSITORY_DIR`]) from a TeX Live mirror whose server sends
//! CORS headers ([`DEFAULT_SERVER`]): `/tlnet/<snapshot>/<file>` is
//! `<server>/<year>/<snapshot>/<file>`. The module checks the database's signature and each
//! archive's checksum, so the server is not trusted; it unpacks the run-time files into
//! `$RETEX_CACHE/texlive/<year>/`, which the worker keeps between visits.
//!
//! The snapshot: the server keeps dated snapshots of the year's tlnet, which do not change, and
//! names the newest in `<server>/<year>/current`. retex keeps the database it fetched, and the
//! directory it came from (`$RETEX_CACHE/texlive/<year>/tlpdb/repository`), and asks for its
//! archives there: so a database and the archives it describes always come from one snapshot.
//! The worker only asks `current` when retex keeps no database ([`Downloads::repository`]).
//!
//! Being a good client of a small server: one request at a time (the worker fetches between the
//! module's runs, one file after the other), only the files retex names (a database's three files
//! and `archive/<package>.tar.xz`), each at most once a visit; a refusal (429, with its
//! `Retry-After`), an unavailable server (503) or no answer holds every request back for a while
//! ([`Downloads::reply`]), and the build goes on without the package, saying why
//! ([`Downloads::take_notes`]).

use std::collections::HashSet;

/// The package server of the browser build: a mirror of TeX Live's tlnet, by year and snapshot,
/// whose server allows TeXCraft's site to read it. A fork or a self-hosted site sets its own with
/// `TEXCRAFT_TEXLIVE_REPOSITORY` at build time; a user in Settings › Building.
pub const DEFAULT_SERVER: &str = match option_env!("TEXCRAFT_TEXLIVE_REPOSITORY") {
    Some(s) => s,
    None => "https://texlive.skadge.org/tlnet",
};

/// The directory the module is given as its repository (`RETEX_REPOSITORY`), with the snapshot
/// after it.
pub const REPOSITORY_DIR: &str = "/tlnet";

/// The database's files, in the order retex reads them.
const DATABASE: [&str; 3] = ["tlpkg/texlive.tlpdb.sha512", "tlpkg/texlive.tlpdb.sha512.asc", "tlpkg/texlive.tlpdb.xz"];

/// How long a refusal without `Retry-After` (429) holds requests back, in seconds.
const BUSY_SECS: f64 = 60.0;
/// How long an unavailable server (503) holds requests back, in seconds.
const CLOSED_SECS: f64 = 600.0;
/// How long a server that did not answer, or answered with another error, holds requests back.
const FAILED_SECS: f64 = 120.0;
/// The longest a `Retry-After` is obeyed for; a longer one waits this long, then asks again.
const MAX_WAIT_SECS: f64 = 3600.0;

/// Where the browser build downloads missing packages from (`build.package_server`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum PackageServer {
    /// [`DEFAULT_SERVER`].
    #[default]
    Default,
    /// No downloads: a package the tree lacks stays missing.
    Off,
    /// Another mirror with the same layout: `https://…/tlnet` (without the year).
    Custom(String),
}

impl PackageServer {
    /// `default`, `off`, or a server's address (`https://`, or `http://` on this computer).
    pub fn parse(s: &str) -> Result<PackageServer, String> {
        let s = s.trim();
        match s {
            "" | "default" => Ok(PackageServer::Default),
            "off" => Ok(PackageServer::Off),
            url => {
                let url = url.trim_end_matches('/');
                let local = ["http://127.0.0.1", "http://localhost"]
                    .iter()
                    .any(|h| url.strip_prefix(h).is_some_and(|r| r.is_empty() || r.starts_with([':', '/'])));
                let host = url.strip_prefix("https://").unwrap_or_default();
                if !(local || !host.is_empty() && !host.starts_with('/')) || url.contains(char::is_whitespace) {
                    return Err(format!("`{url}` is not a package server: give its https:// address, `default` or `off`."));
                }
                Ok(if url == DEFAULT_SERVER { PackageServer::Default } else { PackageServer::Custom(url.to_owned()) })
            }
        }
    }

    /// The server's address, `None` when downloads are off.
    pub fn base(&self) -> Option<&str> {
        match self {
            PackageServer::Default => Some(DEFAULT_SERVER),
            PackageServer::Off => None,
            PackageServer::Custom(url) => Some(url),
        }
    }
}

impl TryFrom<String> for PackageServer {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        PackageServer::parse(&s)
    }
}

impl From<PackageServer> for String {
    fn from(p: PackageServer) -> String {
        match p {
            PackageServer::Default => "default".to_owned(),
            PackageServer::Off => "off".to_owned(),
            PackageServer::Custom(url) => url,
        }
    }
}

/// Whether `s` can name a snapshot: what `current` holds (`20261126`), a single path component.
fn is_snapshot(s: &str) -> bool {
    (1..=32).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The snapshot `current` names: its first line.
pub fn parse_snapshot(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let line = text.lines().next()?.trim();
    is_snapshot(line).then(|| line.to_owned())
}

/// A file of the repository retex may ask for, at `path` in the module's files
/// (`/tlnet/20261126/archive/pgf.tar.xz`): its snapshot and its path in the snapshot. Anything
/// else under the directory is never fetched.
pub fn repository_file(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix(REPOSITORY_DIR)?.strip_prefix('/')?;
    let (snapshot, file) = rest.split_once('/')?;
    if !is_snapshot(snapshot) {
        return None;
    }
    let archive = file.strip_prefix("archive/").and_then(|a| a.strip_suffix(".tar.xz"));
    let package_name = |p: &str| !p.is_empty() && !p.starts_with('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    (DATABASE.contains(&file) || archive.is_some_and(package_name)).then_some((snapshot, file))
}

/// The package of a repository file (`archive/pgf.tar.xz`: `pgf`); `None` for the database.
pub fn package_of(file: &str) -> Option<&str> {
    file.strip_prefix("archive/")?.strip_suffix(".tar.xz")
}

/// The repository directory of `snapshot`, for `RETEX_REPOSITORY`.
pub fn repository_dir(snapshot: &str) -> String {
    format!("{REPOSITORY_DIR}/{snapshot}")
}

/// What the module keeps as its database's repository (`tlpdb/repository`), if it is one of the
/// worker's directories: its snapshot.
pub fn kept_snapshot(repository: &str) -> Option<&str> {
    let snapshot = repository.trim().strip_prefix(REPOSITORY_DIR)?.strip_prefix('/')?;
    is_snapshot(snapshot).then_some(snapshot)
}

/// Where retex keeps the database's repository, for TeX Live `year`, under its cache `/cache`.
pub fn kept_repository_path(year: u16) -> String {
    format!("/cache/texlive/{year}/tlpdb/repository")
}

/// The answer to a request, as the worker got it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// 200, with the body.
    Ok(Vec<u8>),
    /// Another status, with `Retry-After` when the server sent one and the browser shows it.
    Status { code: u16, retry_after: Option<String> },
    /// No answer: the network, a timeout, or a server that does not allow this site (CORS).
    Failed(String),
}

/// What the worker answers the module for a repository file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Bytes(Vec<u8>),
    /// The repository has no such file (`ENOENT`).
    Absent,
    /// The server cannot give it now (`EIO`): the build goes on without the package.
    Unavailable,
}

/// What to do for a repository file: fetch it from this URL, or answer now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Fetch(String),
    Answer(Answer),
}

/// Why requests are held back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Refusal {
    /// 429: too many requests.
    Busy,
    /// 503: the server is not available (its monthly budget spent, or down).
    Closed,
    /// Another status.
    Status(u16),
    /// No answer.
    Unreachable(String),
}

/// The package downloads of a visit: the server, what it refused or lacked, and the notes for the
/// build's log.
#[derive(Debug, Default)]
pub struct Downloads {
    server: PackageServer,
    /// The TeX Live year of the tree (`seed/version.txt`); downloads are off without one.
    year: Option<u16>,
    /// The snapshot `current` named in this visit.
    current: Option<String>,
    /// The URLs the server did not have, this visit.
    absent: HashSet<String>,
    /// Until when (milliseconds, `Date.now()`) requests are held back, and why.
    held: Option<(f64, Refusal)>,
    /// What the build's log says: the notes of refusals, and the packages downloaded.
    notes: Vec<Note>,
    /// A snapshot an archive was missing from: the database retex keeps may be of a snapshot the
    /// server no longer has.
    missing_from: Option<String>,
}

/// A line of the build's log about downloads.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// A package (or, `None`, the package database) was downloaded: its size in bytes.
    Downloaded { package: Option<String>, bytes: u64 },
    /// A package (or the database) was not downloaded: why, as the log says it.
    Refused { package: Option<String>, why: String },
}

impl Note {
    /// The note as the build's log shows it.
    pub fn text(&self) -> String {
        match self {
            Note::Downloaded { package: Some(p), bytes } => format!("Downloaded the LaTeX package {p} ({}).", size_text(*bytes)),
            Note::Downloaded { package: None, bytes } => format!("Downloaded the list of TeX Live packages ({}).", size_text(*bytes)),
            Note::Refused { package: Some(p), why } => format!("The LaTeX package {p} was not downloaded: {why}"),
            Note::Refused { package: None, why } => format!("The list of TeX Live packages was not downloaded: {why}"),
        }
    }

    /// A refusal as a warning record of retex's kind (`--message-format=json`), so that the
    /// build's messages list it with retex's own; `None` for a download.
    pub fn record(&self) -> Option<String> {
        matches!(self, Note::Refused { .. })
            .then(|| serde_json::json!({"type": "diagnostic", "severity": "warning", "message": self.text(), "origin": "TeXCraft"}).to_string())
    }
}

/// A `Retry-After` in seconds (its date form is not read: the default wait applies).
fn retry_secs(value: &str) -> Option<f64> {
    value.trim().parse::<u32>().ok().map(f64::from)
}

/// `840 kB`, `1.2 MB`.
pub fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1e6) } else { format!("{} kB", bytes.div_ceil(1000)) }
}

impl Refusal {
    /// Why, as a sentence of the log.
    fn why(&self) -> String {
        match self {
            Refusal::Busy => "The TeX package server is busy; the build continues without the missing package, and asks again later.".to_owned(),
            Refusal::Closed => "The TeX package server is not available right now; the build continues without the missing package.".to_owned(),
            Refusal::Status(code) => {
                format!("The TeX package server answered with an error (HTTP {code}); the build continues without the missing package.")
            }
            Refusal::Unreachable(why) => {
                format!("The TeX package server could not be reached ({why}); the build continues without the missing package.")
            }
        }
    }
}

impl Downloads {
    pub fn new(server: PackageServer, year: Option<u16>) -> Downloads {
        Downloads { server, year, ..Downloads::default() }
    }

    /// Uses `server` from now on; a change forgets what the last one refused or lacked.
    pub fn set_server(&mut self, server: PackageServer) {
        if server != self.server {
            *self = Downloads::new(server, self.year);
        }
    }

    pub fn server(&self) -> &PackageServer {
        &self.server
    }

    /// The server's address of the year's files, when downloads are on.
    fn year_base(&self) -> Option<String> {
        Some(format!("{}/{}", self.server.base()?, self.year?))
    }

    fn held(&self, now: f64) -> Option<&Refusal> {
        self.held.as_ref().filter(|(until, _)| now < *until).map(|(_, why)| why)
    }

    /// Before a run: `RETEX_REPOSITORY`, when downloads are on. `kept` is what retex keeps as its
    /// database's repository ([`kept_repository_path`]): its snapshot is the one to ask, as the
    /// database describes its archives. Without one, the snapshot `current` named; `None` while
    /// that is not known ([`Self::snapshot_url`]).
    pub fn repository(&self, kept: Option<&str>) -> Option<String> {
        self.year_base()?;
        let snapshot = kept.and_then(kept_snapshot).map(str::to_owned).or_else(|| self.current.clone())?;
        Some(repository_dir(&snapshot))
    }

    /// The URL of `current`, when the worker should ask it before a run: downloads on, retex
    /// keeping no database, the snapshot not known yet, and no refusal holding requests back.
    pub fn snapshot_url(&self, kept: Option<&str>, now: f64) -> Option<String> {
        let base = self.year_base()?;
        let wanted = kept.and_then(kept_snapshot).is_none() && self.current.is_none() && self.held(now).is_none();
        wanted.then(|| format!("{base}/current"))
    }

    /// `current`'s answer.
    pub fn snapshot_reply(&mut self, reply: Reply, now: f64) {
        match reply {
            Reply::Ok(body) => match parse_snapshot(&body) {
                Some(s) => self.current = Some(s),
                None => self.refuse(None, Refusal::Status(200), None, now),
            },
            // The server has no tlnet of this year.
            Reply::Status { code: 404, .. } => {
                let year = self.year.unwrap_or_default();
                self.notes.push(Note::Refused { package: None, why: format!("The TeX package server has no TeX Live {year}.") });
                self.held = Some((now + CLOSED_SECS * 1000.0, Refusal::Status(404)));
            }
            other => self.failed(None, other, now),
        }
    }

    /// For the file the module opens at `path` (under [`REPOSITORY_DIR`]): what to fetch, or the
    /// answer.
    pub fn plan(&self, path: &str, now: f64) -> Plan {
        let (Some(base), Some((snapshot, file))) = (self.year_base(), repository_file(path)) else { return Plan::Answer(Answer::Absent) };
        if self.held(now).is_some() {
            return Plan::Answer(Answer::Unavailable);
        }
        let url = format!("{base}/{snapshot}/{file}");
        if self.absent.contains(&url) { Plan::Answer(Answer::Absent) } else { Plan::Fetch(url) }
    }

    /// The files to fetch with the one at `path`: the database's other files with one of them
    /// (retex reads the three in one go, so the module then stops once, not three times).
    pub fn with(path: &str) -> Vec<String> {
        match repository_file(path) {
            Some((snapshot, file)) if DATABASE.contains(&file) => DATABASE.iter().map(|f| format!("{}/{f}", repository_dir(snapshot))).collect(),
            _ => vec![path.to_owned()],
        }
    }

    /// The answer for the file at `path`, fetched from `url`.
    pub fn reply(&mut self, path: &str, url: &str, reply: Reply, now: f64) -> Answer {
        let file = repository_file(path);
        let package = file.and_then(|(_, f)| package_of(f)).map(str::to_owned);
        match reply {
            Reply::Ok(body) => {
                if file.is_some_and(|(_, f)| f.ends_with(".xz")) {
                    self.notes.push(Note::Downloaded { package, bytes: body.len() as u64 });
                }
                Answer::Bytes(body)
            }
            Reply::Status { code: 404 | 410, .. } => {
                self.absent.insert(url.to_owned());
                if package.is_some() {
                    self.missing_from = file.map(|(s, _)| s.to_owned());
                }
                Answer::Absent
            }
            other => {
                self.failed(package, other, now);
                Answer::Unavailable
            }
        }
    }

    fn failed(&mut self, package: Option<String>, reply: Reply, now: f64) {
        let (refusal, retry_after) = match reply {
            Reply::Status { code: 429, retry_after } => (Refusal::Busy, retry_after),
            Reply::Status { code: 503, retry_after } => (Refusal::Closed, retry_after),
            Reply::Status { code, .. } => (Refusal::Status(code), None),
            Reply::Failed(why) => (Refusal::Unreachable(why), None),
            Reply::Ok(_) => (Refusal::Status(200), None),
        };
        let secs = retry_after.as_deref().and_then(retry_secs).map(|s| s.clamp(1.0, MAX_WAIT_SECS));
        self.refuse(package, refusal, secs, now);
    }

    /// Holds requests back for `secs` (the refusal's own wait without it).
    fn refuse(&mut self, package: Option<String>, refusal: Refusal, secs: Option<f64>, now: f64) {
        let secs = secs.unwrap_or(match refusal {
            Refusal::Busy => BUSY_SECS,
            Refusal::Closed => CLOSED_SECS,
            _ => FAILED_SECS,
        });
        self.notes.push(Note::Refused { package, why: refusal.why() });
        self.held = Some((now + secs * 1000.0, refusal));
    }

    /// The notes since the last call, for the build's log.
    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// A snapshot an archive was missing from since the last call: when retex's database is of
    /// it and `current` names another, the database is stale.
    pub fn take_missing_from(&mut self) -> Option<String> {
        self.missing_from.take()
    }

    /// After an archive was missing from `stale`, the snapshot of retex's database: the URL of
    /// `current`, to ask whether the server has a newer one (then the database is dropped, and
    /// fetched again from that one). `None` while requests are held back.
    pub fn recheck_url(&mut self, now: f64) -> Option<String> {
        self.current = None;
        let base = self.year_base()?;
        self.held(now).is_none().then(|| format!("{base}/current"))
    }

    /// The snapshot `current` named in this visit.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARCHIVE: &str = "/tlnet/20261126/archive/epigraph.tar.xz";

    /// Downloads from a server at `base` for TeX Live 2026, its snapshot known.
    fn downloads(base: &str) -> Downloads {
        let mut d = Downloads::new(PackageServer::parse(base).unwrap_or_default(), Some(2026));
        d.snapshot_reply(Reply::Ok(b"20261126\n".to_vec()), 0.0);
        d
    }

    fn status(code: u16, retry_after: Option<&str>) -> Reply {
        Reply::Status { code, retry_after: retry_after.map(str::to_owned) }
    }

    #[test]
    fn a_repository_file_is_fetched_from_its_snapshot_of_the_year() {
        let d = downloads("https://mirror.example/tlnet/");
        assert_eq!(
            d.plan("/tlnet/20261126/tlpkg/texlive.tlpdb.xz", 0.0),
            Plan::Fetch("https://mirror.example/tlnet/2026/20261126/tlpkg/texlive.tlpdb.xz".into())
        );
        assert_eq!(d.plan(ARCHIVE, 0.0), Plan::Fetch("https://mirror.example/tlnet/2026/20261126/archive/epigraph.tar.xz".into()));
        // Nothing else under the directory is ever asked of the server.
        for path in [
            "/tlnet/20261126/tlpkg/other",
            "/tlnet/20261126/archive/../../etc.tar.xz",
            "/tlnet/a/b/archive/x.tar.xz",
            "/tlnet/archive/x.tar.xz",
            "/texlive/x",
        ] {
            assert_eq!(d.plan(path, 0.0), Plan::Answer(Answer::Absent), "{path}");
        }
        assert_eq!(Downloads::with("/tlnet/20261126/tlpkg/texlive.tlpdb.sha512").len(), 3, "the database's files come together");
    }

    #[test]
    fn the_snapshot_is_asked_once_and_only_without_a_kept_database() {
        let mut d = Downloads::new(PackageServer::Default, Some(2026));
        assert_eq!(d.snapshot_url(None, 0.0), Some(format!("{DEFAULT_SERVER}/2026/current")));
        assert_eq!(d.repository(None), None, "no repository before the snapshot is known");
        // retex keeps a database: its snapshot is the one asked, without `current`.
        assert_eq!(d.snapshot_url(Some("/tlnet/20260401\n"), 0.0), None);
        assert_eq!(d.repository(Some("/tlnet/20260401\n")).as_deref(), Some("/tlnet/20260401"));
        d.snapshot_reply(Reply::Ok(b"20261126\n".to_vec()), 0.0);
        assert_eq!(d.snapshot_url(None, 0.0), None);
        assert_eq!(d.repository(None).as_deref(), Some("/tlnet/20261126"));
        assert_eq!(d.repository(Some("/tlnet/20260401")).as_deref(), Some("/tlnet/20260401"), "the kept database's snapshot wins");
    }

    #[test]
    fn without_a_year_or_with_downloads_off_nothing_is_fetched() {
        assert_eq!(Downloads::new(PackageServer::Default, None).snapshot_url(None, 0.0), None);
        let off = Downloads::new(PackageServer::Off, Some(2026));
        assert_eq!((off.snapshot_url(None, 0.0), off.repository(Some("/tlnet/20261126"))), (None, None));
        assert_eq!(off.plan(ARCHIVE, 0.0), Plan::Answer(Answer::Absent));
    }

    #[test]
    fn a_refusal_holds_every_request_back_for_its_retry_after() {
        let mut d = downloads("default");
        let Plan::Fetch(url) = d.plan(ARCHIVE, 0.0) else { panic!("fetched") };
        assert_eq!(d.reply(ARCHIVE, &url, status(429, Some("30")), 0.0), Answer::Unavailable);
        let other = "/tlnet/20261126/archive/kantlipsum.tar.xz";
        assert_eq!(d.plan(other, 29_000.0), Plan::Answer(Answer::Unavailable), "held: no request at all");
        assert!(matches!(d.plan(other, 31_000.0), Plan::Fetch(_)), "asked again once the wait is over");
        let notes = d.take_notes();
        assert!(notes[0].text().contains("epigraph") && notes[0].record().is_some(), "the log names the package: {notes:?}");
    }

    #[test]
    fn an_unavailable_server_is_said_and_left_alone_for_longer_than_a_busy_one() {
        let mut busy = downloads("default");
        let mut closed = downloads("default");
        busy.reply(ARCHIVE, "u", status(429, None), 0.0);
        closed.reply(ARCHIVE, "u", status(503, None), 0.0);
        let later = BUSY_SECS * 1000.0 + 1.0;
        assert!(matches!(busy.plan(ARCHIVE, later), Plan::Fetch(_)));
        assert_eq!(closed.plan(ARCHIVE, later), Plan::Answer(Answer::Unavailable));
        assert!(closed.take_notes()[0].text().contains("not available right now"));
        // A Retry-After far away is not obeyed for more than an hour.
        let mut long = downloads("default");
        long.reply(ARCHIVE, "u", status(503, Some("999999")), 0.0);
        assert!(matches!(long.plan(ARCHIVE, MAX_WAIT_SECS * 1000.0 + 1.0), Plan::Fetch(_)));
    }

    #[test]
    fn a_missing_archive_is_not_asked_again_and_is_not_a_refusal() {
        let mut d = downloads("default");
        let Plan::Fetch(url) = d.plan(ARCHIVE, 0.0) else { panic!("fetched") };
        assert_eq!(d.reply(ARCHIVE, &url, status(404, None), 0.0), Answer::Absent);
        assert_eq!(d.plan(ARCHIVE, 0.0), Plan::Answer(Answer::Absent));
        assert!(matches!(d.plan("/tlnet/20261126/archive/other.tar.xz", 0.0), Plan::Fetch(_)), "other files are still asked");
        assert!(d.take_notes().is_empty());
        assert_eq!(d.take_missing_from().as_deref(), Some("20261126"), "the kept database may be stale");
    }

    #[test]
    fn no_answer_holds_requests_back_and_a_download_is_listed() {
        let mut d = downloads("default");
        assert_eq!(d.reply(ARCHIVE, "u", Reply::Failed("timeout".into()), 0.0), Answer::Unavailable);
        assert!(d.take_notes()[0].text().contains("could not be reached (timeout)"));
        let mut d = downloads("default");
        assert_eq!(d.reply(ARCHIVE, "u", Reply::Ok(vec![0; 1500]), 0.0), Answer::Bytes(vec![0; 1500]));
        assert_eq!(d.take_notes(), [Note::Downloaded { package: Some("epigraph".into()), bytes: 1500 }]);
    }

    #[test]
    fn a_server_without_the_year_says_so() {
        let mut d = Downloads::new(PackageServer::Default, Some(2031));
        d.snapshot_reply(status(404, None), 0.0);
        assert!(d.take_notes()[0].text().contains("no TeX Live 2031"));
        assert_eq!(d.snapshot_url(None, 1000.0), None, "not asked again at once");
    }

    #[test]
    fn package_servers_are_https_addresses_or_this_computer() {
        assert_eq!(PackageServer::parse("off"), Ok(PackageServer::Off));
        assert_eq!(PackageServer::parse(&format!("{DEFAULT_SERVER}/")), Ok(PackageServer::Default));
        assert_eq!(PackageServer::parse("https://tl.example/tlnet/"), Ok(PackageServer::Custom("https://tl.example/tlnet".into())));
        assert!(PackageServer::parse("http://127.0.0.1:8080/tlnet").is_ok());
        for bad in ["http://tl.example/tlnet", "ftp://x", "https://", "https:///x", "http://localhost.evil.example", "tl.example"] {
            assert!(PackageServer::parse(bad).is_err(), "{bad}");
        }
        // Kept in the preferences as its text.
        let json = serde_json::to_string(&PackageServer::Custom("https://tl.example/tlnet".into())).unwrap_or_default();
        assert_eq!(serde_json::from_str::<PackageServer>(&json).ok(), Some(PackageServer::Custom("https://tl.example/tlnet".into())));
    }
}
