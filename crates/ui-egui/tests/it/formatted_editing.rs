//! Editing in the formatted view: keys make minimal edits of the source, shared with source mode.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
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

#[test]
fn an_elsarticle_front_matter_is_a_title_block_whose_text_is_edited_in_place() {
    // #44: the front matter was shown as its source.
    let fixture = include_str!("../../../latex/tests/fixtures/frontmatter.tex");
    let mut h = formatted(fixture);
    let title = "Scheduling Lighthouse Shifts by the Tides\u{2020}";
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Heading, title).is_some(), "the title is not a heading with its note's mark");
    assert!(h.query_by_label_contains("\\author").is_none(), "the front matter shows as its source");
    for shown in ["Ada Quill", "Harbour Institute, Port Example, Nowhere", "Corresponding author", "We plan the keepers", "lighthouses · tides"] {
        assert!(h.query_by_label_contains(shown).is_some(), "{shown:?} is not shown");
    }
    // The caret starts in the title, whose text is edited as text.
    h.event(egui::Event::Text("Re".into()));
    h.run();
    assert!(text(&mut h).contains("\\title{ReScheduling Lighthouse Shifts by the Tides\\tnoteref{t1}}"), "{}", text(&mut h));
}

/// The view's lines, as their text runs, in order.
fn line_texts(h: &Harness<'_, App>) -> Vec<String> {
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextRun)).map(|n| n.accesskit_node().value().unwrap_or_default()).collect()
}

#[test]
fn typing_over_a_selection_of_whole_lists_keeps_the_text_and_the_keyboard() {
    // The deletion left no line for the caret: the typed text was dropped and the keyboard left.
    let mut h = formatted("\\begin{itemize}\n\\item a\n\\item b\n\\end{itemize}\n");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("x".into()));
    h.run();
    assert!(text(&mut h).contains('x'), "the typed text went nowhere: {:?}", text(&mut h));
    assert!(h.get_by_label("main.tex formatted").accesskit_node().is_focused(), "the keyboard left the text");
}

#[test]
fn enter_at_the_end_of_a_documents_last_paragraph_shows_the_new_one() {
    let mut h = formatted("\\documentclass{article}\n\\begin{document}\nText.\n\\end{document}\n");
    let before = line_texts(&h).len();
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    assert!(line_texts(&h).len() > before, "no empty paragraph shows for the caret: {:?}", line_texts(&h));
    h.get_by_label("main.tex formatted").type_text("More");
    h.run();
    assert!(text(&mut h).contains("Text.\n\nMore"), "{:?}", text(&mut h));
}

#[test]
fn a_click_at_a_paragraphs_start_adds_no_empty_line_above_it() {
    let mut h = formatted("First paragraph.\n\nSecond one.\n");
    let before = line_texts(&h);
    let r = h.get_by_role_and_label(egui::accesskit::Role::Paragraph, "Second one.").rect();
    click(&mut h, egui::pos2(r.min.x + 1.0, r.center().y));
    assert_eq!(line_texts(&h), before, "the click made a line appear");
}

#[test]
fn backspace_after_a_logo_deletes_it_whole_and_undo_brings_it_back() {
    let original = "The \\LaTeX{} way\n";
    let mut h = formatted(original);
    // Right over "The " and the logo: one step for the logo.
    for _ in 0..5 {
        key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    assert_eq!(text(&mut h), "The  way\n", "Backspace did not delete the logo whole");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(text(&mut h), original, "undo did not bring the logo back");
}

#[test]
fn a_heading_made_from_a_list_item_leaves_the_list() {
    let mut h = formatted("\\begin{itemize}\n  \\item One\n  \\item Two\n\\end{itemize}\n");
    key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "edit.heading_1");
    h.run();
    let t = text(&mut h);
    let end = t.find("\\end{itemize}").unwrap_or(0);
    assert!(t.find("\\section{Two}").is_some_and(|s| s > end), "the heading is not after the list: {t:?}");
    let headings: Vec<String> =
        h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::Heading)).filter_map(|n| n.accesskit_node().label()).collect();
    assert!(headings.iter().any(|l| l.contains("Two")), "Two is not shown as a heading: {headings:?}");
}

#[test]
fn an_input_methods_deletion_of_several_characters_deletes_them_all() {
    // A phone's correction deletes the word before the caret, then types its replacement.
    let mut h = formatted("Hello there\n");
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    h.event(egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars: 5, after_chars: 0 }));
    h.run();
    assert_eq!(text(&mut h), "Hello \n", "not every character the input method deleted went");
}

#[test]
fn an_image_inserted_as_a_figure_leaves_the_caret_in_its_caption() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": "\\begin{document}\nText.\n\\end{document}\n"})).is_ok());
    assert!(s.execute("file.import", &json!({"path": "kiln.png", "data": "iVBORw0KGgo="})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    texcraft_ui_egui::ui_cmd::run_with(h.state_mut(), "edit.figure", &json!({"image": "kiln.png"}));
    h.run();
    h.get_by_label("main.tex formatted").type_text("A kiln");
    h.run();
    assert!(text(&mut h).contains("\\caption{A kiln}"), "the typed text did not go in the caption: {:?}", text(&mut h));
}

#[test]
fn typing_over_whole_list_items_is_one_undo_step() {
    // The items went in one step and the text typed came in another: two undos for one typing.
    let start = "Before.\n\n\\begin{itemize}\n  \\item one\n  \\item two\n\\end{itemize}\n\nAfter.\n";
    let mut h = formatted(start);
    keys(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight, 8);
    keys(&mut h, egui::Modifiers::SHIFT, egui::Key::ArrowRight, 7);
    for c in ["k", "i", "l", "n"] {
        h.event(egui::Event::Text(c.into()));
        h.run();
    }
    let typed = text(&mut h);
    assert!(typed.contains("kiln") && !typed.contains("\\item two"), "{typed:?}");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(text(&mut h), start, "one undo did not take the typing over the items back whole");
    key(&mut h, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    assert_eq!(text(&mut h), typed, "redo did not give the typing back whole");
}

#[test]
fn a_word_typed_over_a_selection_is_one_undo_step() {
    // The selection's replacement by the first letter was a step of its own: one undo left it.
    let start = "Fire the pots.\n";
    let mut h = formatted(start);
    keys(&mut h, egui::Modifiers::SHIFT, egui::Key::ArrowRight, 4);
    for c in ["k", "i", "l", "n"] {
        h.event(egui::Event::Text(c.into()));
        h.run();
    }
    assert_eq!(text(&mut h), "kiln the pots.\n");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(text(&mut h), start, "one undo did not take the word typed over the selection back whole");
}

#[test]
fn the_caret_at_a_labelled_headings_end_adds_no_empty_line_under_it() {
    // End on a heading with its label before a theorem drew an empty line under the heading for
    // the caret; a click on the text below then moved it under the pointer and lost the keyboard.
    let start = "\\chapter{Background}\n\\label{ch:bg}\n\n\\begin{theorem}\n  Every theorem is numbered.\n\\end{theorem}\n";
    let mut h = formatted(start);
    let before = line_texts(&h).len();
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    assert_eq!(line_texts(&h).len(), before, "End on the heading added a line: {:?}", line_texts(&h));
    h.event(egui::Event::Text("s".into()));
    h.run();
    assert!(text(&mut h).contains("\\chapter{Backgrounds}"), "the letter typed went elsewhere: {:?}", text(&mut h));
}

#[test]
fn a_click_that_moves_the_text_under_the_pointer_keeps_the_keyboard() {
    // The caret in the empty line Enter made after a list; a click on the paragraph below took the
    // caret out of that line, which went, so the paragraph moved up from under the pointer and the
    // release fell outside the text: the keyboard left it.
    let start = "\\begin{itemize}\n  \\item a\n\\end{itemize}\n\nAfter.\n";
    let mut h = formatted(start);
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    let view = "main.tex formatted";
    let after = |h: &Harness<'_, App>| crate::support::view_lines(h, view).into_iter().find(|(_, _, v)| v == "After.").map(|(_, r, _)| r);
    let Some(r) = after(&h) else { panic!("no line reads After.: {:?}", line_texts(&h)) };
    let at = egui::pos2(r.left() + 2.0, r.center().y);
    click(&mut h, at);
    assert!(after(&h).is_some_and(|moved| !moved.contains(at)), "the click did not move the line (the case this test is about)");
    assert!(h.get_by_label(view).accesskit_node().is_focused(), "the keyboard left the text");
    h.event(egui::Event::Text("x".into()));
    h.run();
    assert!(text(&mut h).contains("xAfter."), "the letter typed went elsewhere: {:?}", text(&mut h));
}

#[test]
fn bold_pressed_again_with_the_caret_in_a_paragraph_keeps_every_word() {
    // Seen once in a screenshot: a word typed after Bold, right before the next word, then Bold
    // pressed again to stop it made the two one bold word and left it selected, so the space
    // typed next replaced both.
    let mut h = formatted("One two three four.\n");
    let shown = |h: &Harness<'_, App>| line_texts(h).join("\n");
    keys(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight, 4);
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::B);
    h.event(egui::Event::Text("big".into()));
    h.run();
    assert!(text(&mut h).contains("One \\textbf{big}two"), "{:?}", text(&mut h));
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::B);
    h.event(egui::Event::Text(" ".into()));
    h.run();
    assert_eq!(shown(&h), "One big two three four.", "a word went: {:?}", text(&mut h));
    // In a word: Bold and Bold again change no word.
    keys(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight, 6);
    for _ in 0..2 {
        key(&mut h, egui::Modifiers::COMMAND, egui::Key::B);
        assert_eq!(shown(&h), "One big two three four.", "a word went: {:?}", text(&mut h));
    }
    h.event(egui::Event::Text("e".into()));
    h.run();
    assert_eq!(shown(&h), "One big two theree four.", "the letter typed did not go at the caret: {:?}", text(&mut h));
}
