use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

fn project() -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\input{chapters/intro}\n"}))?;
    s.execute("file.new", &json!({"path": "chapters/intro.tex", "text": "First line\nThe needle is here.\nA needle again."}))?;
    s.execute("file.open", &json!({"path": "main.tex"}))?;
    Ok(s)
}

fn harness(s: Session) -> Harness<'static, App> {
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness
}

fn cursor_line(h: &mut Harness<'_, App>) -> Option<u64> {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["cursor"]["line"].as_u64()
}

#[test]
fn a_search_result_opens_its_file_at_its_line_from_the_keyboard() {
    let mut harness = harness(project().unwrap());
    ui_cmd::run(harness.state_mut(), "view.find_in_project");
    harness.run();
    harness.get_by_label("Search in the project").type_text("needle");
    harness.run();
    assert!(harness.query_by_label("2 matches in 1 file").is_some(), "the summary is missing");
    // Enter goes to the first result, Enter again opens it.
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.key_press(egui::Key::Enter);
    // The line flashes as it is reached.
    harness.run_steps(4);
    let current = harness.state().session().current().map(|p| p.as_str().to_owned());
    assert_eq!(current.as_deref(), Some("chapters/intro.tex"));
    assert_eq!(cursor_line(&mut harness), Some(2), "the editor is not at the match's line");
}

#[test]
fn replace_all_in_the_project_asks_first_then_replaces_in_every_file() {
    let mut harness = harness(project().unwrap());
    ui_cmd::run(harness.state_mut(), "view.find_in_project");
    harness.run();
    harness.get_by_label("Search in the project").type_text("in");
    harness.run();
    harness.get_by_label("Show Replace").click();
    harness.run();
    harness.get_by_label("Replace with").click();
    harness.run();
    harness.get_by_label("Replace with").type_text("IN");
    harness.run();
    harness.get_by_label("Replace All").click();
    harness.run();
    let intro = ProjectPath::new("chapters/intro.tex").unwrap();
    let text = |h: &Harness<'_, App>| h.state().session().project().text(&intro).map(str::to_owned).unwrap_or_default();
    assert!(!text(&harness).contains("IN"), "replaced before the confirmation");
    assert!(harness.query_by_label_contains("in 2 files?").is_some(), "the confirmation does not say how many files");
    // The dialog's button (the panel's is behind it).
    harness.get_all_by_label("Replace All").last().unwrap().click();
    harness.run();
    assert!(text(&harness).contains("First lINe"), "intro.tex was not replaced");
    let main = harness.state().session().project().text(&ProjectPath::new("main.tex").unwrap()).unwrap().to_owned();
    assert!(main.contains("\\INput"), "main.tex was not replaced");
}
