use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn typed(keys: &str) -> Result<String, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": ""}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    for c in keys.chars() {
        if c == '<' {
            harness.key_press(egui::Key::Backspace);
        } else {
            harness.event(egui::Event::Text(c.to_string()));
        }
        harness.run();
    }
    let path = ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    harness.state_mut().session().project().text(&path).map(str::to_owned).map_err(|e| e.to_string())
}

#[test]
fn brackets_close_as_they_are_typed_and_are_stepped_over() {
    assert_eq!(typed("f(x").unwrap(), "f(x)");
    assert_eq!(typed("f(x)y").unwrap(), "f(x)y", "the closing bracket was typed twice");
    assert_eq!(typed("(<").unwrap(), "", "Backspace left the closing bracket");
    assert_eq!(typed("a\\{").unwrap(), "a\\{", "an escaped brace was paired");
}

#[test]
fn finishing_a_begin_adds_its_end_with_a_line_for_the_caret() {
    assert_eq!(typed("\\begin{itemize}x").unwrap(), "\\begin{itemize}\n  x\n\\end{itemize}");
}
