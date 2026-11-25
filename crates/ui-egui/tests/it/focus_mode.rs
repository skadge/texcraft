//! A9: focus mode hides the menu bar, the rail and the side panel; F10 still reaches the menus,
//! and Escape, once nothing has the keyboard, leaves it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn harness() -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Hello.\n"}))?;
    let prefs = Prefs { auto_compile: false, layout: "split".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    Ok(h)
}

fn press(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    for _ in 0..3 {
        h.run();
    }
}

fn chrome_shown(h: &Harness<'_, App>) -> bool {
    h.query_by_label("File").is_some() || h.query_by_label("Project Files").is_some()
}

#[test]
fn focus_mode_keeps_the_text_and_the_pdf_and_escape_leaves_it_once_the_editor_has_let_go() {
    let mut h = harness().unwrap();
    assert!(chrome_shown(&h));
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run();
    h.run();
    assert!(!chrome_shown(&h), "focus mode left the menu bar or the rail");
    assert!(h.query_by_label("main.tex source").is_some(), "focus mode hid the editor");
    assert!(h.query_by_label("Compile").is_some(), "focus mode hid the PDF pane of the split layout");
    assert!(h.query_by_label("Leave Focus Mode").is_some(), "focus mode has no visible way out");

    press(&mut h, egui::Key::F10);
    assert!(h.query_by_label("File").is_some_and(|n| n.is_focused()), "F10 did not show the menu bar in focus mode");
    press(&mut h, egui::Key::Escape);
    assert!(h.query_by_label("File").is_none(), "the menu bar stayed after the keyboard left it");
    assert!(h.get_by_label("main.tex source").is_focused(), "the keyboard did not go back to the text");

    press(&mut h, egui::Key::Escape);
    assert!(!chrome_shown(&h), "the Escape that left the editor also left focus mode");
    press(&mut h, egui::Key::Escape);
    assert!(chrome_shown(&h), "Escape with nothing focused did not leave focus mode");
}
