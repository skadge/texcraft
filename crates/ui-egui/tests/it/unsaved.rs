use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;

fn edited() -> Result<App, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "changed"}))?;
    Ok(App::new(s))
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
