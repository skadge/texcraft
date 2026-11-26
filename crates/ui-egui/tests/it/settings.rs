//! Settings: each section starts at its top (issue #55).

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
