//! Settings: each section starts at its top (issue #55), and the search finds settings across the
//! sections, usable where they are found (issue #54).

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn harness(size: egui::Vec2, language: &str) -> Harness<'static, App> {
    let s = Session::with_backend(Box::new(texcraft_compile::Unavailable("tests".into())));
    let prefs = Prefs { auto_compile: false, language: language.to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(size).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.run();
    h
}

/// The top of the first node named `label`.
fn top(h: &Harness<'_, App>, label: &str) -> f32 {
    h.query_all_by_label(label).map(|n| n.rect().top()).fold(f32::INFINITY, f32::min)
}

#[test]
fn another_section_starts_at_its_top() {
    let mut h = harness(egui::vec2(1200.0, 440.0), "en");
    ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": "editor"}));
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Check comments").scroll_to_me();
    h.run();
    let tabs = top(&h, "Appearance");
    assert!(top(&h, "Font size") < tabs, "the Editor section did not scroll");
    h.get_by_role_and_label(Role::Tab, "Building").click();
    h.run();
    h.run();
    assert!(top(&h, "Compile automatically") >= tabs - 1.0, "Building starts where Editor was scrolled to");
}

#[test]
fn on_a_phone_another_section_starts_at_its_top() {
    let mut h = harness(egui::vec2(390.0, 640.0), "en");
    ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": "editor"}));
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Check comments").scroll_to_me();
    h.run();
    let bar = h.get_by_label("Back to Settings").rect().bottom();
    assert!(top(&h, "Font size") < bar, "the Editor page did not scroll");
    // Another section's page, straight from this one (a tip's Open Settings, an agent).
    ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": "building"}));
    h.run();
    h.run();
    assert!(top(&h, "Compile automatically") >= bar, "Building's page starts where Editor's was scrolled to");
}

#[test]
fn the_search_finds_settings_across_sections_and_they_work_there() {
    let mut h = harness(egui::vec2(1200.0, 800.0), "en");
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run();
    let field = h.get_by_label("Search settings");
    assert!(field.is_focused(), "Ctrl+F does not give the keyboard to the search");
    field.type_text("SPELLING");
    h.run();
    h.run();
    assert!(h.query_by_role_and_label(Role::CheckBox, "Check spelling").is_some(), "a setting of Editor is not found");
    assert!(h.query_all_by_label("Theme").next().is_none(), "a setting without the word is shown");
    assert!(h.query_by_role_and_label(Role::Heading, "Editor").is_some(), "the setting's section is not named");
    assert!(h.query_by_label_contains("settings found").is_some(), "the results are not counted");
    // Usable in place.
    let on = h.state().session().settings.spelling.on;
    h.get_by_role_and_label(Role::CheckBox, "Check spelling").click();
    h.run();
    assert_ne!(h.state().session().settings.spelling.on, on, "the setting found does not work");

    // Words from two sections' settings: both sections.
    h.get_by_label("Search settings").focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label("Search settings").type_text("automatically");
    h.run();
    h.run();
    assert!(h.query_by_role_and_label(Role::CheckBox, "Save automatically").is_some(), "Editor's setting is not found");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Compile automatically").is_some(), "Building's setting is not found");
    assert!(h.query_by_role_and_label(Role::Heading, "Building").is_some());

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label("Search settings").type_text("zzzq");
    h.run();
    h.run();
    assert!(h.query_by_label("No setting matches the search.").is_some(), "no result is not said");

    // Escape clears the search, then closes.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Search settings").is_some(), "the first Escape closed Settings");
    assert!(h.query_by_label("No setting matches the search.").is_none(), "Escape did not clear the search");
    assert!(h.query_all_by_label("Theme").next().is_some(), "the sections are not back");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Search settings").is_none(), "the second Escape did not close Settings");
}

#[test]
fn the_search_ignores_accents_and_knows_the_english() {
    let mut h = harness(egui::vec2(1200.0, 800.0), "fr");
    ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"search": "theme"}));
    h.run();
    h.run();
    assert!(h.query_all_by_label("Thème").next().is_some(), "\"theme\" does not find Thème");
    assert!(h.query_all_by_label("Langue de l'interface").next().is_none(), "the search is not applied");
    ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"search": "font size"}));
    h.run();
    h.run();
    assert!(h.query_all_by_label("Taille de police").next().is_some(), "the English name does not find the setting");
}
