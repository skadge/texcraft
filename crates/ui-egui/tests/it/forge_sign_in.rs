//! Signing in through the dialogs (docs/forges.md, step 4): the Sign In button first and the token
//! as the fallback, the button off with its reason in a build without an application, and a
//! GitLab sign-in on the desktop (the browser's visit to the loopback redirect made by the test)
//! that then opens a project with no token. No test reaches the network.

use std::io::{Read, Write};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::forge::fake::{FakeGitLab, Script};
use texcraft_engine::forge::oauth::{self, Clients};
use texcraft_engine::forge::{Request, Sending, Transport};
use texcraft_engine::{Session, SignInProgress};
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

const ACCESS: &str = "gl-ui-oauth-access";

/// OAuth's endpoints and `/user` to the script, the project's API to the fake.
struct Router {
    script: Script,
    fake: FakeGitLab,
}

impl Transport for Router {
    fn send(&self, request: Request) -> Sending<'_> {
        if request.url.contains("/oauth/") || request.url.ends_with("/user") {
            self.script.send(request)
        } else {
            Box::pin(async move { self.fake.transport().send(request).await })
        }
    }
}

fn harness(clients: Clients, script: &Script, fake: &FakeGitLab) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let (script, fake) = (script.clone(), fake.clone());
    s.set_forge_transport(move || Box::new(Router { script: script.clone(), fake: fake.clone() }));
    s.set_forge_clients(clients);
    s.set_forge_loopback("127.0.0.1:0");
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

fn clients() -> Clients {
    Clients { gitlab: "gl-client".into(), github: "gh-client".into() }
}

fn type_into(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
}

#[test]
fn the_open_dialog_offers_signing_in_first_and_the_token_second() {
    let (script, fake) = (Script::default(), FakeGitLab::new("https://gitlab.com", "group/paper", ACCESS, &[]));
    let mut h = harness(clients(), &script, &fake);
    ui_cmd::run(h.state_mut(), "forge.open");
    h.run();
    assert!(!h.get_by_role_and_label(Role::Button, "Sign In with GitLab").accesskit_node().is_disabled());
    assert!(h.query_by_label("Personal access token").is_none(), "the token is the fallback, not the first offer");
    h.get_by_label("Use a personal access token instead").click();
    h.run();
    assert!(h.query_by_label("Personal access token").is_some(), "the fallback shows the token field");
}

#[test]
fn without_an_application_the_button_is_off_and_says_why() {
    let (script, fake) = (Script::default(), FakeGitLab::new("https://gitlab.com", "group/paper", ACCESS, &[]));
    let placeholder = Clients { gitlab: "TEXCRAFT_GITLAB_CLIENT_ID_PENDING".into(), github: "TEXCRAFT_GITHUB_CLIENT_ID_PENDING".into() };
    let mut h = harness(placeholder, &script, &fake);
    ui_cmd::run(h.state_mut(), "forge.open");
    h.run();
    assert!(h.get_by_role_and_label(Role::Button, "Sign In with GitLab").accesskit_node().is_disabled());
    assert!(h.query_by_label_contains("This build has no GitLab application yet").is_some(), "the reason is not shown");
    assert!(h.query_by_label("Personal access token").is_some(), "the token is all that is left");
}

#[test]
fn signing_in_from_the_dialog_then_opens_with_no_token() {
    let (script, fake) =
        (Script::default(), FakeGitLab::new("https://gitlab.com", "group/paper", ACCESS, &[("main.tex", b"\\documentclass{article}\n")]));
    script.answer(200, json!({"access_token": ACCESS, "refresh_token": "r", "expires_in": 7200, "scope": "read_api read_user"}));
    script.answer(200, json!({"name": "Ada Lovelace", "username": "ada"}));
    let mut h = harness(clients(), &script, &fake);
    ui_cmd::run(h.state_mut(), "forge.open");
    h.run();
    h.get_by_role_and_label(Role::Button, "Sign In with GitLab").click();
    h.run();
    let Some((_, SignInProgress::Browser { url })) = h.state().session().forge_signing_in() else { panic!("not waiting for the browser") };
    assert!(h.query_by_role_and_label(Role::Button, "Cancel Signing In").is_some(), "the wait can be cancelled");

    // The browser comes back to the loopback redirect.
    let q = oauth::parse_query(url.split_once('?').map(|(_, q)| q).unwrap_or_default());
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let addr = get("redirect_uri").trim_start_matches("http://").trim_end_matches("/callback").to_owned();
    if let Ok(mut stream) = std::net::TcpStream::connect(addr) {
        let _ = write!(stream, "GET /callback?code=c&state={} HTTP/1.1\r\n\r\n", get("state"));
        let _ = stream.read_to_string(&mut String::new());
    }
    for _ in 0..500 {
        h.step();
        if !h.state().session().forge_auth_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.run();
    assert!(h.query_all_by_label_contains("Signed in to GitLab as Ada Lovelace (@ada)").count() >= 1, "who is signed in is not shown");

    type_into(&mut h, "Project address", "https://gitlab.com/group/paper");
    h.get_by_role_and_label(Role::Button, "Open").click();
    assert!(crate::forge::settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("Opened group/paper from GitLab").is_some(), "the project did not open with the sign-in");

    // Settings › Accounts: who, and Sign Out.
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("Accounts").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Button, "Sign Out").is_some());
}
