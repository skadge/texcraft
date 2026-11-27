use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn replace_all_from_the_find_bar_is_one_undoable_edit() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "one two one"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    ui_cmd::run(harness.state_mut(), "edit.replace");
    harness.run();
    harness.get_by_label("Find").type_text("one");
    harness.run();
    assert!(harness.query_by_label_contains("of 2").is_some(), "the count of matches is missing");
    harness.get_by_label("Replace with").click();
    harness.run();
    harness.get_by_label("Replace with").type_text("1");
    harness.run();
    harness.get_by_label("Replace All").click();
    harness.run();
    assert_eq!(text(&mut harness), "1 two 1");
    ui_cmd::run(harness.state_mut(), "edit.undo");
    harness.run();
    assert_eq!(text(&mut harness), "one two one", "Replace All took more than one undo");
}

/// The formatted view of `text` with the replace bar open over it.
fn formatted_replace(text: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": text})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.replace");
    h.run();
    h
}

#[test]
fn find_and_replace_in_the_formatted_view_see_its_text_and_replace_with_text() {
    const TEXT: &str = "A \\textbf{word} and a word.\n";
    // A command's name is not the view's text.
    let mut h = formatted_replace(TEXT);
    h.get_by_label("Find").type_text("textbf");
    h.run();
    assert!(h.query_by_label_contains("No results").is_some(), "the bar found a command's name in the formatted view");
    assert!(h.query_by_label("main.tex formatted").is_some(), "Find left the formatted view");
    let mut h = formatted_replace(TEXT);
    h.get_by_label("Find").type_text("word");
    h.run();
    assert!(h.query_by_label_contains("of 2").is_some(), "the bar does not find the view's words");
    h.get_by_label("Replace with").click();
    h.run();
    h.get_by_label("Replace with").type_text("50%");
    h.run();
    h.get_by_label("Replace All").click();
    h.run();
    assert_eq!(text(&mut h), "A \\textbf{50\\%} and a 50\\%.\n", "the replacement is not text as typed");
}

#[test]
fn find_in_the_formatted_view_finds_words_over_a_styles_boundary_and_replace_keeps_the_style() {
    let mut h = formatted_replace("Some \\textbf{bold} text here.\n");
    h.get_by_label("Find").type_text("bold text");
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("of 1").is_some() || h.query_by_label_contains("1 result").is_some(),
        "the words over the style's end are not found"
    );
    h.get_by_label("Replace with").click();
    h.run();
    h.get_by_label("Replace with").type_text("new words");
    h.run();
    h.get_by_label("Replace All").click();
    h.run();
    let after = text(&mut h);
    assert!(after.contains("\\textbf{new words}") && after.contains("} here."), "the replacement lost the style's braces: {after:?}");
}
