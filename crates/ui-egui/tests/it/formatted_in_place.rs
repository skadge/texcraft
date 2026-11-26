//! Editing in place in the formatted view: a block shown as source opens in a source editor where
//! it stands, by the pointer or the keyboard; a formula opens with its typeset preview under it;
//! leaving the editor changes the bytes that were typed and nothing else.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::ui_cmd;

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn formatted(main: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": main})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h
}

fn key(h: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(modifiers, key);
    h.run();
    h.run();
}

fn focused(h: &Harness<'_, App>, label: &str) -> bool {
    h.query_by_label(label).is_some_and(|n| n.accesskit_node().is_focused())
}

const TIKZ: &str = "Before.\n\n\\begin{tikzpicture}\n  \\draw (0,0) -- (1,1);\n\\end{tikzpicture}\n\nAfter.\n";

#[test]
fn a_block_shown_as_source_is_edited_where_it_stands() {
    let mut h = formatted(TIKZ);
    let block = "LaTeX: the tikzpicture environment";
    // Its title says what it is; a click opens the editor, which takes the keyboard.
    h.get_by_role_and_label(Role::Code, &format!("{block}. LaTeX source: {}", TIKZ[9..TIKZ.len() - 9].trim_end())).click();
    h.run();
    h.run();
    assert!(focused(&h, block), "the editor did not take the keyboard");
    // The end of the second line.
    for _ in 0..4 {
        key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowUp);
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    h.get_by_label(block).type_text("\n  \\fill (0,0) circle (1pt);");
    h.run();
    // Nothing changes until the editor is left; Escape leaves it and gives the text the keyboard.
    assert_eq!(text(&mut h), TIKZ);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    h.run();
    assert_eq!(text(&mut h), TIKZ.replace("(1,1);\n", "(1,1);\n  \\fill (0,0) circle (1pt);\n"));
    assert!(focused(&h, "main.tex formatted"), "the formatted text did not get the keyboard back");
    // Undo takes the whole edit back.
    ui_cmd::run(h.state_mut(), "edit.undo");
    h.run();
    h.run();
    assert_eq!(text(&mut h), TIKZ);
}

#[test]
fn enter_on_a_block_shown_as_source_opens_its_editor_from_the_keyboard() {
    let mut h = formatted(TIKZ);
    let code = h.query_all_by_role(Role::Code).next().map(|n| n.accesskit_node().label().unwrap_or_default()).unwrap_or_default();
    assert!(code.starts_with("LaTeX: the tikzpicture environment"), "{code}");
    h.get_by_role_and_label(Role::Code, &code).focus();
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    let block = "LaTeX: the tikzpicture environment";
    assert!(focused(&h, block), "Enter did not open the editor");
    // Typing replaces nothing else; leaving changes the byte typed alone.
    for _ in 0..4 {
        key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowUp);
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0.."\\begin{tikzpicture}\n  \\draw (0,0) -- (".len() {
        key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Delete);
    h.get_by_label(block).type_text("2");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(text(&mut h), TIKZ.replace("(1,1)", "(2,1)"));
}

#[test]
fn a_command_shown_as_source_in_a_paragraph_opens_under_it_by_enter_once_selected() {
    let original = "A \\mystery{thing} here.\n";
    let mut h = formatted(original);
    h.get_by_label("main.tex formatted").focus();
    h.run();
    // Home, past "A " and the run: Backspace selects the run, Enter edits it.
    key(&mut h, egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0..3 {
        key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    let editor = "LaTeX: a command the view does not show";
    assert!(focused(&h, editor), "Enter on the selected run did not open its editor");
    assert_eq!(text(&mut h), original, "Enter changed the text");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::End);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    h.get_by_label(editor).type_text("s}");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(text(&mut h), "A \\mystery{things} here.\n");
}

#[test]
fn a_formula_opens_with_its_preview_and_a_new_one_is_typed_with_it() {
    let original = "\\documentclass{article}\n\\begin{document}\nSee\n\\[ a + b \\]\nthere.\n\\end{document}\n";
    let mut h = formatted(original);
    // Typeset, an image named by its source; until then, its source.
    let focus = |h: &Harness<'_, App>| match h.query_by_role_and_label(Role::Image, "a + b") {
        Some(n) => {
            n.focus();
            true
        }
        None => h.query_by_role_and_label(Role::Math, "Formula: a + b").map(|n| n.focus()).is_some(),
    };
    assert!(focus(&h), "the formula is not shown");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    assert!(focused(&h, "Formula (LaTeX)"), "Enter on the formula did not open its editor");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::End);
    h.get_by_label("Formula (LaTeX)").type_text("c");
    h.run();
    // The preview typesets what is typed (in-process retex in the tests).
    let preview = |h: &Harness<'_, App>| h.query_by_role_and_label(Role::Image, "Preview of the formula").is_some();
    let started = std::time::Instant::now();
    while !preview(&h) && started.elapsed() < std::time::Duration::from_secs(120) {
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.run_steps(2);
    }
    assert!(preview(&h), "the formula's preview is not shown");
    key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(text(&mut h), original.replace("a + b ", "a + b c"));

    // The toolbar's formula asks in the same field, with its preview, and Enter inserts it.
    h.get_by_label("main.tex formatted").focus();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.math_inline");
    h.run();
    h.run();
    assert!(focused(&h, "The formula in LaTeX, such as x^2 + \\alpha"), "the new formula's field does not have the keyboard");
    h.get_by_label("The formula in LaTeX, such as x^2 + \\alpha").type_text("x^2");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.run();
    assert!(text(&mut h).contains("\\(x^2\\)"), "{}", text(&mut h));
}
