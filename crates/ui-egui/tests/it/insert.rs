use egui_kittest::Harness;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

#[test]
fn an_insert_command_puts_its_markup_at_the_caret_of_the_shown_file() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Hello"})).unwrap();
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut App| app.frame(ui),
        // An automatic build would keep the harness repainting for as long as its thread takes.
        App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() }),
    );
    harness.run();
    ui_cmd::run(harness.state_mut(), "edit.bold");
    harness.run();
    let text = harness.state_mut().session().project().text(&texcraft_engine::project::ProjectPath::new("main.tex").unwrap()).unwrap().to_owned();
    assert!(text.contains("\\textbf{"), "{text}");
    assert!(text.contains("Hello"), "the existing text is kept: {text}");
}
