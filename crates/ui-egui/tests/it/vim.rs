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
