//! Updates (docs/ROADMAP.md, A8): reading the release feeds, comparing versions, the daily
//! schedule, the setting, the fallback to the mirror, the action for each kind of install, and
//! putting a new AppImage in place. No test reaches the network: a fake transport answers the
//! feeds and counts what was asked, and a fake fetcher stands for the download.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::Digest as _;
use texcraft_compile::Unavailable;
use texcraft_engine::forge::{Request, Response, Sending, Transport};
use texcraft_engine::update::{self, Action, Install, appimage};
use texcraft_engine::{Session, UpdateEvent};

const GITLAB: &[u8] = include_bytes!("../fixtures/gitlab-release-latest.json");
const GITHUB: &[u8] = include_bytes!("../fixtures/github-release-latest.json");

/// Answers the feeds' URLs as told, and counts the requests.
#[derive(Clone, Default)]
struct Feeds {
    gitlab: Option<Result<(u16, Vec<u8>), String>>,
    github: Option<Result<(u16, Vec<u8>), String>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Feeds {
    fn gitlab() -> Self {
        Self { gitlab: Some(Ok((200, GITLAB.to_vec()))), ..Self::default() }
    }

    fn calls(&self) -> usize {
        self.asked.lock().map_or(0, |a| a.len())
    }
}

impl Transport for Feeds {
    fn send(&self, request: Request) -> Sending<'_> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(request.url.clone());
        }
        let answer = if request.url.starts_with("https://gitlab.com/") {
            self.gitlab.clone()
        } else if request.url.starts_with("https://api.github.com/") {
            self.github.clone()
        } else {
            None
        };
        let response = match answer {
            Some(Ok((status, body))) => Ok(Response { status, body, ..Response::default() }),
            Some(Err(e)) => Err(e),
            None => Err("no route to host".to_owned()),
        };
        Box::pin(std::future::ready(response))
    }
}

/// A session that checks through `feeds` for a copy installed as `install` running `running`,
/// at the time `clock` holds.
fn session(feeds: &Feeds, install: Install, running: &str, clock: &Arc<AtomicI64>) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let feeds = feeds.clone();
    s.set_update_transport(install, running, move || Box::new(feeds.clone()));
    let clock = clock.clone();
    s.set_update_clock(move || clock.load(Ordering::SeqCst));
    s
}

/// The end of the check or installation under way.
fn settle(s: &mut Session) -> Option<UpdateEvent> {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if let Some(event) = s.poll_update() {
            return Some(event);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

fn status(s: &mut Session) -> Value {
    s.execute("app.update_status", &json!({})).unwrap_or(Value::Null)
}

const NOW: i64 = 1_800_000_000;

#[test]
fn a_release_is_newer_only_by_its_version() {
    assert!(update::is_newer("0.20.0", "0.19.0"));
    assert!(update::is_newer("v1.0.0", "0.99.9"));
    assert!(!update::is_newer("0.19.0", "0.19.0"));
    assert!(!update::is_newer("0.18.2", "0.19.0"));
    assert!(!update::is_newer("not a version", "0.19.0"));
    // A build from source between releases already has the release it follows.
    assert!(!update::is_newer("0.20.0", "0.20.0-dev"));
    assert!(update::is_newer("0.21.0", "0.20.0-dev"));
}

#[test]
fn gitlabs_answer_gives_the_release_and_only_gitlabs_links() -> Result<(), String> {
    let r = update::parse_gitlab(GITLAB)?;
    assert_eq!(r.version, "0.20.0");
    assert!(r.page.starts_with("https://gitlab.com/severin-lemaignan/texcraft/"));
    assert!(r.assets.iter().any(|a| a.name == "TeXCraft-0.20.0-x86_64.AppImage"));
    assert!(r.assets.iter().any(|a| a.name == "SHA256SUMS"));
    assert!(r.assets.iter().all(|a| a.url.starts_with("https://gitlab.com/")), "a link elsewhere is left out");
    let text = update::notes_text(&r.notes);
    assert!(text.contains("Updates: TeXCraft says"), "{text}");
    assert!(!text.contains("](") && !text.contains("**") && !text.contains('#'), "{text}");
    assert!(update::parse_gitlab(b"{\"message\": \"404 Not Found\"}").is_err());
    assert!(update::parse_gitlab(b"<html>").is_err());
    Ok(())
}

#[test]
fn githubs_answer_gives_its_checksums_and_only_the_mirrors_files() -> Result<(), String> {
    let r = update::parse_github(GITHUB)?;
    assert_eq!(r.version, "0.20.0");
    let appimage = r.assets.iter().find(|a| a.name == "TeXCraft-0.20.0-x86_64.AppImage").ok_or("no AppImage")?;
    assert!(appimage.sha256.is_some());
    assert!(r.assets.iter().all(|a| a.url.starts_with("https://github.com/skadge/texcraft/releases/download/")));
    assert!(!r.assets.iter().any(|a| a.name.ends_with("macos.zip")), "another repository's file is left out");
    Ok(())
}

#[test]
fn the_action_follows_the_install() -> Result<(), String> {
    let gitlab = update::parse_gitlab(GITLAB)?;
    let github = update::parse_github(GITHUB)?;
    let appimage = Install::AppImage(PathBuf::from("/home/u/TeXCraft.AppImage"));
    // GitLab's AppImage has its line in SHA256SUMS; GitHub's its own checksum.
    assert!(matches!(update::plan(&appimage, &gitlab, "x86_64"), Action::ReplaceAppImage { sums: Some(_), .. }));
    assert!(matches!(update::plan(&appimage, &github, "x86_64"), Action::ReplaceAppImage { asset, .. } if asset.sha256.is_some()));
    // No AppImage for this machine: the release page.
    assert_eq!(update::plan(&appimage, &gitlab, "aarch64"), Action::ReleasePage);
    assert!(matches!(update::plan(&Install::Flatpak, &gitlab, "x86_64"), Action::Flatpak { bundle: Some(b) } if b.name.ends_with(".flatpak")));
    assert!(
        matches!(update::plan(&Install::Windows, &gitlab, "x86_64"), Action::Download { asset } if asset.url.starts_with("https://github.com/skadge/texcraft/releases/download/v0.20.0/") && asset.name.ends_with("windows.zip"))
    );
    assert!(matches!(update::plan(&Install::MacOs, &gitlab, "aarch64"), Action::Download { asset } if asset.name.ends_with("macos.zip")));
    // An Intel Mac: no build for it.
    assert_eq!(update::plan(&Install::MacOs, &gitlab, "x86_64"), Action::ReleasePage);
    assert_eq!(update::plan(&Install::Source, &gitlab, "x86_64"), Action::ReleasePage);
    assert_eq!(update::plan(&Install::Browser, &gitlab, "wasm32"), Action::Reload);
    // Windows and macOS link the mirror's page, where their files are.
    assert!(update::page_for(&Install::Windows, &gitlab).starts_with("https://github.com/skadge/texcraft/"));
    assert!(update::page_for(&Install::Flatpak, &gitlab).starts_with("https://gitlab.com/"));
    Ok(())
}

#[test]
fn the_automatic_check_runs_once_a_day() {
    let feeds = Feeds::gitlab();
    let clock = Arc::new(AtomicI64::new(NOW));
    let mut s = session(&feeds, Install::Source, "0.19.0", &clock);
    let started = s.execute("app.check_updates", &json!({"automatic": true}));
    assert_eq!(started.ok().map(|v| v["started"].clone()), Some(json!(true)));
    assert_eq!(settle(&mut s), Some(UpdateEvent::Checked { manual: false }));
    assert_eq!(feeds.calls(), 1);
    assert_eq!(status(&mut s)["available"], json!(true));
    // Later the same day: nothing is asked.
    clock.store(NOW + 3 * 3600, Ordering::SeqCst);
    let again = s.execute("app.check_updates", &json!({"automatic": true}));
    assert_eq!(again.ok().map(|v| v["started"].clone()), Some(json!(false)));
    assert_eq!(feeds.calls(), 1);
    // A day on, it asks again.
    clock.store(NOW + update::DAY + 1, Ordering::SeqCst);
    assert!(s.execute("app.check_updates", &json!({"automatic": true})).is_ok());
    assert!(settle(&mut s).is_some());
    assert_eq!(feeds.calls(), 2);
}

#[test]
fn with_the_setting_off_the_automatic_check_asks_nothing() {
    let feeds = Feeds::gitlab();
    let clock = Arc::new(AtomicI64::new(NOW));
    let mut s = session(&feeds, Install::Source, "0.19.0", &clock);
    assert!(s.execute("app.update_settings", &json!({"check": false})).is_ok());
    let done = s.execute("app.check_updates", &json!({"automatic": true}));
    assert_eq!(done.ok().map(|v| v["reason"].clone()), Some(json!("off")));
    assert!(s.poll_update().is_none());
    assert_eq!(feeds.calls(), 0);
    // Asked for, it runs.
    assert!(s.execute("app.check_updates", &json!({})).is_ok());
    assert_eq!(settle(&mut s), Some(UpdateEvent::Checked { manual: true }));
    assert_eq!(feeds.calls(), 1);
}

#[test]
fn a_session_the_shell_did_not_enable_contacts_nothing() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("app.check_updates", &json!({})).is_err());
    assert_eq!(status(&mut s)["enabled"], json!(false));
}

#[test]
fn the_mirror_answers_when_gitlab_does_not_and_both_failing_is_said() -> Result<(), String> {
    let feeds = Feeds { gitlab: Some(Err("connection refused".into())), github: Some(Ok((200, GITHUB.to_vec()))), ..Feeds::default() };
    let clock = Arc::new(AtomicI64::new(NOW));
    let mut s = session(&feeds, Install::Source, "0.19.0", &clock);
    assert!(s.execute("app.check_updates", &json!({})).is_ok());
    assert_eq!(settle(&mut s), Some(UpdateEvent::Checked { manual: true }));
    assert_eq!(status(&mut s)["latest"]["feed"], json!("github"));

    let offline = Feeds { gitlab: Some(Err("offline".into())), github: Some(Ok((503, Vec::new()))), ..Feeds::default() };
    let mut s = session(&offline, Install::Source, "0.19.0", &clock);
    assert!(s.execute("app.check_updates", &json!({})).is_ok());
    let Some(UpdateEvent::CheckFailed { error, .. }) = settle(&mut s) else { return Err("the check did not fail".into()) };
    assert!(error.contains("gitlab.com") && error.contains("github.com"), "{error}");
    // A failed check is not an answer: the next start tries again.
    assert_eq!(status(&mut s)["lastCheck"], Value::Null);
    Ok(())
}

/// The download, as files in memory: `files` by URL.
struct Fetcher {
    files: Vec<(String, Vec<u8>)>,
}

impl appimage::Fetch for Fetcher {
    fn text(&self, url: &str) -> Result<String, String> {
        self.files.iter().find(|(u, _)| u == url).map(|(_, b)| String::from_utf8_lossy(b).into_owned()).ok_or_else(|| format!("{url}: 404"))
    }

    fn download(&self, url: &str, dest: &Path, _sha256: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let bytes = self.files.iter().find(|(u, _)| u == url).map(|(_, b)| b.clone()).ok_or_else(|| format!("{url}: 404"))?;
        progress(bytes.len() as u64, bytes.len() as u64);
        std::fs::write(dest, bytes).map_err(|e| e.to_string())
    }
}

fn sha256(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The fetcher for GitLab's fixture: its AppImage is `new`, and SHA256SUMS lists `listed`'s sum.
fn release_files(new: &[u8], listed: &[u8]) -> Result<Fetcher, String> {
    let r = update::parse_gitlab(GITLAB)?;
    let url = |name: &str| r.assets.iter().find(|a| a.name == name).map(|a| a.url.clone()).ok_or(format!("no {name}"));
    let sums = format!("{}  TeXCraft-0.20.0-x86_64.AppImage\n{}  TeXCraft-0.20.0-x86_64.flatpak\n", sha256(listed), sha256(b"bundle"));
    Ok(Fetcher { files: vec![(url("TeXCraft-0.20.0-x86_64.AppImage")?, new.to_vec()), (url("SHA256SUMS")?, sums.into_bytes())] })
}

#[test]
fn a_new_appimage_takes_the_old_ones_place_and_the_old_one_is_kept() -> Result<(), Box<dyn std::error::Error>> {
    let dir = temp("replace")?;
    let current = dir.join("TeXCraft.AppImage");
    std::fs::write(&current, b"old version")?;
    let clock = Arc::new(AtomicI64::new(NOW));
    let mut s = session(&Feeds::gitlab(), Install::AppImage(current.clone()), "0.19.0", &clock);
    s.set_update_fetch(Arc::new(release_files(b"new version", b"new version")?));
    s.execute("app.check_updates", &json!({}))?;
    settle(&mut s);
    assert_eq!(status(&mut s)["latest"]["action"], json!("replace_appimage"));
    s.execute("app.install_update", &json!({}))?;
    assert_eq!(settle(&mut s), Some(UpdateEvent::Installed));
    assert_eq!(std::fs::read(&current)?, b"new version");
    assert_eq!(std::fs::read(appimage::beside(&current, ".old"))?, b"old version");
    assert_eq!(s.updates().restart_program(), Some(current.as_path()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(std::fs::metadata(&current)?.permissions().mode() & 0o111 != 0, "the new AppImage runs");
    }
    // Once the new one has started, the old one goes.
    appimage::forget_old(&current);
    assert!(!appimage::beside(&current, ".old").exists());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_download_that_does_not_match_its_checksum_replaces_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let dir = temp("mismatch")?;
    let current = dir.join("TeXCraft.AppImage");
    std::fs::write(&current, b"old version")?;
    let clock = Arc::new(AtomicI64::new(NOW));
    let mut s = session(&Feeds::gitlab(), Install::AppImage(current.clone()), "0.19.0", &clock);
    s.set_update_fetch(Arc::new(release_files(b"tampered", b"new version")?));
    s.execute("app.check_updates", &json!({}))?;
    settle(&mut s);
    s.execute("app.install_update", &json!({}))?;
    let Some(UpdateEvent::InstallFailed(why)) = settle(&mut s) else { return Err("the installation did not fail".into()) };
    assert!(why.contains("checksum"), "{why}");
    assert_eq!(std::fs::read(&current)?, b"old version");
    let left: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 1, "nothing but the old AppImage is left: {left:?}");
    assert!(s.updates().restart_program().is_none());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn installing_is_refused_where_this_copy_cannot_replace_itself() -> Result<(), Box<dyn std::error::Error>> {
    let clock = Arc::new(AtomicI64::new(NOW));
    for install in [Install::Flatpak, Install::Windows, Install::Source, Install::Browser] {
        let mut s = session(&Feeds::gitlab(), install.clone(), "0.19.0", &clock);
        if install != Install::Browser {
            s.execute("app.check_updates", &json!({}))?;
            settle(&mut s);
        }
        assert!(s.execute("app.install_update", &json!({})).is_err(), "{install:?}");
    }
    // Nothing newer: nothing to install.
    let mut s = session(&Feeds::gitlab(), Install::AppImage(PathBuf::from("/nowhere")), "0.20.0", &clock);
    s.execute("app.check_updates", &json!({}))?;
    settle(&mut s);
    assert_eq!(status(&mut s)["available"], json!(false));
    assert!(s.execute("app.install_update", &json!({})).is_err());
    Ok(())
}
