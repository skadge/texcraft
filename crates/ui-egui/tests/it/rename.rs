//! Editing the project's name in the top bar.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::library::Library;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn harness(saved_as: Option<&str>) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.use_library(Library::default());
    if let Some(name) = saved_as {
        s.execute("project.save_as", &json!({"path": name}))?;
    }
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    Ok(Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app))
}

fn type_name(h: &mut Harness<'_, App>, button: &str, name: &str, key: egui::Key) {
    h.get_by_label(button).click();
    h.run();
    h.get_by_label("Project name").type_text(name);
    h.run();
    h.key_press(key);
    h.run();
}

fn name(h: &mut Harness<'_, App>) -> Option<String> {
    h.state().session().dir().map(|d| d.display().to_string())
}

#[test]
fn a_name_typed_over_the_old_one_renames_the_project_and_its_recent_entry() -> Result<(), String> {
    let mut h = harness(Some("Draft"))?;
    h.run();
    type_name(&mut h, "Draft: rename the project", "Thesis", egui::Key::Enter);
    assert_eq!(name(&mut h).as_deref(), Some("Thesis"), "the whole old name was selected and replaced");
    assert_eq!(h.state().session().library().map(Library::names), Some(vec!["Thesis".to_owned()]));
    let recent = h.state_mut().take_changed_prefs().map(|p| p.recent).unwrap_or_default();
    assert!(recent.contains(&"Thesis".to_owned()) && !recent.contains(&"Draft".to_owned()), "{recent:?}");
    Ok(())
}

#[test]
fn escape_keeps_the_name() -> Result<(), String> {
    let mut h = harness(Some("Draft"))?;
    h.run();
    type_name(&mut h, "Draft: rename the project", "Thesis", egui::Key::Escape);
    assert_eq!(name(&mut h).as_deref(), Some("Draft"));
    assert!(h.query_by_label("Project name").is_none(), "the field is closed");
    Ok(())
}

#[test]
fn naming_an_untitled_project_in_the_browser_saves_it_under_that_name() -> Result<(), String> {
    let mut h = harness(None)?;
    h.run();
    type_name(&mut h, "Untitled project: name and save the project", "Paper", egui::Key::Enter);
    assert_eq!(name(&mut h).as_deref(), Some("Paper"));
    assert!(!h.state().session().is_dirty());
    Ok(())
}
