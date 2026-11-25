//! E11: a misspelled word's spelling from the text's context menu replaces it, and the symbol
//! palette inserts its command in a formula.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::project::ProjectPath;
use texcraft_engine::spelling::Dictionary;
use texcraft_engine::{Session, SpellingOptions};
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

/// Knows a few English words; offers "word" for "wrod".
struct Fake;

impl Dictionary for Fake {
    fn available(&self, lang: &str) -> Result<(), String> {
        if lang.starts_with("en") { Ok(()) } else { Err(format!("There is no spelling dictionary for {lang}.")) }
    }
    fn check(&self, word: &str, _: &str) -> bool {
        ["the", "is", "here", "let", "be", "word"].contains(&word.to_lowercase().as_str())
    }
    fn suggest(&self, word: &str, _: &str, _: usize) -> Vec<String> {
        if word == "wrod" { vec!["word".to_owned()] } else { Vec::new() }
    }
}

fn harness(text: &str) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_dictionary(Box::new(Fake));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    let spelling = SpellingOptions { language: "en-GB".to_owned(), ..SpellingOptions::default() };
    let prefs = Prefs { auto_compile: false, layout: "editor".to_owned(), spelling, ..Prefs::default() };
    Ok(Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs)))
}

fn text(h: &mut Harness<'_, App>) -> String {
    let main = ProjectPath::new("main.tex").ok();
    main.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn a_misspelled_words_spelling_from_the_context_menu_replaces_it() {
    let mut h = harness("The wrod is here.\n").unwrap();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.spelling.next");
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    assert!(h.get_by_label("Change to “word”").is_focused(), "the first spelling did not get the keyboard");
    assert!(h.query_by_label("Add to Dictionary").is_some(), "Add to Dictionary is not offered");
    h.get_by_label("Change to “word”").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), "The word is here.\n");
}

#[test]
fn the_palette_inserts_a_symbol_in_dollars_outside_math_and_bare_inside() {
    let mut h = harness("Let be.\n").unwrap();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.symbols");
    h.run();
    h.get_by_label("alpha \\alpha").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), "$\\alpha$Let be.\n");
    // The caret stays in the formula: the next symbol goes in it as it is.
    h.get_by_label("beta \\beta").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), "$\\alpha\\beta$Let be.\n");
}
