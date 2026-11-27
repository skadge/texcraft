//! Import a Project (docs/design/import-dialog.md): the grid of sources (names, availability, the
//! keyboard moving between the cards), each page's main path against the fakes (a real clone from
//! the git server in memory, the forges' API), stopping an import, the errors in words, the folder
//! new projects go to, and the phone's list. No test reaches the network.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::gitwire::fake::Server;
use texcraft_engine::gitwire::{Http, Reply};
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, Pick, ui_cmd};

const NOTES: &str = "https://example.org/lab/notes.git";

pub(crate) fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("texcraft-ui-{}-import-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A bare repository at `dir`, as `git init --bare` makes it.
fn bare(dir: &Path) -> PathBuf {
    for sub in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        let _ = std::fs::create_dir_all(dir.join(sub));
    }
    let _ = std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n");
    let _ = std::fs::write(dir.join("config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n");
    dir.to_path_buf()
}

/// A git server in memory holding a project of two commits, pushed to it at `url` by another
/// session; `auth` the user name and token it wants, if any.
pub(crate) fn published(dir: &Path, url: &str, auth: Option<(&str, &str)>) -> Result<Server, String> {
    let server = Server::new(&bare(&dir.join("server.git")));
    let server = match auth {
        Some((user, token)) => server.with_auth(user, token),
        None => server,
    };
    let mut a = Session::with_backend(Box::new(Unavailable("tests".into())));
    let sent = server.clone();
    a.set_git_http(move || Box::new(sent.clone()));
    let paper = |line: &str| format!("\\documentclass{{article}}\n\\begin{{document}}\n{line}\n\\end{{document}}\n");
    a.execute("file.write", &json!({"path": "main.tex", "text": paper("One")}))?;
    a.execute("project.save_as", &json!({"path": dir.join("a").display().to_string()}))?;
    a.execute("file.write", &json!({"path": "main.tex", "text": paper("Two")}))?;
    a.execute("project.save", &json!({}))?;
    a.execute("git.set_remote", &json!({"name": "origin", "url": url}))?;
    a.execute("git.push", &json!({"token": auth.map_or("unused", |(_, t)| t)}))?;
    Ok(server)
}

/// The window, reaching git servers through `http`, its documents folder `documents`.
pub(crate) fn window(session: Session, documents: &Path, size: egui::Vec2) -> Harness<'static, App> {
    window_with(session, documents, size, Prefs { auto_compile: false, ..Prefs::default() })
}

fn window_with(session: Session, documents: &Path, size: egui::Vec2, prefs: Prefs) -> Harness<'static, App> {
    let mut app = App::with_prefs(session, prefs);
    app.set_documents_dir(Some(documents.to_path_buf()));
    let mut h = Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

fn session(http: impl Fn() -> Box<dyn Http> + Send + Sync + 'static) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_git_http(http);
    s
}

fn over(server: &Server) -> Session {
    let server = server.clone();
    session(move || Box::new(server.clone()))
}

const WIDE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// Runs frames until typing has paused long enough for an address check, and the check or the
/// clone running has ended.
pub(crate) fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..45 {
        h.step();
    }
    for _ in 0..600 {
        h.step();
        if h.state().session().import_busy().is_none() && h.state().session().forge_busy().is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
}

fn type_into(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
}

fn open(h: &mut Harness<'_, App>, source: Option<&str>) {
    let params = source.map_or_else(|| json!({}), |s| json!({"source": s}));
    ui_cmd::run_with(h.state_mut(), "project.import_dialog", &params);
    h.run();
    h.run();
}

/// The accessible name of the node with the keyboard.
fn focused(h: &Harness<'_, App>) -> String {
    h.query_all_by(|n| n.is_focused()).last().and_then(|n| n.accesskit_node().label()).unwrap_or_default()
}

fn key(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    h.run();
}

fn said(h: &Harness<'_, App>, text: &str) -> bool {
    h.query_all_by_label_contains(text).count() > 0
}

#[test]
fn the_sources_are_named_grouped_and_crossed_with_the_arrows() {
    let dir = temp("grid");
    let mut h = window(session(|| Box::new(Answer(Some(404)))), &dir, WIDE);
    open(&mut h, None);
    for name in [
        "GitLab, Sign in or use a token",
        "GitHub, Sign in or use a token",
        "Overleaf, Clone address and token",
        "GitLab Server, Add your server",
        "Git Address, Any HTTPS address",
        "Folder, Opens it where it is",
        "Zip File, Unzipped into a new folder",
    ] {
        assert!(h.query_by_role_and_label(Role::Button, name).is_some(), "no card {name}");
    }
    assert!(h.query_by_role_and_label(Role::Group, "Online").is_some() && h.query_by_role_and_label(Role::Group, "On this computer").is_some());
    // Opening gives the keyboard to the first card; the arrows move it in two dimensions.
    assert_eq!(focused(&h), "GitLab, Sign in or use a token");
    key(&mut h, egui::Key::ArrowRight);
    assert_eq!(focused(&h), "GitHub, Sign in or use a token");
    key(&mut h, egui::Key::ArrowDown);
    assert!(focused(&h).starts_with("Git Address") || focused(&h).starts_with("Folder"), "Down did not go to the next row: {}", focused(&h));
    key(&mut h, egui::Key::End);
    assert_eq!(focused(&h), "Zip File, Unzipped into a new folder");
    key(&mut h, egui::Key::Home);
    assert_eq!(focused(&h), "GitLab, Sign in or use a token");
    // The cards read in order, left to right then down.
    let gitlab = h.get_by_role_and_label(Role::Button, "GitLab, Sign in or use a token").rect();
    let github = h.get_by_role_and_label(Role::Button, "GitHub, Sign in or use a token").rect();
    let folder = h.get_by_role_and_label(Role::Button, "Folder, Opens it where it is").rect();
    assert!(github.left() > gitlab.right() && (github.top() - gitlab.top()).abs() < 1.0 && folder.top() > gitlab.bottom());

    // Enter opens a page; Back returns to the grid with the card focused; Escape closes.
    for _ in 0..4 {
        key(&mut h, egui::Key::ArrowRight);
    }
    assert!(focused(&h).starts_with("Git Address"));
    key(&mut h, egui::Key::Enter);
    assert!(h.query_by_label("Import from a Git Address").is_some(), "Enter did not open the page");
    h.get_by_role_and_label(Role::Button, "Back to sources").click();
    h.run();
    h.run();
    assert!(h.query_by_label("Import a Project").is_some());
    assert_eq!(focused(&h), "Git Address, Any HTTPS address", "Back did not give the keyboard to the card chosen");
    key(&mut h, egui::Key::Escape);
    assert!(h.query_by_label("Import a Project").is_none(), "Escape did not close the dialog");
}

#[test]
fn in_the_browser_unreachable_sources_stay_and_say_the_way_out() {
    let dir = temp("browser");
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.use_library(texcraft_engine::library::Library::default());
    let mut h = window(s, &dir, WIDE);
    open(&mut h, None);
    for name in ["Overleaf, Desktop app only", "Git Address, Desktop app only", "GitHub, Personal access token", "Folder, Copied into this browser"] {
        assert!(h.query_by_role_and_label(Role::Button, name).is_some(), "no card {name}");
    }
    assert!(h.query_by_role_and_label(Role::Group, "On this device").is_some());
    let overleaf = h.get_by_role_and_label(Role::Button, "Overleaf, Desktop app only");
    assert!(!overleaf.accesskit_node().is_disabled(), "an unavailable card is still focusable and chosen");
    overleaf.click();
    h.run();
    assert!(said(&h, "Overleaf's git server does not answer web pages"), "the page does not say why");
    assert!(h.query_by_role_and_label(Role::Button, "Get the Desktop App").is_some());
    h.get_by_role_and_label(Role::Button, "Import a Zip File…").click();
    h.run();
    assert!(h.state_mut().take_picks().iter().any(|p| p.pick == Pick::ImportProjectZip), "the way out does not ask for a zip");
}

#[test]
fn a_git_address_is_checked_then_cloned_into_the_documents_folder_and_opened() {
    let dir = temp("git");
    let server = published(&dir, NOTES, None).unwrap();
    let docs = dir.join("Documents");
    let _ = std::fs::create_dir_all(&docs);
    let mut h = window(over(&server), &docs, WIDE);
    open(&mut h, Some("git"));
    type_into(&mut h, "Address", NOTES);
    settle(&mut h);
    assert!(said(&h, "Public repository · 1 branch"), "the check's answer is not said");
    let into = h.get_by_label("Into").accesskit_node().value().unwrap_or_default();
    assert_eq!(PathBuf::from(&into), docs.join("notes"), "the folder does not follow the repository's name");
    h.get_by_role_and_label(Role::Button, "Import").click();
    settle(&mut h);
    assert!(said(&h, "Imported notes (main) into"), "the status line does not say where it went");
    assert!(h.query_by_label("Import from a Git Address").is_none(), "the dialog stayed open");
    assert_eq!(h.state().session().dir(), Some(&docs.join("notes")));
    let remotes = h.state_mut().control("engine.execute", &json!({"command": "git.remotes", "params": {}}))["result"].clone();
    assert_eq!(remotes["upstream"], json!({"remote": "origin", "branch": "main"}), "Sync has nothing to set up: {remotes}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_occupied_folder_is_flagged_with_a_free_name_to_use() {
    let dir = temp("occupied");
    let server = published(&dir, NOTES, None).unwrap();
    let _ = std::fs::create_dir_all(dir.join("notes"));
    let _ = std::fs::write(dir.join("notes/mine.txt"), "mine");
    let mut h = window(over(&server), &dir, WIDE);
    open(&mut h, Some("git"));
    type_into(&mut h, "Address", NOTES);
    settle(&mut h);
    assert!(said(&h, "A folder named notes is there already."));
    assert!(h.get_by_role_and_label(Role::Button, "Import").accesskit_node().is_disabled(), "Import is offered into a folder in use");
    h.get_by_role_and_label(Role::Button, "Use notes-2").click();
    h.run();
    assert_eq!(PathBuf::from(h.get_by_label("Into").accesskit_node().value().unwrap_or_default()), dir.join("notes-2"));
    assert!(!h.get_by_role_and_label(Role::Button, "Import").accesskit_node().is_disabled());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn change_chooses_where_new_projects_go_and_it_is_kept() {
    let dir = temp("change");
    let mut h = window(session(|| Box::new(Answer(Some(404)))), &dir, WIDE);
    open(&mut h, Some("git"));
    h.get_by_role_and_label(Role::Button, "Change…").click();
    h.run();
    let picks = h.state_mut().take_picks();
    let pick = picks.iter().find(|p| p.pick == Pick::CloneFolder).expect("Change… asks for a folder");
    assert_eq!(pick.directory.as_deref(), Some(dir.as_path()), "the picker does not start where projects go");
    let elsewhere = dir.join("Papers");
    let _ = std::fs::create_dir_all(&elsewhere);
    h.state_mut().picked(Pick::CloneFolder, vec![elsewhere.clone()]);
    h.run();
    type_into(&mut h, "Address", NOTES);
    h.run();
    assert_eq!(PathBuf::from(h.get_by_label("Into").accesskit_node().value().unwrap_or_default()), elsewhere.join("notes"));
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    assert_eq!(prefs.clone_folder.as_deref().map(PathBuf::from), Some(elsewhere), "the folder chosen is not the default from now on");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server that answers every request with `status`, or (none) cannot be reached.
struct Answer(Option<u16>);

impl Http for Answer {
    fn get(&self, _: &str, _: &[(String, String)]) -> Result<Reply, String> {
        self.0.map(|status| Reply { status, content_type: None, body: Vec::new() }).ok_or_else(|| "connection refused".to_owned())
    }
    fn post(&self, url: &str, headers: &[(String, String)], _: Vec<u8>) -> Result<Reply, String> {
        self.get(url, headers)
    }
}

#[test]
fn refusals_are_said_under_the_address_in_words() {
    let dir = temp("errors");
    for (status, words) in [
        (Some(404), "No git repository at this address. Copy the address again from the project's page."),
        (Some(403), "This account cannot read lab/notes"),
        (None, "Could not reach example.org. Check the connection, then try again."),
    ] {
        let mut h = window(session(move || Box::new(Answer(status))), &dir, WIDE);
        open(&mut h, Some("git"));
        type_into(&mut h, "Address", NOTES);
        settle(&mut h);
        assert!(said(&h, words), "{status:?}: not said: {words}");
        assert!(h.get_by_role_and_label(Role::Button, "Import").accesskit_node().is_disabled());
    }
    // A server that wants signing in: the fields appear, and a wrong token is said.
    let _ = std::fs::create_dir_all(dir.join("closed"));
    let server = published(&dir.join("closed"), "https://ada@example.org/lab/notes.git", Some(("ada", "right-token"))).unwrap();
    let mut h = window(over(&server), &dir, WIDE);
    open(&mut h, Some("git"));
    assert!(h.query_by_label("Password or token").is_none(), "the sign-in fields come only when needed");
    type_into(&mut h, "Address", NOTES);
    settle(&mut h);
    assert!(said(&h, "This repository needs signing in."));
    type_into(&mut h, "User name", "ada");
    type_into(&mut h, "Password or token", "wrong-token");
    settle(&mut h);
    assert!(said(&h, "example.org did not accept the token"), "a refused token is not said");
    // An SSH address: its HTTPS form offered.
    let mut h = window(over(&server), &dir, WIDE);
    open(&mut h, Some("git"));
    type_into(&mut h, "Address", "git@example.org:lab/notes.git");
    settle(&mut h);
    assert!(said(&h, "TeXCraft uses git over HTTPS."));
    h.get_by_role_and_label(Role::Button, "Use https://example.org/lab/notes.git").click();
    h.run();
    assert_eq!(h.get_by_label("Address").accesskit_node().value().as_deref(), Some("https://example.org/lab/notes.git"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_history_too_large_is_said_with_the_limit_and_leaves_no_folder() {
    let dir = temp("large");
    let server = published(&dir, NOTES, None).unwrap();
    let mut s = over(&server);
    s.set_clone_limit(64);
    let mut h = window(s, &dir, WIDE);
    open(&mut h, Some("git"));
    type_into(&mut h, "Address", NOTES);
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Import").click();
    settle(&mut h);
    assert!(said(&h, "TeXCraft imports up to"), "the limit is not said");
    assert!(h.query_by_role_and_label(Role::Group, "notes, Import failed").is_some(), "the card does not say it failed");
    assert!(!dir.join("notes").exists(), "the folder made is left behind");
    // Try Again: the page as it was, to try another.
    h.get_by_role_and_label(Role::Button, "Try Again: notes").click();
    h.run();
    assert!(h.query_by_label("Import from a Git Address").is_some(), "the page is not back to try another");
    assert_eq!(h.get_by_label("Address").accesskit_node().value().as_deref(), Some(NOTES), "what was typed is lost");
    assert!(h.query_by_role_and_label(Role::Group, "notes, Import failed").is_none(), "the failed card stayed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The server, answering slowly: a clone takes long enough to be stopped.
struct Slow(Server);

impl Http for Slow {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Reply, String> {
        Http::get(&self.0, url, headers)
    }
    fn post(&self, url: &str, headers: &[(String, String)], body: Vec<u8>) -> Result<Reply, String> {
        std::thread::sleep(std::time::Duration::from_millis(600));
        Http::post(&self.0, url, headers, body)
    }
}

#[test]
fn cancel_stops_the_clone_and_removes_the_folder_it_made() {
    let dir = temp("cancel");
    let server = published(&dir, NOTES, None).unwrap();
    let mut h = window(session(move || Box::new(Slow(server.clone()))), &dir, WIDE);
    open(&mut h, Some("git"));
    type_into(&mut h, "Address", NOTES);
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Import").click();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(h.state().session().import_busy(), Some("clone"));
    // The dialog closed and the project's card is on the start page at once, with its phase.
    assert!(h.query_by_label("Import from a Git Address").is_none(), "the dialog stayed over the start page");
    assert!(h.query_by_role_and_label(Role::Group, "notes, Importing…").is_some(), "no card for the import");
    let phase = h.query_all_by_label_contains("Importing… ").any(|n| n.accesskit_node().live() != egui::accesskit::Live::Off);
    assert!(phase, "the card's phase is not a live region");
    assert!(said(&h, "Receiving the history…") || said(&h, "Asking the server for the project…"), "the progress is not said");
    assert!(dir.join("notes").exists(), "no folder made");
    // A click as a person makes it: the press and the release in frames of their own.
    let at = h.get_by_role_and_label(Role::Button, "Cancel: notes").rect().center();
    h.event(egui::Event::PointerMoved(at));
    h.step();
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
        h.step();
    }
    for _ in 0..10 {
        h.step();
    }
    assert!(said(&h, "Import stopped: the folder it made is removed."), "{:?}", h.state().session().import_busy());
    assert!(h.query_by_role_and_label(Role::Group, "notes, Importing…").is_none(), "the card stayed");
    assert!(!dir.join("notes").exists(), "the folder is still there");
    // The clone's thread ends by itself and leaves nothing either.
    std::thread::sleep(std::time::Duration::from_millis(900));
    settle(&mut h);
    assert!(!dir.join("notes").exists(), "the folder is still there");
    assert!(h.state().session().dir().is_none(), "a project was opened");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_overleaf_project_is_cloned_with_its_git_token() {
    const TOKEN: &str = "olp_ui0test0token";
    let dir = temp("overleaf");
    let url = "https://git.overleaf.com/6f2c1a9e4b7d";
    let server = published(&dir, url, Some(("git", TOKEN))).unwrap();
    let mut h = window(over(&server), &dir, WIDE);
    open(&mut h, Some("overleaf"));
    type_into(&mut h, "Clone address", "https://example.org/x");
    assert!(said(&h, "This is not an Overleaf git address. It starts with https://git.overleaf.com/."));
    // A whole clone command pasted is read as its address.
    let mut h = window(over(&server), &dir, WIDE);
    open(&mut h, Some("overleaf"));
    type_into(&mut h, "Clone address", &format!("git clone {url}"));
    type_into(&mut h, "Git token", TOKEN);
    settle(&mut h);
    assert!(said(&h, "Project found"), "the address and token are not checked");
    assert!(
        h.get_by_role_and_label(Role::CheckBox, "Remember on this computer").accesskit_node().data().toggled()
            == Some(egui::accesskit::Toggled::True),
        "Remember is not ticked by default"
    );
    h.get_by_role_and_label(Role::Button, "Import").click();
    settle(&mut h);
    assert!(said(&h, "Imported 6f2c1a9e4b7d (main) into"), "not imported");
    let remotes = h.state_mut().control("engine.execute", &json!({"command": "git.remotes", "params": {}}))["result"].clone();
    assert_eq!(remotes["hosting"], "overleaf", "{remotes}");
    // Remembered: the host goes to the preferences (the token to the keychain), and Settings ›
    // Accounts lists it with Forget.
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    assert_eq!(prefs.git_hosts, ["git.overleaf.com"]);
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("Accounts").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Forget: git.overleaf.com").click();
    h.run();
    assert!(said(&h, "Forgot the token kept for git.overleaf.com."));
    assert!(h.state_mut().take_changed_prefs().is_some_and(|p| p.git_hosts.is_empty()), "the host is still in the preferences");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_github_project_is_found_by_its_suggestions_and_cloned_with_the_token() {
    const TOKEN: &str = "github_pat_import_test";
    let dir = temp("github");
    let fake = texcraft_engine::forge::fake::Fake::github("ada/paper", TOKEN, &[("main.tex", b"x\n")]);
    let server = published(&dir, "https://github.com/ada/paper.git", Some(("x-access-token", TOKEN))).unwrap();
    let mut s = over(&server);
    s.set_forge_transport(move || fake.transport());
    let mut h = window(s, &dir, WIDE);
    open(&mut h, Some("github"));
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    type_into(&mut h, "Personal access token", TOKEN);
    crate::forge::quiet(&mut h);
    h.get_by_role_and_label(Role::EditableComboBox, "Project").click();
    h.run();
    h.get_by_role_and_label(Role::EditableComboBox, "Project").type_text("pap");
    crate::forge::quiet(&mut h);
    key(&mut h, egui::Key::ArrowDown);
    key(&mut h, egui::Key::Enter);
    crate::forge::quiet(&mut h);
    assert_eq!(PathBuf::from(h.get_by_label("Into").accesskit_node().value().unwrap_or_default()), dir.join("paper"));
    h.get_by_role_and_label(Role::Button, "Import").click();
    settle(&mut h);
    assert!(said(&h, "Imported paper (main) into"), "not imported");
    assert!(dir.join("paper/.git").is_dir(), "not a clone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn on_a_phone_the_sources_are_a_list_inside_the_window() {
    const PHONE: egui::Vec2 = egui::vec2(400.0, 820.0);
    let dir = temp("phone");
    let mut h = window(session(|| Box::new(Answer(Some(404)))), &dir, PHONE);
    open(&mut h, None);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, PHONE).expand(1.0);
    let titles = ["GitLab, ", "GitHub, ", "Overleaf, ", "GitLab Server, ", "Git Address, ", "Folder, ", "Zip File, "];
    let rows: Vec<egui::Rect> = h
        .query_all_by_role(Role::Button)
        .filter(|n| n.accesskit_node().label().is_some_and(|l| titles.iter().any(|t| l.starts_with(t))))
        .map(|n| n.rect())
        .collect();
    assert_eq!(rows.len(), 7, "{rows:?}");
    for pair in rows.windows(2) {
        assert!(pair[1].top() >= pair[0].bottom() - 0.5, "two sources side by side or overlapping: {pair:?}");
    }
    let visible: Vec<_> = rows.iter().filter(|r| r.bottom() <= screen.bottom()).collect();
    assert!(visible.iter().all(|r| screen.contains_rect(**r)), "a row is past the window's sides: {rows:?}");
    assert!(rows.iter().all(|r| r.height() >= 55.0), "the rows are not a finger's height");
}

#[test]
fn a_project_on_a_gitlab_server_added_is_chosen_there_and_cloned() {
    const TOKEN: &str = "glpat-server-import";
    const SERVER: &str = "https://git.example.org";
    let dir = temp("server");
    let fake = texcraft_engine::forge::fake::FakeGitLab::new(SERVER, "lab/paper", TOKEN, &[("main.tex", b"x\n")]);
    let git = published(&dir, "https://git.example.org/lab/paper.git", Some(("oauth2", TOKEN))).unwrap();
    let mut s = over(&git);
    s.set_forge_transport(move || fake.transport());
    let server = texcraft_engine::forge::servers::Server::new(SERVER, texcraft_engine::forge::Service::GitLab, "lab-app").unwrap();
    let prefs = Prefs { auto_compile: false, forge_servers: vec![server], ..Prefs::default() };
    let mut h = window_with(s, &dir, WIDE, prefs);
    open(&mut h, None);
    let card = h.get_by_role_and_label(Role::Button, "GitLab Server, git.example.org");
    card.click();
    h.run();
    assert!(h.query_by_label("Import from a GitLab Server").is_some());
    let chosen = h.get_by_role_and_label(Role::ComboBox, "Server");
    assert!(chosen.accesskit_node().value().unwrap_or_default().starts_with("git.example.org"), "the server added is not chosen");
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    type_into(&mut h, "Personal access token", TOKEN);
    // A path alone is on the server chosen.
    type_into(&mut h, "Project", "lab/paper");
    crate::forge::quiet(&mut h);
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::Button, "Import").click();
    settle(&mut h);
    assert!(said(&h, "Imported paper (main) into"), "not imported");
    let remotes = h.state_mut().control("engine.execute", &json!({"command": "git.remotes", "params": {}}))["result"].clone();
    assert_eq!(remotes["url"], "https://git.example.org/lab/paper.git", "{remotes}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_import_finished_after_another_project_was_opened_joins_the_recent_projects() {
    let dir = temp("elsewhere");
    let server = published(&dir, NOTES, None).unwrap();
    let other = dir.join("other");
    let _ = std::fs::create_dir_all(&other);
    let _ = std::fs::write(other.join("main.tex"), "\\documentclass{article}\n");
    let mut h = window(session(move || Box::new(Slow(server.clone()))), &dir, WIDE);
    open(&mut h, Some("git"));
    type_into(&mut h, "Address", NOTES);
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Import").click();
    // The import's card shows once its thread has started (the server is slow, so it is still busy).
    for _ in 0..400 {
        h.step();
        if h.query_by_role_and_label(Role::Group, "notes, Importing…").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(h.query_by_role_and_label(Role::Group, "notes, Importing…").is_some(), "no card for the import");
    // Meanwhile the user opens another project: the import does not take its place.
    h.state_mut().open_project(&other.display().to_string());
    settle(&mut h);
    assert_eq!(h.state().session().dir(), Some(&other), "the import replaced the project opened meanwhile");
    assert!(dir.join("notes/.git").is_dir(), "not cloned");
    assert!(said(&h, "it is in the recent projects"), "the status line does not say where it went");
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    assert!(prefs.recent.iter().any(|p| std::path::Path::new(p) == dir.join("notes")), "not among the recent projects: {:?}", prefs.recent);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn in_the_browser_an_import_has_its_card_and_cancel_stops_it() {
    let fake = texcraft_engine::forge::fake::Fake::github("ada/paper", "github_pat_card", &[("main.tex", b"x\n")]);
    let open = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut h = crate::forge_pick::gated(&fake, &open, true);
    ui_cmd::run_with(h.state_mut(), "project.import_dialog", &json!({"source": "github", "address": "https://github.com/ada/paper"}));
    h.run();
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    h.get_by_label("Personal access token").click();
    h.step();
    h.get_by_label("Personal access token").type_text("github_pat_card");
    for _ in 0..5 {
        h.step();
    }
    h.get_by_role_and_label(Role::Button, "Import").click();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_role_and_label(Role::Group, "paper, Importing…").is_some(), "no card for the import");
    assert!(said(&h, "Receiving the files…"), "the card does not say what it is doing");
    h.get_by_role_and_label(Role::Button, "Cancel: paper").click();
    for _ in 0..4 {
        h.step();
    }
    assert!(said(&h, "Import stopped."), "stopping is not said");
    assert!(h.query_by_role_and_label(Role::Group, "paper, Importing…").is_none(), "the card stayed");
    open.store(true, std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(100));
    for _ in 0..10 {
        h.step();
    }
    assert!(h.state().session().forge().is_none(), "the project stopped was opened all the same");
}

#[test]
fn a_github_token_remembered_serves_the_next_import_and_is_listed_to_replace_and_forget() {
    const TOKEN: &str = "github_pat_remember_test";
    let dir = temp("remember");
    let fake = texcraft_engine::forge::fake::Fake::github("ada/paper", TOKEN, &[("main.tex", b"x\n")]);
    let server = published(&dir, "https://github.com/ada/paper.git", Some(("x-access-token", TOKEN))).unwrap();
    let mut s = over(&server);
    s.set_forge_transport(move || fake.transport());
    let mut h = window(s, &dir, WIDE);
    open(&mut h, Some("github"));
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    type_into(&mut h, "Personal access token", TOKEN);
    let remember = h.get_by_role_and_label(Role::CheckBox, "Remember on this computer");
    assert_eq!(remember.accesskit_node().data().toggled(), Some(egui::accesskit::Toggled::True), "Remember is not ticked by default");
    type_into(&mut h, "Project", "https://github.com/ada/paper");
    crate::forge::quiet(&mut h);
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::Button, "Import").click();
    settle(&mut h);
    assert!(said(&h, "Imported paper (main) into"), "not imported");

    // The next import from GitHub asks for no token: the one kept is used.
    open(&mut h, Some("github"));
    assert!(said(&h, "A token for github.com is kept on this computer"), "the page does not say a token is kept");
    type_into(&mut h, "Project", "pap");
    crate::forge::quiet(&mut h);
    assert!(h.query_all_by_role(Role::ListBoxOption).count() > 0, "the suggestions need the token again");
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();

    // Settings › Accounts lists it by service and host, with Replace… and Forget.
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("Accounts").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Button, "Forget: github.com").is_some(), "the token kept is not listed");
    h.get_by_role_and_label(Role::Button, "Replace…: github.com").click();
    h.run();
    type_into(&mut h, "New token for github.com", "github_pat_new_one");
    h.get_by_role_and_label(Role::Button, "Keep").click();
    h.run();
    assert!(said(&h, "Replaced the token kept for github.com."));
    h.get_by_role_and_label(Role::Button, "Forget: github.com").click();
    h.run();
    assert!(said(&h, "Forgot the token kept for github.com."));
    assert!(h.query_by_role_and_label(Role::Button, "Forget: github.com").is_none(), "still listed once forgotten");
    // The preferences have the host at most, never a token.
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    let text = format!("{prefs:?}");
    assert!(!text.contains(TOKEN) && !text.contains("github_pat_new_one"), "a token in the preferences");
    assert!(prefs.git_hosts.is_empty(), "the host is still in the preferences");
    let _ = std::fs::remove_dir_all(&dir);
}
