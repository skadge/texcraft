//! New Project's "Where to Keep It" (docs/design/new-project.md) through the window: the places
//! listed (this computer, then the forges connected, the default chosen), a new project made on a
//! fake GitLab and GitHub (the repository made, the template committed and pushed, the project
//! opened), a name taken said, a push that fails after the repository was made keeping the
//! project, a connection added from the step, and the keyboard through both steps. The forge is
//! the fake in memory, the git server too; tokens are made-up strings, kept in the session's
//! memory: nothing reaches the network or the system's keychain.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::Service;
use texcraft_engine::forge::fake::Fake;
use texcraft_engine::gitwire::fake::Server;
use texcraft_ui_egui::{App, ui_cmd};

const TOKEN: &str = "glpat-fake-ui-new-project";
const WIDE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// A scratch folder: the documents folder, the template store and the git server's repository.
fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-ui-{}-new-project-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["Documents", "server.git/objects/pack", "server.git/refs/heads"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    std::fs::write(dir.join("server.git/HEAD"), "ref: refs/heads/main\n")?;
    std::fs::write(dir.join("server.git/config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n")?;
    Ok(dir)
}

/// The window over `fake` and the git server, its documents folder in `dir`.
fn harness(dir: &Path, fake: &Fake, server: &Server, size: egui::Vec2) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(dir.join("store"));
    let (fake, server) = (fake.clone(), server.clone());
    s.set_forge_transport(move || fake.transport());
    s.set_git_http(move || Box::new(server.clone()));
    let mut app = crate::support::app(s);
    app.set_documents_dir(Some(dir.join("Documents")));
    let mut h = Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

fn engine(h: &mut Harness<'_, App>, command: &str, params: serde_json::Value) -> serde_json::Value {
    h.state_mut().control("engine.execute", &json!({"command": command, "params": params}))
}

/// Connects `service`'s public forge with the token (Settings › Accounts' token connection).
fn connect(h: &mut Harness<'_, App>, service: Service) {
    let done = engine(h, "accounts.add", json!({"kind": service.id(), "token": TOKEN}));
    assert!(done.get("error").is_none(), "{done}");
    h.run();
}

fn focused(h: &Harness<'_, App>) -> String {
    h.query_all_by(|n| n.is_focused()).last().and_then(|n| n.accesskit_node().label()).unwrap_or_default()
}

fn key(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    h.run();
    h.run();
}

fn said(h: &Harness<'_, App>, text: &str) -> bool {
    h.query_all_by_label_contains(text).count() > 0
}

fn selected(h: &Harness<'_, App>, label: &str) -> bool {
    h.get_by_role_and_label(Role::RadioButton, label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

/// The gallery, the Article template chosen, then its second step.
fn where_step(h: &mut Harness<'_, App>) {
    ui_cmd::run(h.state_mut(), "project.from_template");
    h.run();
    h.get_by_label("Article").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Create Project…").click();
    h.run();
    h.run();
    assert!(h.query_by_label("Where to Keep It").is_some(), "Create Project… did not go on to the second step");
}

/// Runs frames until the name's check, the owners' list or the new project has ended.
fn settle(h: &mut Harness<'_, App>) -> bool {
    for _ in 0..600 {
        h.step();
        if h.state().session().git_busy().is_none() && h.state().session().forge_busy().is_none() {
            // The step's own half-second pause before it asks again.
            for _ in 0..40 {
                h.step();
            }
            if h.state().session().git_busy().is_none() {
                h.run();
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn the_step_lists_this_computer_and_the_forges_connected_the_default_chosen() {
    let dir = temp("list").unwrap();
    let fake = Fake::account(Service::GitLab, "https://gitlab.com", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git"));
    let mut h = harness(&dir, &fake, &server, WIDE);
    connect(&mut h, Service::GitLab);
    engine(&mut h, "accounts.set_default", json!({"instance": "https://gitlab.com"}));
    where_step(&mut h);
    assert!(h.query_by_role_and_label(Role::RadioButton, "This Computer, A folder of your choice").is_some(), "no card for this computer");
    assert!(selected(&h, "GitLab, @ada · Default"), "the default connection is not chosen");
    assert!(!selected(&h, "This Computer, A folder of your choice"));
    assert!(h.query_by_role_and_label(Role::Button, "Add a Connection…, GitLab, GitHub or your server").is_some());
    assert!(said(&h, "To start on Overleaf, create the project there, then Import it."), "Overleaf's way is not said");
    assert!(h.query_by_label("GitHub").is_none(), "a forge not connected has a card");
    // The repository's name comes from the template's, and is checked on the forge.
    assert!(settle(&mut h));
    assert_eq!(h.get_by_label("Repository").value().as_deref(), Some("article"));
    assert!(said(&h, "ada/article is free."), "the name's check is not said");
    assert!(selected(&h, "Private: Who can see it"), "private is not the default");
    assert!(h.query_by_label("Internal: Who can see it").is_some(), "GitLab's internal is not offered");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn on_gitlab_the_repository_is_made_the_template_pushed_and_the_project_opened() {
    let dir = temp("gitlab").unwrap();
    let fake = Fake::account(Service::GitLab, "https://gitlab.com", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git")).with_auth("oauth2", TOKEN);
    let mut h = harness(&dir, &fake, &server, WIDE);
    connect(&mut h, Service::GitLab);
    where_step(&mut h);
    // No default: the last place, else this computer; the forge is chosen by its card.
    assert!(selected(&h, "This Computer, A folder of your choice"));
    h.get_by_role_and_label(Role::RadioButton, "GitLab, @ada").click();
    h.run();
    assert!(settle(&mut h));
    let folder = dir.join("Documents/article");
    assert_eq!(h.get_by_label("Folder").value().as_deref(), Some(folder.display().to_string().as_str()), "the folder does not follow the name");
    h.get_by_role_and_label(Role::Button, "Create Project").click();
    h.run();
    assert!(settle(&mut h), "the new project did not end");
    assert!(said(&h, "Created ada/article on GitLab."), "the status does not say where the project is");
    assert!(h.query_by_label("Where to Keep It").is_none(), "the dialog stayed");
    assert_eq!(h.state().session().dir(), Some(&folder), "the project is not open in its folder");
    assert!(server.get("refs/heads/main").is_some(), "the server did not receive the commit");
    assert_eq!(fake.created().len(), 1);
    let remotes = engine(&mut h, "git.remotes", json!({}));
    assert_eq!(remotes["result"]["upstream"], json!({"remote": "origin", "branch": "main"}), "Sync would not work at once: {remotes}");
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    assert_eq!(prefs.new_project_place.as_deref(), Some("https://gitlab.com"), "the place chosen is not remembered");
    assert!(!format!("{prefs:?}").contains(TOKEN));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn on_github_a_name_taken_is_said_and_the_step_stays() {
    let dir = temp("taken").unwrap();
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    fake.take("ada/article");
    let server = Server::new(&dir.join("server.git")).with_auth("x-access-token", TOKEN);
    let mut h = harness(&dir, &fake, &server, WIDE);
    connect(&mut h, Service::GitHub);
    where_step(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "GitHub, @ada").click();
    h.run();
    assert!(settle(&mut h));
    assert!(said(&h, "ada/article exists already: choose another name."), "the name taken is not said");
    assert!(h.query_by_label("Internal: Who can see it").is_none(), "GitHub has no internal here");
    assert!(h.get_by_role_and_label(Role::Button, "Create Project").accesskit_node().is_disabled(), "Create Project is on over a name taken");
    // Another name is free, and goes.
    h.get_by_label("Repository").click();
    h.run();
    h.get_by_label("Repository").type_text("-2");
    h.run();
    assert!(settle(&mut h));
    assert!(said(&h, "ada/article-2 is free."));
    h.get_by_role_and_label(Role::Button, "Create Project").click();
    h.run();
    assert!(settle(&mut h));
    assert!(said(&h, "Created ada/article-2 on GitHub."));
    assert!(fake.files("main").is_empty(), "GitHub's API was used for files on the desktop: {:?}", fake.files("main").keys());
    assert!(server.get("refs/heads/main").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_push_that_fails_keeps_the_project_and_offers_try_again() {
    let dir = temp("push").unwrap();
    let fake = Fake::account(Service::GitLab, "https://gitlab.com", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git")).with_auth("oauth2", TOKEN);
    server.refuse("the disk is full");
    let mut h = harness(&dir, &fake, &server, WIDE);
    connect(&mut h, Service::GitLab);
    engine(&mut h, "accounts.set_default", json!({"instance": "https://gitlab.com"}));
    where_step(&mut h);
    assert!(settle(&mut h));
    h.get_by_role_and_label(Role::Button, "Create Project").click();
    h.run();
    assert!(settle(&mut h));
    assert!(said(&h, "but sending it did not work"), "the failed push is not said");
    assert!(h.query_by_role_and_label(Role::Button, "Try Again").is_some(), "no way to send it again");
    let folder = dir.join("Documents/article");
    assert_eq!(h.state().session().dir(), Some(&folder), "the project was not kept");
    assert_eq!(fake.created().len(), 1, "the repository is not kept");
    h.get_by_role_and_label(Role::Button, "Send Later").click();
    h.run();
    assert!(h.query_by_label("Where to Keep It").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_connection_added_from_the_step_gets_its_card_chosen() {
    let dir = temp("add").unwrap();
    let fake = Fake::account(Service::GitLab, "https://gitlab.com", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git"));
    let mut h = harness(&dir, &fake, &server, WIDE);
    where_step(&mut h);
    h.get_by_role_and_label(Role::Button, "Add a Connection…, GitLab, GitHub or your server").click();
    h.run();
    h.get_by_label("GitLab, Sign in or use a token").click();
    h.run();
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    h.get_by_label("Personal access token").click();
    h.run();
    h.get_by_label("Personal access token").type_text(TOKEN);
    h.run();
    h.get_by_role_and_label(Role::Button, "Connect").click();
    h.run();
    for _ in 0..300 {
        h.step();
        if !h.state().session().forge_auth_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
    h.run();
    assert!(h.query_by_label("Where to Keep It").is_some(), "the step did not come back");
    assert!(selected(&h, "GitLab, @ada"), "the new connection's card is not chosen");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_steps_are_crossed_by_keyboard() {
    let dir = temp("keys").unwrap();
    let fake = Fake::account(Service::GitLab, "https://gitlab.com", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git")).with_auth("oauth2", TOKEN);
    let mut h = harness(&dir, &fake, &server, WIDE);
    connect(&mut h, Service::GitLab);
    ui_cmd::run(h.state_mut(), "project.from_template");
    h.run();
    h.run();
    // Tab to the Article card, Enter chooses it; Tab to Create Project…, Enter goes on.
    let mut found = false;
    for _ in 0..80 {
        key(&mut h, egui::Key::Tab);
        if focused(&h) == "Article" {
            found = true;
            break;
        }
    }
    assert!(found, "Tab does not reach the Article card");
    key(&mut h, egui::Key::Enter);
    let mut found = false;
    for _ in 0..80 {
        key(&mut h, egui::Key::Tab);
        if focused(&h) == "Create Project…" {
            found = true;
            break;
        }
    }
    assert!(found, "Tab does not reach Create Project…");
    key(&mut h, egui::Key::Enter);
    assert!(h.query_by_label("Where to Keep It").is_some(), "Enter did not go on");
    assert_eq!(focused(&h), "This Computer, A folder of your choice", "the chosen card does not have the keyboard");
    // The arrows move between the cards; Enter chooses.
    key(&mut h, egui::Key::ArrowRight);
    assert_eq!(focused(&h), "GitLab, @ada");
    key(&mut h, egui::Key::Enter);
    assert!(selected(&h, "GitLab, @ada"));
    // Back to the templates, with Create Project… on the keyboard; and on again.
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    h.run();
    h.run();
    assert!(h.query_by_label("Where to Keep It").is_none(), "Alt+Left did not go back");
    assert_eq!(focused(&h), "Create Project…", "Back did not give the keyboard to Create Project…");
    key(&mut h, egui::Key::Enter);
    assert!(selected(&h, "GitLab, @ada"), "the place chosen was not kept over Back");
    assert!(settle(&mut h));
    // Tab through the fields to Create Project, Enter makes it.
    let mut found = false;
    for _ in 0..40 {
        key(&mut h, egui::Key::Tab);
        if focused(&h) == "Create Project" {
            found = true;
            break;
        }
    }
    assert!(found, "Tab does not reach Create Project");
    key(&mut h, egui::Key::Enter);
    assert!(settle(&mut h));
    assert!(said(&h, "Created ada/article on GitLab."), "Enter on Create Project did not make it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn on_a_phone_the_places_are_a_list() {
    let dir = temp("phone").unwrap();
    let fake = Fake::account(Service::GitLab, "https://gitlab.com", "ada", TOKEN);
    let server = Server::new(&dir.join("server.git"));
    let mut h = harness(&dir, &fake, &server, egui::vec2(390.0, 800.0));
    connect(&mut h, Service::GitLab);
    ui_cmd::run(h.state_mut(), "project.from_template");
    h.run();
    h.get_by_label("Article").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Create Project…").click();
    h.run();
    h.run();
    let here = h.get_by_role_and_label(Role::RadioButton, "This Computer, A folder of your choice").rect();
    let gitlab = h.get_by_role_and_label(Role::RadioButton, "GitLab, @ada").rect();
    assert!(gitlab.top() > here.bottom() - 1.0 && here.width() > 300.0, "not a list on a phone");
    let _ = std::fs::remove_dir_all(&dir);
}
