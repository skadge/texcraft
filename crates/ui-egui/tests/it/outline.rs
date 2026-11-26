//! The Outline of the whole document (docs/ROADMAP.md, E14): the headings of the main file and
//! of the files it includes in one tree, numbered as the PDF numbers them, the shown file's part
//! told apart from the others, a click or Enter opening an entry's file at its line, and an
//! include that is not in the project said in words.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

const MAIN: &str =
    "\\documentclass{article}\n\\begin{document}\n\\section{Intro}\n\\input{chapters/one}\n\\input{gone}\n\\section{End}\n\\end{document}\n";
const ONE: &str = "\\section{Method}\nText.\n\\subsection{Detail}\n";

fn harness() -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for (path, text) in [("chapters/one.tex", ONE), ("main.tex", MAIN)] {
        let _ = s.execute("file.new", &json!({"path": path}));
        assert!(s.execute("file.write", &json!({"path": path, "text": text})).is_ok());
    }
    assert!(s.execute("file.set_main", &json!({"path": "main.tex"})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let prefs = Prefs { auto_compile: false, side: "outline".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    for _ in 0..3 {
        h.run();
    }
    h
}

fn shown_file(h: &mut Harness<'_, App>) -> Option<String> {
    h.state_mut().session().current().map(|p| p.as_str().to_owned())
}

fn caret_line(h: &mut Harness<'_, App>) -> u64 {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["cursor"]["line"].as_u64().unwrap_or(0)
}

/// The outline's rows, top to bottom: name and description.
fn rows(h: &Harness<'_, App>) -> Vec<(String, String)> {
    let mut found: Vec<(f32, String, String)> = h
        .query_all_by_role(Role::TreeItem)
        .filter(|n| n.accesskit_node().description().is_some_and(|d| d.starts_with("in ")))
        .map(|n| (n.rect().top(), n.accesskit_node().label().unwrap_or_default(), n.accesskit_node().description().unwrap_or_default()))
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, l, d)| (l, d)).collect()
}

#[test]
fn the_outline_is_the_whole_document_s_with_the_shown_file_s_part_marked() {
    let mut h = harness();
    let names: Vec<String> = rows(&h).into_iter().map(|(l, _)| l).collect();
    let order: Vec<&str> = names.iter().map(|n| n.rsplit("  ").next().unwrap_or_default()).collect();
    assert_eq!(order, ["Intro", "Method", "Detail", "End"], "the included file's headings are not in their place: {names:?}");
    assert!(names[3].starts_with('3'), "the headings after an include are numbered after its: {names:?}");
    let marked =
        |h: &Harness<'_, App>, title: &str| rows(h).into_iter().find(|(l, _)| l.ends_with(title)).is_some_and(|(_, d)| d.contains("file shown"));
    assert!(marked(&h, "Intro") && !marked(&h, "Method"), "the shown file's part is not told apart: {:?}", rows(&h));
    assert!(h.query_by_label_contains("gone is not among the project's files").is_some(), "a missing include is not said");
    // Another file's entry opens it at the entry's line, and its part is marked now.
    let detail = names.iter().find(|n| n.ends_with("Detail")).cloned().unwrap_or_default();
    h.get_by_role_and_label(Role::TreeItem, &detail).click();
    for _ in 0..3 {
        h.run();
    }
    assert_eq!(shown_file(&mut h).as_deref(), Some("chapters/one.tex"));
    assert_eq!(caret_line(&mut h), 3, "the file did not open at the heading's line");
    assert!(marked(&h, "Method") && !marked(&h, "Intro"), "the marking did not follow the shown file: {:?}", rows(&h));
}

#[test]
fn the_outline_is_walked_with_the_arrows_and_enter_opens_an_entry() {
    let mut h = harness();
    let first = rows(&h).first().map(|(l, _)| l.clone()).unwrap_or_default();
    h.get_by_role_and_label(Role::TreeItem, &first).focus();
    h.run();
    for _ in 0..2 {
        h.key_press(egui::Key::ArrowDown);
        h.run();
    }
    let focused = h.query_all_by_role(Role::TreeItem).find(|n| n.accesskit_node().is_focused()).and_then(|n| n.accesskit_node().label());
    assert!(focused.as_deref().is_some_and(|l| l.ends_with("Detail")), "Down did not move through the entries: {focused:?}");
    h.key_press(egui::Key::Enter);
    for _ in 0..3 {
        h.run();
    }
    assert_eq!(shown_file(&mut h).as_deref(), Some("chapters/one.tex"), "Enter did not open the entry's file");
    assert_eq!(caret_line(&mut h), 3);
}
