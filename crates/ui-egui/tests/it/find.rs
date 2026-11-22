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
