//! Opening a project from GitLab and committing to it through the window's dialogs, against the
//! fake GitLab (no network).

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
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
    // Short steps: the window checks the branch every two minutes of its time, which a test's
    // frames should not reach.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
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

/// Runs frames until the forge operation has ended; false if it does not. One frame at a time:
/// the window repaints while the operation runs.
fn settle(h: &mut Harness<'_, App>) -> bool {
    for _ in 0..500 {
        h.step();
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

const PAPER: &[u8] = b"\\documentclass{article}\n\\begin{document}\nOne.\n\nTwo.\n\nThree.\n\\end{document}\n";

/// A window with the fake's project opened from GitLab.
fn opened(fake: &FakeGitLab) -> Harness<'static, App> {
    let mut h = harness(fake);
    let open = json!({"command": "forge.open", "params": {"url": "https://gitlab.example/group/paper", "token": TOKEN}});
    assert_eq!(h.state_mut().control("engine.execute", &open)["ok"], true);
    h.run();
    h
}

fn write(h: &mut Harness<'_, App>, path: &str, text: &str) {
    let edit = json!({"command": "file.write", "params": {"path": path, "text": text}});
    assert_eq!(h.state_mut().control("engine.execute", &edit)["ok"], true);
    h.run();
}

fn paper() -> FakeGitLab {
    FakeGitLab::new("https://gitlab.example", "group/paper", TOKEN, &[("main.tex", PAPER), ("intro.tex", b"Intro\n")])
}

fn with(text: &[u8], from: &str, to: &str) -> String {
    String::from_utf8_lossy(text).replace(from, to)
}

#[test]
fn a_conflict_is_listed_in_the_commit_dialog_and_keeping_mine_commits_it() {
    let fake = paper();
    let mut h = opened(&fake);
    fake.push("main", &[("main.tex", Some(with(PAPER, "Two.", "Their two.").as_bytes()))]);
    write(&mut h, "main.tex", &with(PAPER, "Two.", "My two."));
    ui_cmd::run(h.state_mut(), "forge.commit");
    h.run();
    type_into(&mut h, "What this commit changes", "My two");
    h.get_by_role_and_label(Role::Button, "Commit").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    // Refused, said, and the dialog back with the file to decide and the message kept.
    assert!(h.query_by_label_contains("nothing was taken in or committed").is_some(), "the status line does not say why");
    assert!(h.query_by_label_contains("1 file in conflict with the branch").is_some(), "the dialog does not list the conflict");
    assert!(h.get_by_role_and_label(Role::Button, "Commit").accesskit_node().is_disabled(), "a commit is offered before a choice is made");
    h.get_by_label("Keep mine: main.tex").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Commit").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("Committed 1 file to main").is_some(), "the status line does not say it committed");
    assert_eq!(fake.files("main")["main.tex"], with(PAPER, "Two.", "My two.").into_bytes());
}

#[test]
fn a_protected_branch_goes_to_a_merge_request_whose_link_is_shown() {
    let fake = paper();
    let mut h = opened(&fake);
    fake.protect("main");
    write(&mut h, "intro.tex", "Mine\n");
    ui_cmd::run(h.state_mut(), "forge.commit");
    h.run();
    type_into(&mut h, "What this commit changes", "Mine");
    h.get_by_role_and_label(Role::Button, "Commit").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("does not take your commits").is_some(), "the dialog does not say why a new branch");
    h.get_by_role_and_label(Role::Button, "Commit and Open Merge Request").click();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label("Open the merge request on GitLab").is_some(), "the merge request's link is not shown");
    assert_eq!(fake.merge_requests().len(), 1);
    assert_eq!(h.state().session().forge().map(|l| l.branch.starts_with("main-")), Some(true), "the project does not follow the new branch");
}

#[test]
fn the_branch_is_checked_when_the_window_comes_back() {
    let fake = paper();
    let mut h = opened(&fake);
    fake.push("main", &[("intro.tex", Some(b"Their intro\n".as_slice()))]);
    h.input_mut().focused = false;
    h.run();
    h.input_mut().focused = true;
    h.run();
    assert!(settle(&mut h), "the GitLab operation did not end");
    assert!(h.query_by_label_contains("Took in the new commits of main: intro.tex").is_some(), "the new commit was not taken in");
    let text = h.state().session().project().text(&texcraft_engine::project::ProjectPath::new("intro.tex").unwrap()).ok().map(str::to_owned);
    assert_eq!(text.as_deref(), Some("Their intro\n"));
}
