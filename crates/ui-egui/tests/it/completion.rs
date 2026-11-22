use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn editor(text: &str, vim: bool) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    Ok(harness)
}

fn type_chars(harness: &mut Harness<'_, App>, text: &str) {
    for c in text.chars() {
        harness.event(egui::Event::Text(c.to_string()));
        harness.run();
    }
}

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn a_label_is_offered_in_a_reference_and_enter_puts_it_in() {
    let mut harness = editor("", false).unwrap();
    type_chars(&mut harness, "\\label{sec:intro} \\ref{s");
    assert!(harness.query_by_label("sec:intro").is_some(), "the label is not offered");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(text(&mut harness), "\\label{sec:intro} \\ref{sec:intro}");
    assert!(harness.query_by_label("sec:intro").is_none(), "the list stayed open");
}

#[test]
fn escape_closes_the_list_before_it_leaves_vims_insert_mode() {
    let mut harness = editor("", true).unwrap();
    type_chars(&mut harness, "i\\label{a} \\ref{");
    assert!(harness.query_by_label("a").is_some(), "the label is not offered in Insert mode");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("a").is_none(), "Escape left the list open");
    assert!(harness.query_by_label_contains("INSERT").is_some(), "Escape also left Insert mode");
}

#[test]
fn f12_goes_from_a_reference_to_its_label() {
    let mut harness = editor("Intro.\n\\section{A}\\label{sec:a}\nSee \\ref{sec:a}.", false).unwrap();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    for _ in 0..4 {
        harness.key_press(egui::Key::ArrowLeft);
    }
    harness.run();
    assert!(harness.query_by_label_contains("Ln 3,").is_some());
    harness.key_press(egui::Key::F12);
    harness.run();
    assert!(harness.query_by_label_contains("Ln 2,").is_some(), "F12 did not go to the label");
}

#[test]
fn quick_fix_from_the_keyboard_applies_the_first_fix() {
    let mut harness = editor("Old style: {\\bf bold}.", false).unwrap();
    texcraft_ui_egui::ui_cmd::run(harness.state_mut(), "edit.quick_fix");
    harness.run();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!text(&mut harness).contains("\\bf "), "the fix was not applied: {}", text(&mut harness));
}
