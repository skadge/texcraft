//! Save Project to GitHub… through the window's dialog, against the fake forge and the git
//! server in memory (no network): the repository made, the history pushed, its page offered; a
//! name taken said in the dialog, which stays open.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::Service;
use texcraft_engine::forge::fake::Fake;
use texcraft_engine::gitwire::fake::Server;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

const TOKEN: &str = "ghp-ui-save-test";

/// A scratch folder with a bare repository in it, `server.git`.
fn temp(name: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-ui-{}-save-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("server.git/objects/pack"))?;
    std::fs::create_dir_all(dir.join("server.git/refs/heads"))?;
    std::fs::write(dir.join("server.git/HEAD"), "ref: refs/heads/main\n")?;
    std::fs::write(dir.join("server.git/config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n")?;
    Ok(dir)
}

/// A window with a project saved in `dir/My Paper`, the fake GitHub account and the git server.
fn harness(dir: &std::path::Path, fake: &Fake, server: &Server) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let (fake, server) = (fake.clone(), server.clone());
    s.set_forge_transport(move || fake.transport());
    s.set_git_http(move || Box::new(server.clone()));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n"}))?;
    s.execute("project.save_as", &json!({"path": dir.join("My Paper").display().to_string()}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    Ok(h)
}

/// Runs frames until the operation has ended.
fn settle(h: &mut Harness<'_, App>) -> bool {
    for _ in 0..500 {
        h.step();
        if h.state().session().git_busy().is_none() {
            h.run();
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// Opens the dialog and gives it the token (signing in is the dialog's first offer when this
/// build can).
fn open_with_token(h: &mut Harness<'_, App>) {
    ui_cmd::run(h.state_mut(), "forge.save_to_github");
    h.run();
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    h.get_by_label("Personal access token").click();
    h.run();
    h.get_by_label("Personal access token").type_text(TOKEN);
    h.run();
    settle(h);
}

#[test]
fn saving_to_github_through_the_dialog_pushes_the_history_and_offers_the_page() {
    let dir = temp("main").unwrap();
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git")).with_auth("x-access-token", TOKEN);
    let mut h = harness(&dir, &fake, &server).unwrap();
    open_with_token(&mut h);
    // The name comes from the folder's, made one a repository can have; private by default.
    let name = h.get_by_label("Name");
    assert_eq!(name.value().as_deref(), Some("My-Paper"));
    assert_eq!(
        h.get_by_label("Private: Who can see it").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "private is not the default"
    );
    h.get_by_role_and_label(Role::Button, "Save to GitHub").click();
    // One frame: the save runs in the background, and its spinner repaints until `settle` sees it end.
    h.step();
    assert!(settle(&mut h), "the save did not end");
    assert!(h.query_by_label_contains("The project is in ada/My-Paper").is_some(), "the dialog does not say where the project is");
    assert!(h.query_by_role_and_label(Role::Button, "Open in Browser").is_some());
    assert!(server.get("refs/heads/main").is_some(), "the history was not pushed");
    assert_eq!(fake.created()[0]["private"], true);
    h.get_by_role_and_label(Role::Button, "Close").click();
    h.run();
    // The History panel's remote, with Push and Pull.
    let remotes = h.state_mut().control("engine.execute", &json!({"command": "git.remotes", "params": {}}));
    assert_eq!(remotes["result"]["url"], "https://github.com/ada/My-Paper.git", "{remotes}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_taken_is_said_in_the_dialog_which_stays_open() {
    let dir = temp("taken").unwrap();
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    fake.take("ada/My-Paper");
    let server = Server::new(&dir.join("server.git"));
    let mut h = harness(&dir, &fake, &server).unwrap();
    open_with_token(&mut h);
    h.get_by_role_and_label(Role::Button, "Save to GitHub").click();
    // One frame: the save runs in the background, and its spinner repaints until `settle` sees it end.
    h.step();
    assert!(settle(&mut h), "the save did not end");
    assert!(h.query_by_label_contains("choose another name").is_some(), "the dialog does not say the name is taken");
    assert!(h.query_by_label("Name").is_some(), "the dialog closed");
    // Another name goes.
    h.get_by_label("Name").click();
    h.run();
    h.get_by_label("Name").type_text("-2");
    h.run();
    h.get_by_role_and_label(Role::Button, "Save to GitHub").click();
    // One frame: the save runs in the background, and its spinner repaints until `settle` sees it end.
    h.step();
    assert!(settle(&mut h), "the save did not end");
    assert!(h.query_by_label_contains("The project is in ada/My-Paper-2").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
