//! Copying and pasting in the window's text fields: the clipboard's keys reach the focused field,
//! not the source editor, and middle click pastes the primary selection (X11, Wayland).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn harness(vim: bool) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let _ = s.execute("file.write", &json!({"path": "main.tex", "text": "hello\n"}));
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

/// What the window put on the clipboard in the next frame.
fn copied(h: &mut Harness<'_, App>) -> Option<String> {
    h.step();
    h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(t) => Some(t.clone()),
        _ => None,
    })
}

#[test]
fn the_clipboard_keys_act_on_a_dialog_field_and_not_on_the_editor() {
    let mut h = harness(false);
    h.get_by_label("main.tex source").focus();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.goto_line");
    h.run();
    // As egui-winit sends Ctrl+V: a Paste event, without the key press.
    h.event(egui::Event::Paste("12".into()));
    h.run();
    assert_eq!(h.get_by_label("Line number").value().as_deref(), Some("12"), "the paste did not reach the field");
    assert_eq!(text(&mut h), "hello\n", "the paste went to the editor");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Copy);
    assert_eq!(copied(&mut h).as_deref(), Some("12"), "Ctrl+A then Ctrl+C did not copy the field's text");
    h.event(egui::Event::Cut);
    assert_eq!(copied(&mut h).as_deref(), Some("12"), "Ctrl+X did not copy the field's text");
    h.run();
    assert_eq!(h.get_by_label("Line number").value().as_deref(), Some(""), "Ctrl+X left the text in the field");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(h.get_by_label("Line number").value().as_deref(), Some("12"), "Ctrl+Z did not undo in the field");
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    h.run();
    assert_eq!(h.get_by_label("Line number").value().as_deref(), Some(""), "Ctrl+Shift+Z did not redo in the field");
    assert_eq!(text(&mut h), "hello\n", "a key went to the editor");
}

#[test]
fn a_paste_reaches_a_settings_field() {
    let mut h = harness(false);
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    // The Settings dialog's Review page (the rail has a Review tab too).
    if let Some(tab) = h.get_all_by_label("Review").last() {
        tab.click();
    }
    h.run();
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Your name").click();
    h.run();
    h.event(egui::Event::Paste("Ada".into()));
    h.run();
    assert!(
        h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Your name").value().is_some_and(|v| v.ends_with("Ada")),
        "the paste did not reach the field"
    );
    assert_eq!(text(&mut h), "hello\n", "the paste went to the editor");
}

#[test]
fn the_editor_still_pastes_when_it_has_the_keyboard() {
    let mut h = harness(false);
    h.get_by_label("main.tex source").focus();
    h.run();
    h.event(egui::Event::Paste("A".into()));
    h.run();
    assert!(text(&mut h).contains('A'), "the editor did not paste: {:?}", text(&mut h));
}
