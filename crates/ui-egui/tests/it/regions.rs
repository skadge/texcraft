use egui_kittest::Harness;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn announced(h: &mut Harness<'_, App>) -> String {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["status"]["text"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn f6_visits_every_region_in_order_and_wraps_and_shift_f6_goes_back() {
    let app = App::with_prefs(Session::with_backend(Box::new(Unavailable("tests".into()))), Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    let mut seen = Vec::new();
    for _ in 0..6 {
        h.key_press(egui::Key::F6);
        h.run();
        seen.push(announced(&mut h));
    }
    let expected = ["Menu bar and project", "Sections", "Project files and outline", "Editor", "PDF and build log"];
    assert_eq!(seen[..5], expected, "{seen:?}");
    assert_eq!(seen[5], expected[0], "F6 wraps around");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F6);
    h.run();
    assert_eq!(announced(&mut h), expected[4], "Shift+F6 goes back");
}
