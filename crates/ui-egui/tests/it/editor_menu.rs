//! E7 and E8: the source's context menu, and selection commands on Vim mode's selections.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::{Identity, Prefs};
use texcraft_ui_egui::{App, ui_cmd};

fn harness(text: &str, vim: bool) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    let prefs = Prefs { auto_compile: false, vim, layout: "editor".to_owned(), identity: Some(Identity::new("Ada")?), ..Prefs::default() };
    Ok(Harness::builder().with_size(egui::vec2(1500.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs)))
}

/// Writes `text` in the open composer and posts it; the passage the new comment is on.
fn post(h: &mut Harness<'_, App>, text: &str) -> String {
    h.get_by_role_and_label(Role::MultilineTextInput, "Comment").type_text(text);
    h.run();
    h.get_by_role_and_label(Role::Button, "Comment").click();
    h.run();
    h.run();
    let threads = h.state_mut().session().review().threads().to_vec();
    assert_eq!(threads.len(), 1, "the comment was not posted");
    threads[0].anchor.quote.exact.clone()
}

#[test]
fn shift_f10_opens_the_texts_menu_with_the_keyboard_in_it_and_add_a_comment_comments_the_selection() {
    let mut h = harness("The quick brown fox.\n", false).unwrap();
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    assert!(h.query_by_label("Add a Comment").is_none(), "the menu is open before it was asked for");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    // Nothing is selected: no Add a Comment, and Cut (disabled) does not take the keyboard.
    assert!(h.query_by_label("Add a Comment").is_none(), "Add a Comment is offered without a selection");
    assert!(h.get_by_label_contains("Paste").is_focused(), "the menu's first available entry did not get the keyboard");
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert!(h.query_by_label_contains("Paste").is_none(), "Escape did not close the menu");
    assert!(h.get_by_label("main.tex source").is_focused(), "the keyboard did not go back to the text");

    ui_cmd::run(h.state_mut(), "edit.select_all");
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    assert!(h.get_by_label_contains("Cut").is_focused(), "with a selection, Cut is the entry with the keyboard");
    h.get_by_label_contains("Add a Comment").click();
    h.run();
    h.run();
    assert_eq!(post(&mut h, "Which fox?"), "The quick brown fox.\n");
}

#[test]
fn edit_comment_in_vim_mode_comments_visual_modes_selection_and_leaves_visual_mode() {
    let mut h = harness("The quick brown fox.\n", true).unwrap();
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    h.get_by_label("main.tex source").type_text("wve");
    h.run();
    assert!(h.query_by_label("VISUAL").is_some(), "v did not start Visual mode");
    ui_cmd::run(h.state_mut(), "edit.comment");
    h.run();
    assert!(h.query_by_label("NORMAL").is_some(), "Visual mode stayed after the comment was started");
    assert_eq!(post(&mut h, "Fast?"), "quick");
}
