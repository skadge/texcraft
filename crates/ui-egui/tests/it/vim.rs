use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn vim_keys_typed_into_the_editor_edit_the_file_and_its_ctrl_keys_reach_it() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": ""})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim: true, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    harness.get_by_label("main.tex source").type_text("ihello");
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.get_by_label("main.tex source").type_text("x");
    harness.run();
    assert_eq!(text(&mut harness), "hell", "Insert mode, Escape and x");
    harness.get_by_label("main.tex source").type_text("u");
    harness.run();
    assert_eq!(text(&mut harness), "hello");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::R);
    harness.run();
    assert_eq!(text(&mut harness), "hell", "Ctrl-R did not reach Vim");
}

#[test]
fn a_click_moves_vims_caret_at_once() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "abc\ndef"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim: true, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    // The second line's first character: the source's top left is laid out as the window draws it.
    let line_one = harness.get_by_label("main.tex source").rect();
    let p = egui::pos2(line_one.left() + 60.0, line_one.top() + 8.0 + 21.0 + 10.0);
    harness.event(egui::Event::PointerMoved(p));
    harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    harness.run();
    // Before any key: the caret shown (and the status bar's place) is where the click was.
    assert!(harness.query_by_label_contains("Ln 2,").is_some(), "Vim's caret stayed on line 1 after the click");
}

#[test]
fn ctrl_arrows_move_vims_caret_by_words() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "one two three"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim: true, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowRight);
    harness.run();
    harness.get_by_label("main.tex source").type_text("x");
    harness.run();
    assert_eq!(text(&mut harness), "one wo three", "Ctrl+Right in Normal mode");
    harness.get_by_label("main.tex source").type_text("i");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowRight);
    harness.get_by_label("main.tex source").type_text("X");
    harness.run();
    assert_eq!(text(&mut harness), "one wo Xthree", "Ctrl+Right in Insert mode");
}
