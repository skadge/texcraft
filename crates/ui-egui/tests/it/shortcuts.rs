use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn a_command_chord_runs_while_typing_and_plain_letters_stay_text() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": ""})).unwrap();
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::new(s));
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    harness.get_by_label("main.tex source").type_text("b");
    harness.run();
    assert_eq!(text(&mut harness), "b", "a plain letter typed into the editor");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    harness.run();
    assert!(text(&mut harness).contains("\\textbf{"), "the bold shortcut ran: {}", text(&mut harness));
}
