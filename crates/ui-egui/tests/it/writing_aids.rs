//! E11 and E12: a misspelled word's spelling and a grammar problem's replacement from the text's
//! context menu replace them, and the symbol palette inserts its command in a formula.

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
    harness_with(text, SpellingOptions { language: "en-GB".to_owned(), ..SpellingOptions::default() })
}

fn harness_with(text: &str, spelling: SpellingOptions) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_dictionary(Box::new(Fake));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
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

/// Runs frames until the grammar checker is done with the file (it works in the background).
fn until_checked(h: &mut Harness<'_, App>) {
    let start = std::time::Instant::now();
    h.run();
    while h.state_mut().session().grammar_busy() && start.elapsed() < std::time::Duration::from_secs(60) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run();
    }
    h.run();
}

#[test]
fn a_grammar_problem_is_underlined_apart_and_its_replacement_from_the_menu_keeps_the_latex() {
    // Spelling off: only the grammar problem is underlined.
    let spelling = SpellingOptions { on: false, language: "en-GB".to_owned(), ..SpellingOptions::default() };
    let mut h = harness_with("It is \\textbf{here}: this is an mistake.\n", spelling).unwrap();
    until_checked(&mut h);
    // F7 goes to it; the context menu there offers its replacement, Ignore and Turn Off This Rule.
    ui_cmd::run(h.state_mut(), "edit.spelling.next");
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    assert!(h.query_by_label("Turn Off This Rule").is_some() && h.query_by_label("Ignore").is_some(), "the grammar entries are missing");
    h.get_by_label("Change to “a”").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), "It is \\textbf{here}: this is a mistake.\n");
}
