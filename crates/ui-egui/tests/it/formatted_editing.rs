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

/// Clicks at `at` with the mouse, the button released in the next frame.
fn click(h: &mut Harness<'_, App>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
    }
    h.run();
}

#[test]
fn a_click_in_the_text_keeps_the_keyboard_for_typed_and_composed_text() {
    // #24: the caret showed while the button was down and went away with the click, so the
    // browser's text (its input's `Text` and IME events) went nowhere.
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": "First paragraph.\n\nSecond one.\n"})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    let r = h.get_by_role_and_label(egui::accesskit::Role::Paragraph, "Second one.").rect();
    click(&mut h, egui::pos2(r.max.x - 1.0, r.center().y));
    h.event(egui::Event::Text("!".into()));
    h.run();
    assert_eq!(text(&mut h), "First paragraph.\n\nSecond one.!\n", "typed text after a click went nowhere");
    for event in [egui::ImeEvent::Preedit { text: "e".into(), active_range_chars: None }, egui::ImeEvent::Commit("é".into())] {
        h.event(egui::Event::Ime(event));
        h.run();
    }
    assert_eq!(text(&mut h), "First paragraph.\n\nSecond one.!é\n", "composed text after a click went nowhere");
}

/// Clicks `times` times in a row at `at` with the mouse and `modifiers`, quickly enough to count
/// as a double or triple click.
fn clicks(h: &mut Harness<'_, App>, at: egui::Pos2, times: usize, modifiers: egui::Modifiers) {
    // Held down through the clicks, as the window reports keys held.
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.event(egui::Event::PointerMoved(at));
    h.step();
    for _ in 0..times {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers });
            h.step();
        }
    }
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    // A jump to the source flashes its line for a while.
    h.run_steps(4);
}

#[test]
fn a_double_click_selects_the_word_a_triple_click_the_paragraph_and_ctrl_double_click_shows_the_source() {
    // #38: a double-click jumped to the source editor.
    let start = "First paragraph.\n\nSecond one here.\n";
    let harness = || {
        let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
        assert!(s.execute("file.write", &json!({"path": "main.tex", "text": start})).is_ok());
        let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
        // Short frames, so the clicks come quickly enough to count together.
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(0.02).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
        h.run();
        h.get_by_label("Formatted").click();
        h.run();
        // Later than a double-click's delay after the clock's start, which egui would take for
        // an earlier click.
        h.run_steps(30);
        let r = h.get_by_role_and_label(egui::accesskit::Role::Paragraph, "Second one here.").rect();
        // Inside the first word, past its first letter, wherever the text is aligned.
        let at = egui::pos2(r.left() + 20.0, r.center().y);
        (h, at)
    };

    let (mut h, at) = harness();
    clicks(&mut h, at, 2, egui::Modifiers::NONE);
    assert!(h.query_by_label("main.tex formatted").is_some(), "the double-click left the formatted view");
    h.event(egui::Event::Text("X".into()));
    h.run();
    assert_eq!(text(&mut h), start.replacen("Second", "X", 1), "the double-click did not select the word");

    let (mut h, at) = harness();
    clicks(&mut h, at, 3, egui::Modifiers::NONE);
    h.event(egui::Event::Text("X".into()));
    h.run();
    assert_eq!(text(&mut h), "First paragraph.\n\nX\n", "the triple-click did not select the paragraph");

    let (mut h, at) = harness();
    clicks(&mut h, at, 2, egui::Modifiers::COMMAND);
    assert!(h.query_by_label("main.tex formatted").is_none(), "Ctrl+double-click did not show the source");
}

#[test]
fn show_in_source_takes_the_selection_to_the_source_editor() {
    let mut h = formatted("Some words.\n");
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "edit.select_all");
    h.run();
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "view.show_in_source");
    // The line flashes in the source for a moment.
    h.run_steps(8);
    assert!(h.query_by_label("main.tex formatted").is_none(), "the formatted view is still shown");
    h.event(egui::Event::Text("X".into()));
    h.run_steps(2);
    assert_eq!(text(&mut h), "X\n", "the selection did not come along to the source");
}

fn keys(h: &mut Harness<'_, App>, modifiers: egui::Modifiers, k: egui::Key, times: usize) {
    for _ in 0..times {
        key(h, modifiers, k);
    }
}

#[test]
fn deleting_selected_items_removes_them_and_typing_over_two_joins_them() {
    // #48: the items' text went, their empty items stayed.
    let start = "Before.\n\n\\begin{itemize}\n  \\item one\n  \\item two\n\\end{itemize}\n\nAfter.\n";
    let mut h = formatted(start);
    // To the start of "one", then to the end of "two".
    keys(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight, 8);
    keys(&mut h, egui::Modifiers::SHIFT, egui::Key::ArrowRight, 7);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    let out = text(&mut h);
    assert!(!out.contains("itemize") && !out.contains("\\item"), "the list or its items stayed: {out:?}");
    assert!(out.starts_with("Before.\n\n") && out.ends_with("\n\nAfter.\n"), "{out:?}");

    // From inside "one" to inside "two", typed over: one item, the selection's text replaced.
    let mut h = formatted(start);
    keys(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight, 9);
    keys(&mut h, egui::Modifiers::SHIFT, egui::Key::ArrowRight, 5);
    h.event(egui::Event::Text("X".into()));
    h.run();
    assert_eq!(text(&mut h), start.replace("one\n  \\item two", "oXo"));
}

#[test]
fn three_dots_typed_become_an_ellipsis_that_undo_takes_back() {
    // #45.
    let mut h = formatted("Wait\n");
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    for _ in 0..3 {
        h.event(egui::Event::Text(".".into()));
        h.run();
    }
    assert_eq!(text(&mut h), "Wait\\dots\n");
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Paragraph, "Wait…").is_some(), "the view does not show an ellipsis");
    // A word typed after it stays a word, not part of the command's name.
    h.event(egui::Event::Text("x".into()));
    h.run();
    assert_eq!(text(&mut h), "Wait\\dots{}x\n");
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Paragraph, "Wait…x").is_some());
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(text(&mut h), "Wait\\dots\n", "undo took back more than the word");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(text(&mut h), "Wait...\n", "undo did not bring the dots back");
}
