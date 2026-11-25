//! Opening a project from GitLab and committing to it through the window's dialogs, against the
//! fake GitLab (no network).

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::fake::FakeGitLab;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

const TOKEN: &str = "glpat-ui-test";

fn harness(fake: &FakeGitLab) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let fake = fake.clone();
    s.set_forge_transport(move || fake.transport());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

/// Types `text` into the field labelled `label`, focused first.
fn type_into(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
}

/// Runs frames until the forge operation has ended; false if it does not.
fn settle(h: &mut Harness<'_, App>) -> bool {
    for _ in 0..500 {
        h.run();
        if h.state().session().forge_busy().is_none() {
            h.run();
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn a_project_opened_from_gitlab_is_edited_and_committed_through_the_dialogs() {
    let fake =
        FakeGitLab::new("https://gitlab.example", "group/paper", TOKEN, &[("main.tex", b"\\documentclass{article}\n"), ("intro.tex", b"Intro\n")]);
    let mut h = harness(&fake);
    ui_cmd::run(h.state_mut(), "forge.open");
    h.run();
    type_into(&mut h, "Project address", "https://gitlab.example/group/paper");
    type_into(&mut h, "Personal access token", TOKEN);
    h.run();
    assert!(h.query_by_label_contains("never written to disk").is_some(), "the dialog does not say the token is not kept");
    h.get_by_role_and_label(Role::Button, "Open").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("Opened group/paper from GitLab").is_some(), "the status line does not say it opened");
    assert!(h.query_by_label_contains("group/paper · main · up to date").is_some(), "the top bar does not show the project and branch");

    let edit = json!({"command": "file.write", "params": {"path": "intro.tex", "text": "Better intro\n"}});
    assert_eq!(h.state_mut().control("engine.execute", &edit)["ok"], true);
    h.run();
    ui_cmd::run(h.state_mut(), "forge.commit");
    h.run();
    assert!(h.query_all_by_label_contains("1 changed file").count() >= 1, "the dialog does not count the changes");
    assert!(h.query_all_by_label("intro.tex").count() >= 2, "the dialog does not list the file beside the file tree");
    assert!(h.query_by_label("Personal access token").is_none(), "the token given this session was asked for again");
    type_into(&mut h, "What this commit changes", "Better intro");
    h.run();
    h.get_by_role_and_label(Role::Button, "Commit").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("Committed 1 file to main").is_some(), "the status line does not say it committed");
    assert_eq!(fake.files("main")["intro.tex"], b"Better intro\n");
}

#[test]
fn a_refused_token_is_said_on_the_status_line() {
    let fake = FakeGitLab::new("https://gitlab.example", "group/paper", TOKEN, &[("main.tex", b"x\n")]);
    let mut h = harness(&fake);
    ui_cmd::run(h.state_mut(), "forge.open");
    h.run();
    type_into(&mut h, "Project address", "gitlab.example/group/paper");
    type_into(&mut h, "Personal access token", "glpat-wrong");
    h.run();
    h.get_by_role_and_label(Role::Button, "Open").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("did not accept the access token").is_some());
    assert!(h.state().session().forge().is_none());
}
