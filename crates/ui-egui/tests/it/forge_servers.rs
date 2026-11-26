//! Adding a self-managed server through the interface (docs/forges.md, "Self-managed servers"):
//! Settings › Accounts › Add a GitLab Server…, its check, what to register, the application's id,
//! then signing in to it; a link's proposal, which asks before anything is added; and the Open
//! dialog's offer for an unknown host. No test reaches the network.

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

const SERVER: &str = "https://git.example.org";
const ACCESS: &str = "server-ui-oauth-access";

/// The probe, OAuth's endpoints and `/user` to the script, the project's API to the fake.
struct Router {
    script: Script,
    fake: FakeGitLab,
}

impl Transport for Router {
    fn send(&self, request: Request) -> Sending<'_> {
        let u = &request.url;
        if u.contains("/oauth/") || u.ends_with("/user") || u.ends_with("/version") {
            self.script.send(request)
        } else {
            Box::pin(async move { self.fake.transport().send(request).await })
        }
    }
}

fn harness(script: &Script) -> Harness<'static, App> {
    let fake = FakeGitLab::new(SERVER, "lab/paper", ACCESS, &[("main.tex", b"\\documentclass{article}\n")]);
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let script = script.clone();
    s.set_forge_transport(move || Box::new(Router { script: script.clone(), fake: fake.clone() }));
    s.set_forge_clients(Clients { gitlab: "gl-client".into(), github: "gh-client".into() });
    s.set_forge_loopback("127.0.0.1:0");
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

fn type_into(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
}

/// Steps until the session's sign-ins and checks have ended.
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..500 {
        h.step();
        if !h.state().session().forge_auth_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.run();
}

#[test]
fn a_gitlab_server_is_checked_registered_added_and_signed_in_to_from_settings() {
    let script = Script::default();
    script.answer(401, json!({"message": "401 Unauthorized"}));
    let mut h = harness(&script);
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("Accounts").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Add a GitLab Server…").click();
    h.run();

    type_into(&mut h, "Server address", "git.example.org");
    h.get_by_role_and_label(Role::Button, "Check Server").click();
    settle(&mut h);
    assert!(h.query_by_label_contains("git.example.org is a GitLab server").is_some(), "the check's outcome is not said");
    // What to register, each value with a Copy button named after it.
    for name in ["Copy: Name", "Copy: Redirect URI", "Copy: Scopes"] {
        assert!(h.query_by_role_and_label(Role::Button, name).is_some(), "no {name}");
    }
    assert!(h.query_by_label_contains("http://127.0.0.1:47321/callback").is_some(), "the desktop's redirect URI is not shown");
    assert!(h.get_by_role_and_label(Role::Button, "Add Server").accesskit_node().is_disabled(), "no id yet");
    type_into(&mut h, "Application ID", "server-app");
    h.get_by_role_and_label(Role::Button, "Add Server").click();
    h.run();

    // Back in Settings › Accounts, with the server and its own Sign In button.
    assert!(h.query_by_label("git.example.org (GitLab)").is_some(), "the server is not listed");
    let Some(prefs) = h.state_mut().take_changed_prefs() else { panic!("the preferences did not change") };
    assert_eq!(prefs.forge_servers.len(), 1);
    assert_eq!((prefs.forge_servers[0].instance.as_str(), prefs.forge_servers[0].client_id.as_str()), (SERVER, "server-app"));

    script.answer(200, json!({"access_token": ACCESS, "refresh_token": "r", "expires_in": 7200, "scope": "read_api read_user"}));
    script.answer(200, json!({"name": "Ada Lovelace", "username": "ada"}));
    let buttons: Vec<_> = h.query_all_by_role_and_label(Role::Button, "Sign In with GitLab").collect();
    assert_eq!(buttons.len(), 2, "gitlab.com's and the server's");
    buttons[1].click();
    h.run();
    let Some((_, SignInProgress::Browser { url })) = h.state().session().forge_signing_in() else { panic!("not waiting for the browser") };
    assert!(url.starts_with("https://git.example.org/oauth/authorize?"), "{url}");
    let q = oauth::parse_query(url.split_once('?').map(|(_, q)| q).unwrap_or_default());
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    assert_eq!(get("client_id"), "server-app");
    let addr = get("redirect_uri").trim_start_matches("http://").trim_end_matches("/callback").to_owned();
    if let Ok(mut stream) = std::net::TcpStream::connect(addr) {
        let _ = write!(stream, "GET /callback?code=c&state={} HTTP/1.1\r\n\r\n", get("state"));
        let _ = stream.read_to_string(&mut String::new());
    }
    settle(&mut h);
    assert!(h.state().session().forge_signed_in_at(SERVER).is_some(), "not signed in to the server");
    // The preferences never hold the sign-in.
    let prefs = h.state_mut().take_changed_prefs().unwrap_or(prefs);
    let kept = serde_json::to_string(&prefs).unwrap_or_default();
    assert!(kept.contains("server-app") && !kept.contains(ACCESS), "{kept}");
}

#[test]
fn a_server_that_does_not_answer_as_gitlab_is_said_and_the_address_kept() {
    let script = Script::default();
    script.answer(200, json!("<html>wiki</html>"));
    let mut h = harness(&script);
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("Accounts").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Add a GitLab Server…").click();
    h.run();
    type_into(&mut h, "Server address", "wiki.example.org");
    h.get_by_role_and_label(Role::Button, "Check Server").click();
    settle(&mut h);
    assert!(h.query_by_label_contains("does not answer as a GitLab server").is_some(), "the refusal is not said");
    assert!(h.query_by_label("Application ID").is_none(), "nothing to register on it");
    assert!(h.state().session().forge_servers().is_empty());
}

#[test]
fn a_link_s_server_is_added_only_when_the_user_says_so() {
    let script = Script::default();
    let mut h = harness(&script);
    let link = "gitlab=https%3A%2F%2Fgit.example.org%2Flab%2Fpaper&client_id=link-app";
    assert!(h.state_mut().forge_propose_server(link));
    h.run();
    assert!(h.query_by_label_contains("This link proposes to add the GitLab server git.example.org").is_some(), "the link is not asked about");
    assert!(h.state().session().forge_servers().is_empty(), "a link cannot add a server by itself");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    h.run();
    assert!(h.state().session().forge_servers().is_empty(), "cancelled, nothing added");

    assert!(h.state_mut().forge_propose_server(link));
    h.run();
    h.get_by_role_and_label(Role::Button, "Add Server").click();
    h.run();
    assert_eq!(h.state().session().forge_client_id(SERVER, texcraft_engine::forge::Service::GitLab).as_deref(), Some("link-app"));
    // Then the link's project, on Import a Project's GitLab Server page, with the server's
    // sign-in offered.
    assert!(h.query_by_label("Import from a GitLab Server").is_some(), "the link's project is not offered");
    assert!(h.query_by_role_and_label(Role::Button, "Sign In with GitLab").is_some_and(|b| !b.accesskit_node().is_disabled()));
    assert!(!h.state_mut().forge_propose_server("code=1&state=2"), "a query that proposes nothing");
}

#[test]
fn the_gitlab_page_offers_to_add_an_unknown_server_and_comes_back_to_the_project() {
    let script = Script::default();
    let mut h = harness(&script);
    ui_cmd::run(h.state_mut(), "forge.open");
    h.run();
    type_into(&mut h, "Project", "https://git.example.org/lab/paper");
    assert!(h.query_by_label("Personal access token").is_some(), "an unknown server needs the token until it is added");
    h.get_by_role_and_label(Role::Button, "Add git.example.org for Signing In…").click();
    h.run();
    assert!(h.query_by_label("Add a GitLab Server").is_some(), "the Add a Server dialog did not open");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    h.run();
    assert!(h.query_by_label("Import from GitLab").is_some(), "the GitLab page is not back");
    assert!(h.query_by_role_and_label(Role::Button, "Add git.example.org for Signing In…").is_some(), "the address typed is not kept");
}
