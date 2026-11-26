//! Saving by itself and Sync (issues #34, #35): the changes are written once typing pauses or the
//! window loses the focus, versions are made at their moments only, a failed write is said and
//! asked about, and Sync makes a version, takes in the remote and sends the project, against the
//! git server in memory. The window's clock is the harness's: each step is a quarter second.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::gitwire::fake::Server;
use texcraft_ui_egui::App;

const TOKEN: &str = "ghp-ui-sync-test";

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-ui-{}-autosave-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A project saved in `dir`, its main.tex "one".
fn saved(dir: &Path) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "one\n"}))?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    Ok(s)
}

fn harness(s: Session) -> Harness<'static, App> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    h.step();
    h
}

/// An edit, as the editor makes one, without letting the window's clock run.
fn edit(h: &mut Harness<'_, App>, text: &str) {
    let edit = json!({"command": "file.write", "params": {"path": "main.tex", "text": text}});
    assert_eq!(h.state_mut().control("engine.execute", &edit)["ok"], true);
}

fn steps(h: &mut Harness<'_, App>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn on_disk(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("main.tex")).unwrap_or_default()
}

fn versions(h: &mut Harness<'_, App>) -> usize {
    let list = h.state_mut().control("engine.execute", &json!({"command": "history.list", "params": {"limit": 1000}}));
    list["result"]["commits"].as_array().map_or(0, Vec::len)
}

/// Moves the window's clock `seconds` on, in one frame.
fn later(h: &mut Harness<'_, App>, seconds: f64) {
    let now = h.ctx.input(|i| i.time);
    h.input_mut().time = Some(now + seconds);
    h.step();
    h.input_mut().time = None;
}

#[test]
fn changes_are_written_once_typing_pauses_or_the_window_loses_the_focus() {
    let dir = temp("timing").unwrap();
    let mut h = harness(saved(&dir).unwrap());
    edit(&mut h, "two\n");
    steps(&mut h, 4);
    assert_eq!(on_disk(&dir), "one\n", "written while typing");
    assert!(h.query_by_label("Saving…").is_some(), "the top bar does not say the changes wait");
    // Typing again starts the pause again.
    edit(&mut h, "three\n");
    steps(&mut h, 4);
    assert_eq!(on_disk(&dir), "one\n", "written before the pause");
    steps(&mut h, 3);
    assert_eq!(on_disk(&dir), "three\n", "not written after the pause");
    assert!(!h.state().session().is_dirty());
    assert!(h.query_by_label("All changes saved").is_some());

    // Leaving the window writes at once.
    edit(&mut h, "four\n");
    h.step();
    h.input_mut().focused = false;
    h.step();
    assert_eq!(on_disk(&dir), "four\n", "not written when the window lost the focus");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_is_made_after_the_long_pause_and_on_closing_not_on_each_write() {
    let dir = temp("versions").unwrap();
    let mut h = harness(saved(&dir).unwrap());
    let first = versions(&mut h);
    for text in ["two\n", "three\n", "four\n"] {
        edit(&mut h, text);
        steps(&mut h, 8);
        assert_eq!(on_disk(&dir), text);
    }
    assert_eq!(versions(&mut h), first, "a version for a write");
    later(&mut h, 301.0);
    assert_eq!(versions(&mut h), first + 1, "no version after the long pause");
    later(&mut h, 301.0);
    assert_eq!(versions(&mut h), first + 1, "a version with nothing changed");

    edit(&mut h, "five\n");
    h.step();
    let before_closing = latest(&mut h);
    assert!(!h.state_mut().close_requested(), "closing asks although the window saves by itself");
    assert_eq!(on_disk(&dir), "five\n", "closing did not write");
    // The version on closing amends the session's automatic one (not pushed): one version of both.
    assert_eq!(versions(&mut h), first + 1, "closing added a version beside the session's automatic one");
    assert_ne!(latest(&mut h), before_closing, "closing made no version");
    let commit = latest(&mut h);
    let text = h.state_mut().control("engine.execute", &json!({"command": "history.show", "params": {"commit": commit, "path": "main.tex"}}));
    assert_eq!(text["result"]["text"], "five\n", "the version on closing does not hold the last change");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The latest version's id.
fn latest(h: &mut Harness<'_, App>) -> Value {
    let list = h.state_mut().control("engine.execute", &json!({"command": "history.list", "params": {"limit": 1}}));
    list["result"]["commits"][0]["id"].clone()
}

#[test]
fn a_write_that_fails_is_said_and_closing_asks_again() {
    let dir = temp("fails").unwrap();
    let folder = dir.join("paper");
    let mut h = harness(saved(&folder).unwrap());
    // The folder is gone, and a file is where it was: nothing can be written there.
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::write(&folder, "in the way").unwrap();
    edit(&mut h, "two\n");
    steps(&mut h, 8);
    assert!(h.state().session().is_dirty(), "a failed write is said to have worked");
    assert!(h.query_by_label("Not saved").is_some(), "the top bar does not say it");
    // Not tried again every frame: once the project changes, or the user asks.
    assert!(h.state_mut().close_requested(), "closing does not ask");
    steps(&mut h, 2);
    assert!(h.query_by_label_contains("could not be saved").is_some(), "the question does not say why it is asked");
    assert!(h.query_by_label("Try Again").is_some() && h.query_by_label("Don't Save").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A bare repository at `dir`, as `git init --bare` makes it.
fn bare(dir: &Path) -> std::io::Result<PathBuf> {
    for sub in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n")?;
    std::fs::write(dir.join("config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n")?;
    Ok(dir.to_path_buf())
}

fn reaching(mut s: Session, server: &Server) -> Session {
    let server = server.clone();
    s.set_git_http(move || Box::new(server.clone()));
    s
}

/// Runs frames until the transfer under way ends (it runs on a thread of its own); whether it
/// ended.
fn until_done(h: &mut Harness<'_, App>) -> bool {
    for _ in 0..500 {
        h.step();
        if h.state().session().git_busy().is_none() {
            h.step();
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

fn head(h: &mut Harness<'_, App>) -> Value {
    h.state_mut().control("engine.execute", &json!({"command": "history.status", "params": {}}))["result"]["head"].clone()
}

fn dialog(h: &mut Harness<'_, App>) -> Value {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["dialog"].clone()
}

#[test]
fn sync_sends_a_version_of_the_changes_and_counts_what_waits() {
    let dir = temp("sync").unwrap();
    let server = Server::new(&bare(&dir.join("server.git")).unwrap()).with_auth("x-access-token", TOKEN);
    let mut s = reaching(saved(&dir.join("a")).unwrap(), &server);
    s.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"})).unwrap();
    s.execute("git.push", &json!({"token": TOKEN})).unwrap();
    let mut h = harness(s);
    assert!(h.query_by_label_contains("Sync: up to date").is_some(), "no Sync beside the name, or not up to date");

    edit(&mut h, "two\n");
    steps(&mut h, 8);
    assert!(h.query_by_label_contains("Sync: 1 version to send").is_some(), "the change waiting is not counted");
    h.query_by_label_contains("Sync: 1 version to send").unwrap().click();
    assert!(until_done(&mut h), "the transfer did not end");
    let sent = server.get("refs/heads/main").map(|id| id.to_string());
    assert_eq!(sent.as_deref(), head(&mut h).as_str(), "the version was not sent");
    assert!(h.query_by_label_contains("Synced with origin/main").is_some(), "the status line does not say it");
    assert!(h.query_by_label_contains("Sync: up to date").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sync_in_conflict_opens_the_choice_and_choosing_syncs() {
    let dir = temp("conflict").unwrap();
    let server = Server::new(&bare(&dir.join("server.git")).unwrap()).with_auth("x-access-token", TOKEN);
    let mut a = reaching(saved(&dir.join("a")).unwrap(), &server);
    a.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"})).unwrap();
    a.execute("project.sync", &json!({"token": TOKEN})).unwrap();
    // Another copy changes the same line and sends it first.
    let mut b = reaching(Session::with_backend(Box::new(Unavailable("tests".into()))), &server);
    copy(&dir.join("a"), &dir.join("b")).unwrap();
    b.execute("project.open", &json!({"path": dir.join("b").display().to_string()})).unwrap();
    b.execute("file.write", &json!({"path": "main.tex", "text": "theirs\n"})).unwrap();
    b.execute("project.sync", &json!({"token": TOKEN})).unwrap();

    let mut h = harness(a);
    edit(&mut h, "mine\n");
    steps(&mut h, 8);
    h.query_by_label_contains("Sync:").unwrap().click();
    assert!(until_done(&mut h), "the transfer did not end");
    assert_eq!(dialog(&mut h), "forge_resolve", "the files in conflict are not offered");
    h.get_by_label("Keep mine: main.tex").click();
    h.step();
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Sync").click();
    assert!(until_done(&mut h), "the transfer did not end");
    b.execute("project.sync", &json!({})).unwrap();
    let path = texcraft_engine::project::ProjectPath::new("main.tex").unwrap();
    assert_eq!(b.project().text(&path).ok(), Some("mine\n"), "mine did not reach the remote");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
fn an_overleaf_remote_is_named_and_its_token_asked_for() {
    const OVERLEAF: &str = "olp_ui0test0token";
    let dir = temp("overleaf").unwrap();
    let server = Server::new(&bare(&dir.join("server.git")).unwrap()).with_auth("git", OVERLEAF);
    let mut s = reaching(saved(&dir.join("p")).unwrap(), &server);
    s.execute("git.set_remote", &json!({"name": "origin", "url": "https://git@git.overleaf.com/69cb8d545624606190d85c57"})).unwrap();
    let mut h = harness(s);
    assert!(h.query_by_label("Overleaf").is_some(), "the service is not named beside Sync");
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "view.side.history");
    h.step();
    assert!(h.query_all_by_label("Overleaf").count() >= 2, "the History panel does not name the service");

    h.query_by_label_contains("Sync:").unwrap().click();
    steps(&mut h, 3);
    assert_eq!(dialog(&mut h), "git_token", "no token is asked for");
    h.get_by_label("Token").type_text(OVERLEAF);
    h.step();
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Sync").click();
    assert!(until_done(&mut h), "the transfer did not end");
    let sent = server.get("refs/heads/main").map(|id| id.to_string());
    assert_eq!(sent.as_deref(), head(&mut h).as_str(), "nothing reached the server");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_history_panel_offers_push_and_pull_as_buttons_and_no_commit() {
    let dir = temp("panel").unwrap();
    let server = Server::new(&bare(&dir.join("server.git")).unwrap()).with_auth("x-access-token", TOKEN);
    let mut s = reaching(saved(&dir.join("p")).unwrap(), &server);
    s.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"})).unwrap();
    let mut h = harness(s);
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "view.side.history");
    h.step();
    let button = egui::accesskit::Role::Button;
    assert!(h.query_by_role_and_label(button, "Push: nothing sent to the remote yet").is_some(), "Push does not say what it sends");
    assert!(h.query_by_role_and_label(button, "Pull").is_some());
    assert!(h.query_by_label("Commit…").is_none(), "the greyed Commit… is still there");
    assert!(h.query_by_label_contains("git repository of its own").is_none(), "the long explanation is still there");
    assert!(h.query_by_role_and_label(button, "Name This Version…").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}
