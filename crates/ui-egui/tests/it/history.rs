use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-ui-{}-history-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A project saved twice in `dir`: main.tex "first line" then "second line".
fn saved_twice(dir: &std::path::Path) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "first line\n"}))?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    s.execute("file.write", &json!({"path": "main.tex", "text": "second line\n"}))?;
    s.execute("project.save", &json!({}))?;
    Ok(s)
}

fn harness(s: Session) -> Harness<'static, App> {
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness
}

fn main_text(h: &Harness<'_, App>) -> String {
    let Ok(path) = ProjectPath::new("main.tex") else { return String::new() };
    h.state().session().project().text(&path).map(str::to_owned).unwrap_or_default()
}

#[test]
fn the_history_lists_each_save_and_compares_a_file_with_the_version_before() {
    let dir = temp("list").unwrap();
    let mut harness = harness(saved_twice(&dir).unwrap());
    ui_cmd::run(harness.state_mut(), "view.side.history");
    harness.run();
    let versions = harness.query_all_by_label_contains("Saved with TeXCraft").count();
    assert_eq!(versions, 2, "each save is a version");
    // The newest first: choosing it lists its file, choosing the file compares it.
    harness.query_all_by_label_contains("Saved with TeXCraft").next().unwrap().click();
    harness.run();
    harness.get_by_role_and_label(egui::accesskit::Role::TreeItem, "main.tex").click();
    harness.run();
    assert!(harness.query_by_label_contains("Removed 1: first line").is_some(), "the removed line is not shown");
    assert!(harness.query_by_label_contains("Added 1: second line").is_some(), "the added line is not shown");
    assert!(harness.query_by_label_contains("1 line added, 1 line removed").is_some(), "the summary is missing");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn restoring_a_file_from_the_history_asks_first_and_leaves_it_unsaved() {
    let dir = temp("restore").unwrap();
    let mut harness = harness(saved_twice(&dir).unwrap());
    ui_cmd::run(harness.state_mut(), "view.side.history");
    harness.run();
    // The older version.
    harness.query_all_by_label_contains("Saved with TeXCraft").nth(1).unwrap().click();
    harness.run();
    harness.get_by_role_and_label(egui::accesskit::Role::TreeItem, "main.tex").click();
    harness.run();
    harness.get_by_label("Restore This File…").click();
    harness.run();
    assert_eq!(main_text(&harness), "second line\n", "restored before the confirmation");
    assert!(harness.query_by_label_contains("unsaved changes").is_some(), "the confirmation does not say the result is unsaved");
    harness.get_by_label("Restore").click();
    harness.run();
    assert_eq!(main_text(&harness), "first line\n");
    assert!(harness.state().session().is_dirty(), "a restored version is an unsaved change");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn without_a_folder_the_history_says_why_it_is_empty() {
    let s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let mut harness = harness(s);
    ui_cmd::run(harness.state_mut(), "view.side.history");
    harness.run();
    assert!(harness.query_by_label_contains("save it in a folder").is_some(), "no reason is given");
}

fn in_library() -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.use_library(texcraft_engine::library::Library::default());
    s
}

#[test]
fn a_browser_project_lists_each_save_as_a_version() {
    let mut s = in_library();
    s.execute("file.write", &json!({"path": "main.tex", "text": "first line\n"})).unwrap();
    s.execute("project.save_as", &json!({"path": "Paper"})).unwrap();
    s.execute("file.write", &json!({"path": "main.tex", "text": "second line\n"})).unwrap();
    s.execute("project.save", &json!({})).unwrap();
    let mut harness = harness(s);
    ui_cmd::run(harness.state_mut(), "view.side.history");
    harness.run();
    assert_eq!(harness.query_all_by_label_contains("Saved with TeXCraft").count(), 2, "each save is a version");
}

#[test]
fn an_untitled_browser_project_says_its_history_starts_at_its_first_save() {
    let mut harness = harness(in_library());
    ui_cmd::run(harness.state_mut(), "view.side.history");
    harness.run();
    assert!(harness.query_by_label_contains("save it to start one").is_some(), "no reason is given");
}
