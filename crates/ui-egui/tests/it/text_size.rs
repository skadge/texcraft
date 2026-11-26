//! Issue #33: Ctrl with the mouse wheel (or a pinch) over the text changes its size, in the source
//! and the formatted view, within bounds, kept with the preferences.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::{TEXT_SIZE_MAX, TEXT_SIZE_MIN};

fn harness() -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": "Some text.\n"})).is_ok());
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    h.run();
    h
}

/// Zooms by `factor` with the pointer at `at`, as Ctrl with the wheel does; the size kept after.
fn zoom(h: &mut Harness<'_, App>, at: egui::Pos2, factor: f32) -> f32 {
    h.event(egui::Event::PointerMoved(at));
    h.event(egui::Event::Zoom(factor));
    h.run();
    h.state_mut().take_changed_prefs().map_or(f32::NAN, |p| p.editor_font_size)
}

#[test]
fn ctrl_and_the_wheel_over_the_text_change_its_size_within_bounds_and_elsewhere_do_not() {
    let mut h = harness();
    let before = h.state_mut().take_changed_prefs().map_or(texcraft_ui_egui::prefs::TEXT_SIZE_DEFAULT, |p| p.editor_font_size);
    let source = h.get_by_label("main.tex source").rect().center();
    let larger = zoom(&mut h, source, 1.2);
    assert!(larger > before, "the source's text did not grow: {before} then {larger}");
    assert!(zoom(&mut h, source, 100.0) <= TEXT_SIZE_MAX, "past the largest size");
    assert!(zoom(&mut h, source, 0.001) >= TEXT_SIZE_MIN, "past the smallest size");

    // Over the formatted view too (the text is at its smallest now).
    h.get_by_label("Formatted").click();
    h.run();
    let formatted = h.get_by_label("main.tex formatted").rect().center();
    let grown = zoom(&mut h, formatted, 1.5);
    assert!(grown > TEXT_SIZE_MIN, "the formatted view's text did not grow: {grown}");

    // Over the window's top bar, nothing changes.
    let top = egui::pos2(600.0, 5.0);
    let unchanged = zoom(&mut h, top, 1.5);
    assert!(unchanged.is_nan(), "the text's size changed with the pointer off the text: {unchanged}");
}
