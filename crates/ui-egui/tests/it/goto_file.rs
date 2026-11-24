use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn current(harness: &Harness<'_, App>) -> Option<String> {
    harness.state().session().current().map(|p| p.as_str().to_owned())
}

#[test]
fn go_to_file_opens_the_file_typed_and_lists_it_first_next_time() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}"})).unwrap();
    s.execute("file.new", &json!({"path": "chapters/intro.tex", "text": "Intro"})).unwrap();
    s.execute("file.new", &json!({"path": "chapters/method.tex", "text": "Method"})).unwrap();
    s.execute("file.open", &json!({"path": "main.tex"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    ui_cmd::run(harness.state_mut(), "view.go_to_file");
    harness.run();
    harness.get_by_label("Type part of a file's path").type_text("intr");
    harness.run();
    assert!(harness.query_by_label_contains("chapters/intro.tex, 1 of 1").is_some(), "the match is not announced as selected");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(current(&harness).as_deref(), Some("chapters/intro.tex"));
    assert!(harness.query_by_label("Type part of a file's path").is_none(), "the palette stayed open");

    // Opened last, it leads the list when nothing is typed; Down then Enter opens the next one.
    ui_cmd::run(harness.state_mut(), "view.go_to_file");
    harness.run();
    assert!(harness.query_by_label_contains("chapters/intro.tex, 1 of 3").is_some(), "the recent file is not first");
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_ne!(current(&harness).as_deref(), Some("chapters/intro.tex"), "Down did not move the selection");
}

#[test]
fn escape_closes_go_to_file_without_opening_anything() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "x"})).unwrap();
    s.execute("file.new", &json!({"path": "other.tex", "text": "y"})).unwrap();
    s.execute("file.open", &json!({"path": "main.tex"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    ui_cmd::run(harness.state_mut(), "view.go_to_file");
    harness.run();
    harness.get_by_label("Type part of a file's path").type_text("other");
    harness.run();
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Type part of a file's path").is_none(), "Escape left the palette open");
    assert_eq!(current(&harness).as_deref(), Some("main.tex"));
}
