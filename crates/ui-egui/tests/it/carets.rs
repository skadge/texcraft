//! Several carets: Cmd+D adds the next occurrence, Alt+click adds a caret, typing goes in at
//! every one as one undo step, and Escape returns to one caret.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn harness(text: &str) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    Ok(harness)
}

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn cmd_d_selects_each_occurrence_and_typing_replaces_them_all_as_one_undo_step() -> Result<(), String> {
    let mut harness = harness("one two one")?;
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.run();
    assert!(harness.query_by_label_contains("2 carets").is_some(), "adding the occurrence was not announced");
    harness.get_by_label("main.tex source").type_text("X");
    harness.run();
    assert_eq!(text(&mut harness), "X two X");
    ui_cmd::run(harness.state_mut(), "edit.undo");
    harness.run();
    assert_eq!(text(&mut harness), "one two one", "the edit at both took more than one undo");
    Ok(())
}

#[test]
fn alt_click_adds_a_caret_and_escape_leaves_one() -> Result<(), String> {
    let mut harness = harness("abc\ndef")?;
    // The second line's start (as the Vim click test finds it).
    let source = harness.get_by_label("main.tex source").rect();
    let p = egui::pos2(source.left() + 60.0, source.top() + 8.0 + 21.0 + 10.0);
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    harness.event(egui::Event::PointerMoved(p));
    harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::ALT });
    harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::ALT });
    harness.run();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
    harness.get_by_label("main.tex source").type_text("X");
    harness.run();
    assert_eq!(text(&mut harness), "Xabc\nXdef", "typing did not go in at both carets");
    harness.key_press(egui::Key::ArrowRight);
    harness.key_press(egui::Key::Backspace);
    harness.run();
    assert_eq!(text(&mut harness), "Xbc\nXef", "Right and Backspace did not act at both carets");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.get_by_label("main.tex source").is_focused(), "Escape left the editor instead of dropping the extra caret");
    harness.get_by_label("main.tex source").type_text("Y");
    harness.run();
    assert_eq!(text(&mut harness).matches('Y').count(), 1, "Escape kept several carets");
    Ok(())
}
