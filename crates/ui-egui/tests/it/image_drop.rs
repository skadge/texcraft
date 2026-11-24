//! An image dragged from the file list onto the source becomes an `\includegraphics` where it is
//! let go; another file dragged there changes nothing.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn drag(harness: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    harness.event(button(from, true));
    harness.step();
    for i in 1..=10 {
        harness.event(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
        harness.step();
    }
    harness.event(button(to, false));
    harness.run_steps(3);
}

fn main_text(harness: &mut Harness<'_, App>) -> String {
    let Ok(main) = ProjectPath::new("main.tex") else { return String::new() };
    harness.state_mut().session().project().text(&main).map(str::to_owned).unwrap_or_default()
}

#[test]
fn an_image_dropped_on_the_source_is_included_there() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\begin{document}\nHello.\n\\end{document}\n"})).unwrap();
    s.execute("file.set_main", &json!({"path": "main.tex"})).unwrap();
    s.execute("file.new", &json!({"path": "notes.txt", "text": "x"})).unwrap();
    s.execute("file.import", &json!({"path": "figures/plot.png", "data": "iVBORw0KGgo="})).unwrap();
    s.execute("file.open", &json!({"path": "main.tex"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness =
        Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    let editor = harness.get_by_label("main.tex source").rect();
    // Below the text: the end of the file.
    let target = editor.center_bottom() - egui::vec2(0.0, 40.0);

    let notes = harness.get_by_role_and_label(egui::accesskit::Role::TreeItem, "notes.txt").rect().center();
    let before = main_text(&mut harness);
    drag(&mut harness, notes, target);
    assert_eq!(main_text(&mut harness), before, "a file that is not an image changed the text");

    let plot = harness.get_by_role_and_label(egui::accesskit::Role::TreeItem, "plot.png").rect().center();
    drag(&mut harness, plot, target);
    let text = main_text(&mut harness);
    assert!(text.starts_with(&before), "the existing text is not kept: {text}");
    assert!(text.contains("\\includegraphics[width=\\linewidth]{figures/plot.png}"), "{text}");
    // The main document has no graphicx: the status line says so.
    assert!(harness.query_by_label_contains("graphicx").is_some(), "the missing graphicx is not said");
}

#[test]
fn the_keyboard_inserts_an_image_at_the_caret_from_the_files_menu() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\usepackage{graphicx}\n"})).unwrap();
    s.execute("file.set_main", &json!({"path": "main.tex"})).unwrap();
    s.execute("file.import", &json!({"path": "plot.png", "data": "iVBORw0KGgo="})).unwrap();
    s.execute("file.open", &json!({"path": "main.tex"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    let before = main_text(&mut harness);
    harness.get_by_role_and_label(egui::accesskit::Role::TreeItem, "plot.png").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    harness.run();
    // Into the menu, down to its entry.
    for _ in 0..6 {
        if harness.query_by_label("Insert in Document").is_some_and(|n| n.is_focused()) {
            break;
        }
        harness.key_press(egui::Key::Tab);
        harness.run();
    }
    harness.key_press(egui::Key::Enter);
    harness.run();
    let text = main_text(&mut harness);
    assert!(text.starts_with("\\includegraphics") && text.ends_with(&before), "not inserted at the caret: {text}");
}
