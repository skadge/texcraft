//! Editing in the formatted view: keys make minimal edits of the source, shared with source mode.

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

/// The formatted view of `text`, its text holding the keyboard.
fn formatted(text: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": text})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("Formatted").click();
    harness.run();
    harness.get_by_label("main.tex formatted").focus();
    harness.run();
    harness
}

fn key(harness: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run();
}

#[test]
fn typing_enter_and_bold_edit_the_source_minimally_and_undo_restores_it() {
    let original = "First paragraph.\n\nSecond one.\n";
    let mut h = formatted(original);
    // The caret starts at the text's start: End takes it after "paragraph.".
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    h.get_by_label("main.tex formatted").type_text(" More");
    h.run();
    assert_eq!(text(&mut h), "First paragraph. More\n\nSecond one.\n", "typing is not an insertion at the caret");

    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.get_by_label("main.tex formatted").type_text("New");
    h.run();
    assert_eq!(text(&mut h), "First paragraph. More\n\nNew\n\nSecond one.\n", "Enter did not make a paragraph of its own");
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Paragraph, "New").is_some(), "the new paragraph is not shown");

    key(&mut h, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::B);
    assert_eq!(text(&mut h), "First paragraph. More\n\n\\textbf{New}\n\nSecond one.\n", "Ctrl+B did not wrap the selected word");

    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(text(&mut h), "First paragraph. More\n\nNew\n\nSecond one.\n", "undo did not take the bold away");
    // The history is the source editor's: undo from the menu goes on to the original text.
    for _ in 0..10 {
        if text(&mut h) == original {
            break;
        }
        texcraft_ui_egui::ui_cmd::run(h.state_mut(), "edit.undo");
        h.run();
    }
    assert_eq!(text(&mut h), original);
}

#[test]
fn special_characters_are_escaped_and_an_atom_deletes_in_two_presses() {
    let mut h = formatted("See \\ref{sec:a} now.\n");
    // After "See " and the reference.
    for _ in 0..5 {
        key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    assert_eq!(text(&mut h), "See \\ref{sec:a} now.\n", "the first Backspace deleted the reference instead of selecting it");
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    assert_eq!(text(&mut h), "See  now.\n", "the second Backspace did not delete the whole reference");
    h.get_by_label("main.tex formatted").type_text("50%");
    h.run();
    assert_eq!(text(&mut h), "See 50\\% now.\n");
}
