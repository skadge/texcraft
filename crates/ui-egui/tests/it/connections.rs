//! Settings › Accounts' connections (docs/design/connections.md) by keyboard: the empty page
//! and Add a Connection…, its cards crossed with the arrows and Escape giving the keyboard back,
//! a git server connected with a token, a GitHub token checked and set as the default, which
//! Import a Project then opens on, and Disconnect asking first. Tokens are made-up strings; the
//! forge is a scripted transport; nothing reaches the network or the system's keychain.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::fake::Script;
use texcraft_ui_egui::{App, ui_cmd};

const GITHUB_TOKEN: &str = "ghp_fake_ui_connection";

fn harness(script: &Script, size: egui::Vec2) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let script = script.clone();
    s.set_forge_transport(move || script.transport());
    let app = crate::support::app(s);
    let mut h = Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

const WIDE: egui::Vec2 = egui::vec2(1400.0, 900.0);

fn accounts(h: &mut Harness<'_, App>) {
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("Accounts").click();
    h.run();
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

fn type_into(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
}

fn said(h: &Harness<'_, App>, text: &str) -> bool {
    h.query_all_by_label_contains(text).count() > 0
}

/// Steps until the token check has ended.
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..300 {
        h.step();
        if !h.state().session().forge_auth_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
    h.run();
}

#[test]
fn add_a_connection_is_reached_and_left_by_keyboard() {
    let mut h = harness(&Script::default(), WIDE);
    accounts(&mut h);
    assert!(said(&h, "Connect TeXCraft to where your projects live"), "the empty page does not say what connections are for");
    let add = h.get_by_role_and_label(Role::Button, "Add a Connection…");
    add.focus();
    h.run();
    key(&mut h, egui::Key::Enter);
    assert!(h.query_by_label("Add a Connection").is_some(), "Enter did not open the dialog");
    for name in ["GitLab and GitHub", "Other git servers"] {
        assert!(h.query_by_role_and_label(Role::Group, name).is_some(), "no group {name}");
    }
    // The first card has the keyboard; the arrows move it.
    assert_eq!(focused(&h), "GitLab, Sign in or use a token");
    key(&mut h, egui::Key::ArrowRight);
    assert_eq!(focused(&h), "GitHub, Sign in or use a token");
    key(&mut h, egui::Key::End);
    assert_eq!(focused(&h), "Other Git Server, Any HTTPS host and a token");
    // Enter opens its page; Back returns to the card; Escape gives the keyboard back to the page.
    key(&mut h, egui::Key::Enter);
    assert!(h.query_by_label("Connect to a Git Server").is_some(), "Enter did not open the page");
    assert_eq!(focused(&h), "Server", "the first field does not have the keyboard");
    h.get_by_role_and_label(Role::Button, "Back to connections").click();
    h.run();
    h.run();
    assert_eq!(focused(&h), "Other Git Server, Any HTTPS host and a token", "Back did not give the keyboard to the card");
    key(&mut h, egui::Key::Escape);
    assert!(h.query_by_label("Add a Connection").is_none(), "Escape did not close the dialog");
    assert_eq!(focused(&h), "Add a Connection…", "the keyboard did not come back to Add a Connection…");
}

#[test]
fn a_git_server_is_connected_with_a_token_and_disconnected_after_asking() {
    let mut h = harness(&Script::default(), WIDE);
    ui_cmd::run_with(h.state_mut(), "app.add_connection", &json!({"kind": "git"}));
    h.run();
    type_into(&mut h, "Server", "git.example.net");
    type_into(&mut h, "Git token", "fake-git-token");
    h.get_by_role_and_label(Role::Button, "Connect").click();
    h.run();
    h.run();
    assert!(said(&h, "Connected to git.example.net."), "not said");
    assert!(h.query_by_role_and_label(Role::Group, "Other git servers").is_some(), "the row is not in its group");
    assert_eq!(focused(&h), "Disconnect…: git.example.net", "the keyboard is not on the new row");
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    assert_eq!(prefs.git_hosts, ["git.example.net"]);
    assert!(!format!("{prefs:?}").contains("fake-git-token"), "a token in the preferences");

    // Disconnect asks first; Escape keeps the connection.
    key(&mut h, egui::Key::Enter);
    assert!(h.query_by_label("Disconnect from git.example.net?").is_some(), "Disconnect did not ask");
    assert!(said(&h, "Projects linked to it stay as they are"), "what happens to its projects is not said");
    key(&mut h, egui::Key::Escape);
    assert!(h.query_by_role_and_label(Role::Button, "Disconnect…: git.example.net").is_some(), "Escape disconnected");
    assert_eq!(focused(&h), "Disconnect…: git.example.net", "the keyboard did not come back to the row");
    h.get_by_role_and_label(Role::Button, "Disconnect…: git.example.net").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Disconnect").click();
    h.run();
    h.run();
    assert!(said(&h, "Disconnected from git.example.net."));
    assert!(said(&h, "Connect TeXCraft to where your projects live"), "the page is not empty again");
}

#[test]
fn a_github_token_is_checked_set_as_default_and_import_opens_on_it() {
    let script = Script::default();
    script.answer(200, json!({"login": "octo", "name": "Octo Cat"}));
    let mut h = harness(&script, WIDE);
    ui_cmd::run_with(h.state_mut(), "app.add_connection", &json!({"kind": "github"}));
    h.run();
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    type_into(&mut h, "Personal access token", GITHUB_TOKEN);
    h.get_by_role_and_label(Role::Button, "Connect").click();
    settle(&mut h);
    assert!(said(&h, "Connected to github.com as @octo."), "the check's outcome is not said");
    assert!(h.query_by_label("Octo Cat (@octo)").is_some(), "who the token is for is not in the row");
    assert!(h.query_by_label("Token").is_some(), "the state is not said in words");

    h.get_by_role_and_label(Role::Button, "Set as Default: github.com").click();
    h.run();
    h.run();
    assert!(said(&h, "github.com is the default connection."));
    assert!(h.query_by_role_and_label(Role::Button, "Set as Default: github.com").is_none(), "the default can be set again");
    let prefs = h.state_mut().take_changed_prefs().expect("the preferences changed");
    assert_eq!(prefs.default_connection.as_deref(), Some("https://github.com"));
    assert!(!format!("{prefs:?}").contains(GITHUB_TOKEN), "a token in the preferences");

    // Import a Project opens with the default's card focused, saying so.
    key(&mut h, egui::Key::Escape);
    ui_cmd::run_with(h.state_mut(), "project.import_dialog", &json!({}));
    h.run();
    h.run();
    assert!(focused(&h).starts_with("GitHub") && focused(&h).ends_with("Default"), "Import does not open on the default: {}", focused(&h));
}

#[test]
fn the_page_and_its_dialog_fit_a_phone() {
    let script = Script::default();
    let mut h = harness(&script, egui::vec2(390.0, 800.0));
    ui_cmd::run_with(h.state_mut(), "app.add_connection", &json!({}));
    h.run();
    h.run();
    // A list of rows, the whole width, in the same order.
    let gitlab = h.get_by_role_and_label(Role::Button, "GitLab, Sign in or use a token").rect();
    let github = h.get_by_role_and_label(Role::Button, "GitHub, Sign in or use a token").rect();
    assert!(github.top() > gitlab.bottom() - 1.0 && gitlab.width() > 300.0, "not a list on a phone");
}
