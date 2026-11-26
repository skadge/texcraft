use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;

fn edited() -> Result<App, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "changed"}))?;
    Ok(crate::support::app(s))
}

#[test]
fn closing_with_unsaved_changes_waits_for_the_users_answer() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), edited().unwrap());
    harness.run();
    assert!(harness.state_mut().close_requested(), "the close goes through without asking");
    harness.run();
    assert!(!harness.state_mut().take_quit());
    harness.get_by_label("Don't Save").click();
    harness.run();
    assert!(harness.state_mut().take_quit(), "Don't Save does not quit");
    assert!(!harness.state_mut().close_requested(), "the confirmed close is held again");
}

#[test]
fn cancelling_keeps_the_window_open() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), edited().unwrap());
    harness.run();
    assert!(harness.state_mut().close_requested());
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(!harness.state_mut().take_quit());
    assert!(harness.state_mut().close_requested(), "a later close no longer asks");
}

#[test]
fn the_question_says_what_is_about_to_replace_the_project() {
    use egui_kittest::kittest::NodeT as _;
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), edited().unwrap());
    harness.run();
    texcraft_ui_egui::ui_cmd::run(harness.state_mut(), "forge.open");
    harness.run();
    harness.run();
    let said =
        harness.root().children_recursive().filter_map(|n| n.accesskit_node().value()).any(|v| v.contains("GitLab or GitHub is about to open"));
    assert!(said, "the question does not say why it is asked now");
}

/// Issue #23: in the browser the question says where the changes go (this browser's storage),
/// and offers no forge for a project that is on none.
#[test]
fn in_the_browser_the_question_offers_to_keep_the_changes_in_the_browser() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.use_library(texcraft_engine::library::Library::default());
    s.execute("file.write", &json!({"path": "main.tex", "text": "changed"}))?;
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    h.run();
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "project.new");
    h.run();
    assert!(h.query_by_label("Keep in This Browser").is_some() && h.query_by_label("Don't Keep").is_some(), "the browser's choices are missing");
    assert!(h.query_by_label("Save").is_none() && h.query_by_label("Don't Save").is_none(), "the browser still asks to Save");
    assert!(h.query_all_by_label_contains("Commit to").next().is_none(), "a forge is offered for a project on none");
    Ok(())
}

/// For a project opened from GitLab, committing is a choice of its own: it opens the commit
/// dialog, and what was about to happen waits.
#[test]
fn a_project_from_gitlab_can_be_committed_from_the_question() {
    let fake = crate::forge::paper();
    let mut h = crate::forge::opened(&fake);
    crate::forge::write(&mut h, "main.tex", "Changed.\n");
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "project.new");
    h.run();
    h.get_by_label("Commit to GitLab…").click();
    h.run();
    let ui = h.state_mut().control("ui.inspect", &json!({}));
    assert_eq!(ui["result"]["dialog"], "forge_commit", "{ui}");
    assert!(h.state().session().forge().is_some(), "the project was replaced");
}
