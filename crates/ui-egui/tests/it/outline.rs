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
        // Opening an entry scrolls the editor to its line, which takes six frames to settle.
        .with_max_steps(10)
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

/// SyncTeX data written by hand for `MAIN`: its line 3 (Intro) on the first page, its line 6
/// (End) low on the second.
const SYNCTEX: &str = "SyncTeX Version:1
Input:1:/tmp/texcraft-build/src/./main.tex
Output:pdf
Magnification:1000
Unit:1
X Offset:0
Y Offset:0
Content:
{1
[1,3:0,0:39000000,50000000,0
(1,3:4736286,8865054:22609920,462029,135004
h1,3:4736286,8865054:983040,0,0
)
]
}1
{2
[1,6:0,0:39000000,50000000,0
(1,6:4736286,40000000:22609920,462029,135004
h1,6:4736286,40000000:983040,0,0
)
]
}2
Postamble:
Count:6
Post scriptum:
";

/// The sample PDF of two pages, with `SYNCTEX`.
struct Synced;

impl texcraft_compile::Backend for Synced {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &texcraft_compile::Job) -> texcraft_compile::Outcome {
        texcraft_compile::Outcome {
            status: texcraft_compile::Status::Built,
            pdf: Some(std::sync::Arc::new(crate::pdf_preview::sample::links_pdf())),
            synctex: Some(std::sync::Arc::new(SYNCTEX.as_bytes().to_vec())),
            pages: 2,
            ..texcraft_compile::Outcome::unavailable(job.id, "")
        }
    }
}

fn pdf_scroll(h: &mut Harness<'_, App>) -> f64 {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["preview"]["scroll"][1].as_f64().unwrap_or_default()
}

/// Issue #30: an entry chosen in the outline shows its heading in the PDF too, once built.
#[test]
fn choosing_an_entry_scrolls_the_pdf_to_its_heading_too() {
    let mut s = Session::with_backend(Box::new(Synced));
    for (path, text) in [("chapters/one.tex", ONE), ("main.tex", MAIN)] {
        let _ = s.execute("file.new", &json!({"path": path}));
        assert!(s.execute("file.write", &json!({"path": path, "text": text})).is_ok());
    }
    assert!(s.execute("file.set_main", &json!({"path": "main.tex"})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    assert!(s.execute("build.compile", &json!({"wait": true})).is_ok());
    let prefs = Prefs { auto_compile: false, side: "outline".to_owned(), layout: "split".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_max_steps(10)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    for _ in 0..40 {
        h.step();
        if h.query_by_label_contains("1 / 2").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(h.query_by_label_contains("1 / 2").is_some(), "the PDF is not shown");
    let before = pdf_scroll(&mut h);
    let end = rows(&h).into_iter().map(|(l, _)| l).find(|l| l.ends_with("End")).unwrap_or_default();
    h.get_by_role_and_label(Role::TreeItem, &end).click();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(caret_line(&mut h), 6, "the editor did not go to the heading");
    assert!(pdf_scroll(&mut h) > before + 100.0, "the PDF did not scroll to the heading: {before} then {}", pdf_scroll(&mut h));
}
