//! Updates in the window (docs/ROADMAP.md, A8): the daily check after the start, the status bar's
//! notice, the What's New dialog and its action for the kind of install, and Settings' row. A fake
//! transport answers GitLab's releases API; nothing reaches the network.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::{Request, Response, Sending, Transport};
use texcraft_engine::update::{Install, Saved};
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

/// GitLab's answer for a made-up release 0.20.0 (our own, in the API's shape).
const RELEASE: &str = r###"{
  "name": "TeXCraft 0.20.0", "tag_name": "v0.20.0", "upcoming_release": false,
  "description": "## What's new\n\n- **Faster** builds.\n- A notice when a new version is out.",
  "released_at": "2026-12-01T10:00:00.000Z",
  "assets": {"links": [
    {"name": "TeXCraft-0.20.0-x86_64.AppImage", "url": "https://gitlab.com/api/v4/projects/1/packages/generic/texcraft/0.20.0/TeXCraft-0.20.0-x86_64.AppImage", "link_type": "package"},
    {"name": "SHA256SUMS", "url": "https://gitlab.com/api/v4/projects/1/packages/generic/texcraft/0.20.0/SHA256SUMS", "link_type": "other"}
  ]},
  "_links": {"self": "https://gitlab.com/severin-lemaignan/texcraft/-/releases/v0.20.0"}
}"###;

/// Answers every request with the release, and keeps the URLs asked.
#[derive(Clone, Default)]
struct Feed(Arc<Mutex<Vec<String>>>);

impl Feed {
    fn calls(&self) -> usize {
        self.0.lock().map_or(0, |a| a.len())
    }
}

impl Transport for Feed {
    fn send(&self, request: Request) -> Sending<'_> {
        // The browser build asks its site for `version.txt`; the desktop asks GitLab.
        let body = if request.url.starts_with("version.txt") { b"0.20.0\n".to_vec() } else { RELEASE.as_bytes().to_vec() };
        if let Ok(mut asked) = self.0.lock() {
            asked.push(request.url);
        }
        Box::pin(std::future::ready(Ok(Response { status: 200, body, ..Response::default() })))
    }
}

fn window(feed: &Feed, install: Install, updates: Saved, step: f32) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let feed = feed.clone();
    s.set_update_transport(install, "0.19.0", move || Box::new(feed.clone()));
    let mut app = App::with_prefs(s, Prefs { auto_compile: false, updates, ..Prefs::default() });
    app.show_start_page();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_step_dt(step)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

/// Runs frames until the check has ended.
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..500 {
        h.step();
        if !h.state().session().updates().checking() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
}

#[test]
fn the_daily_check_waits_after_the_start_and_asks_nothing_when_switched_off() {
    let feed = Feed::default();
    let mut h = window(&feed, Install::Source, Saved::default(), 0.5);
    assert_eq!(feed.calls(), 0, "the start is not slowed by a check");
    for _ in 0..30 {
        h.step();
    }
    settle(&mut h);
    assert_eq!(feed.calls(), 1);
    assert!(h.query_by_label("TeXCraft 0.20.0 is available").is_some(), "the status bar says a new version is out");

    let off = Feed::default();
    let mut h = window(&off, Install::Source, Saved { check: false, last_check: None }, 0.5);
    for _ in 0..30 {
        h.step();
    }
    h.run();
    assert_eq!(off.calls(), 0);
    assert!(h.query_by_label("TeXCraft 0.20.0 is available").is_none());
}

#[test]
fn the_notice_opens_whats_new_with_the_notes_and_this_copys_action() {
    let feed = Feed::default();
    let mut h = window(&feed, Install::Source, Saved { check: false, last_check: None }, 1.0 / 60.0);
    ui_cmd::run(h.state_mut(), "help.check_updates");
    settle(&mut h);
    assert!(h.query_by_label_contains("Faster builds").is_some(), "the notes are shown as text");
    // Built from source: the release page, said why.
    assert!(h.query_by_label("Open the Release Page").is_some());
    assert!(h.query_by_label_contains("built from source").is_some());
    h.get_by_label("Close").click();
    h.run();
    assert!(h.query_by_label("What's New").is_none(), "the dialog closed");
    h.get_by_role_and_label(Role::Button, "TeXCraft 0.20.0 is available").click();
    h.run();
    assert!(h.query_by_label("What's New").is_some(), "the notice opens the dialog again");
}

#[test]
fn an_appimage_offers_to_download_and_install() {
    let feed = Feed::default();
    let mut h = window(&feed, Install::AppImage(PathBuf::from("/nowhere/TeXCraft.AppImage")), Saved { check: false, last_check: None }, 1.0 / 60.0);
    ui_cmd::run(h.state_mut(), "help.check_updates");
    settle(&mut h);
    assert!(h.query_by_label("Download and Install").is_some());
    assert!(h.query_by_label_contains("checks it against the release's checksums").is_some());
}

#[test]
fn a_flatpak_says_updates_come_through_flatpak() {
    let feed = Feed::default();
    let mut h = window(&feed, Install::Flatpak, Saved { check: false, last_check: None }, 1.0 / 60.0);
    ui_cmd::run(h.state_mut(), "help.check_updates");
    settle(&mut h);
    assert!(h.query_by_label_contains("Updates come through Flatpak").is_some());
    assert!(h.query_by_label("Download and Install").is_none(), "a Flatpak is not replaced");
}

#[test]
fn settings_switch_the_daily_check_and_say_what_it_contacts() {
    let feed = Feed::default();
    let mut h = window(&feed, Install::Source, Saved::default(), 1.0 / 60.0);
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    assert!(h.query_by_label_contains("Asks gitlab.com once a day").is_some());
    h.get_by_role_and_label(Role::CheckBox, "Check for updates").click();
    h.run();
    assert!(!h.state().session().settings.updates.check);
    let kept = h.state_mut().take_changed_prefs().map(|p| p.updates.check);
    assert_eq!(kept, Some(false), "the setting is kept with the preferences");
}

#[test]
fn the_browser_compares_with_its_site_and_reloads() {
    let feed = Feed::default();
    let mut h = window(&feed, Install::Browser, Saved { check: false, last_check: None }, 1.0 / 60.0);
    ui_cmd::run(h.state_mut(), "help.check_updates");
    settle(&mut h);
    assert!(h.query_by_label_contains("Reloading loads the new version").is_some());
    h.get_by_role_and_label(Role::Button, "Reload").click();
    h.run();
    assert!(h.state_mut().take_reload(), "the page is asked to reload");
}
