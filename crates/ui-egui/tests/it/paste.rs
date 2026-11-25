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
    with_text("hello\n", vim)
}

fn with_text(text: &str, vim: bool) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let _ = s.execute("file.write", &json!({"path": "main.tex", "text": text}));
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

/// A key's press alone: the harness runs a frame per event and keeps the last frame's output, so
/// what the press puts out is read in the next step.
fn press(h: &Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) {
    h.event(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
}

/// What the window put in the primary selection over the next frames.
fn primary(h: &mut Harness<'_, App>) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..4 {
        h.step();
        out.extend(h.output().platform_output.commands.iter().filter_map(|c| match c {
            egui::OutputCommand::TextSelectionSettled(t) => Some(t.clone()),
            _ => None,
        }));
    }
    out
}

/// A middle click at `pos`, as egui-winit sends it on X11 and Wayland: the button, then the
/// primary selection's text with the click's place.
fn middle_click(h: &mut Harness<'_, App>, pos: egui::Pos2, text: &str) {
    h.hover_at(pos);
    h.run();
    let press = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: egui::Modifiers::NONE };
    h.event(press(true));
    h.event(egui::Event::MiddleClickPaste { pos, text: text.into() });
    h.step();
    h.event(press(false));
    h.run();
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

#[test]
fn vims_normal_mode_still_takes_ctrl_v_as_its_own_key() {
    let mut h = harness(true);
    h.get_by_label("main.tex source").focus();
    h.run();
    // Ctrl+V reaches the window as a paste; in Normal mode it is Vim's (block selection, which
    // says it is not supported yet), not the clipboard's text.
    h.event(egui::Event::Paste("zzz".into()));
    h.run();
    assert_eq!(text(&mut h), "hello\n", "Ctrl+V in Normal mode pasted");
    assert!(h.query_by_label_contains("Visual block").is_some(), "Ctrl+V did not reach Vim");
}

#[test]
fn a_middle_click_on_the_text_pastes_the_primary_selection_where_it_was_clicked() {
    let mut h = with_text("hello\nworld", false);
    h.get_by_label("main.tex source").focus();
    h.run();
    // Below the last line: the end of the text, away from the caret at its start.
    let rect = h.get_by_label("main.tex source").rect();
    middle_click(&mut h, egui::pos2(rect.center().x, rect.bottom() - 20.0), "PRIMARY");
    let now = text(&mut h);
    assert!(now.find("PRIMARY") > now.find("world"), "the primary selection was not pasted at the click: {now:?}");
    assert!(now.starts_with("hello"), "the paste went to the caret: {now:?}");
}

#[test]
fn a_middle_click_pastes_into_a_field_and_not_into_the_editor() {
    let mut h = harness(false);
    ui_cmd::run(h.state_mut(), "edit.goto_line");
    h.run();
    let field = h.get_by_label("Line number").rect().center();
    middle_click(&mut h, field, "7");
    assert_eq!(h.get_by_label("Line number").value().as_deref(), Some("7"), "the primary selection was not pasted in the field");
    assert_eq!(text(&mut h), "hello\n", "the primary selection went to the editor");
}

#[test]
fn selecting_text_in_the_editor_the_formatted_view_and_a_field_sets_the_primary_selection() {
    let mut h = harness(false);
    h.get_by_label("main.tex source").focus();
    h.run();
    press(&h, egui::Modifiers::SHIFT, egui::Key::End);
    assert!(primary(&mut h).iter().any(|t| t == "hello"), "the editor's selection is not the primary selection");

    ui_cmd::run(h.state_mut(), "edit.goto_line");
    h.run();
    h.event(egui::Event::Paste("42".into()));
    h.run();
    press(&h, egui::Modifiers::COMMAND, egui::Key::A);
    assert!(primary(&mut h).iter().any(|t| t == "42"), "the field's selection is not the primary selection");

    let mut h = with_text("First paragraph.\n", false);
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    press(&h, egui::Modifiers::SHIFT, egui::Key::End);
    assert!(primary(&mut h).iter().any(|t| t == "First paragraph."), "the formatted view's selection is not the primary selection");
}
