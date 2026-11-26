//! The formatted view for readers who do not know LaTeX (docs/ROADMAP.md, F5): references and
//! citations as the last build numbered them, included files in place, environments in words.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::{Backend, Job, Outcome, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::ui_cmd;

/// "Builds" by handing back the `.aux` retex writes for [`MAIN`] (amsmath, the plain style).
struct Numbered;

const AUX: &str = "\\relax\n\\bibcite{knuth}{1}\n\\newlabel{sec:model}{{2}{1}}\n\\newlabel{eq:motion}{{1}{1}}\n\\newlabel{eq:solution}{{2}{1}}\n\\newlabel{thm:small}{{1}{1}}\n";

impl Backend for Numbered {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome { status: Status::Built, pdf: Some(Arc::new(b"%PDF-1.7".to_vec())), aux: Some(Arc::from(AUX)), ..Outcome::unavailable(job.id, "") }
    }
}

const MAIN: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n\\newtheorem{theorem}{Theorem}\n\\begin{document}\n\\section{Introduction}\n\\input{sections/intro}\n\\section{Model}\\label{sec:model}\n\\begin{align}\n  a &= b \\label{eq:motion}\\\\\n  c &= d \\label{eq:solution}\n\\end{align}\n\\begin{theorem}[Small angles]\\label{thm:small}\nThe period does not depend on the amplitude.\n\\end{theorem}\nEquation~\\eqref{eq:solution} solves~\\eqref{eq:motion}, as \\cite{knuth} says.\n\\input{missing}\n\\end{document}\n";

const INTRO: &str = "A pendulum swings.\n\\input{sections/inner}\n";
const INNER: &str = "It swings back.\n";

/// A session with the project, its main file open.
fn session() -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Numbered));
    s.execute("file.write", &json!({"path": "main.tex", "text": MAIN}))?;
    for (path, text) in [("sections/intro.tex", INTRO), ("sections/inner.tex", INNER)] {
        s.execute("file.new", &json!({"path": path}))?;
        s.execute("file.write", &json!({"path": path, "text": text}))?;
    }
    s.execute("file.open", &json!({"path": "main.tex"}))?;
    Ok(s)
}

/// The app over `s`, the main file shown formatted.
fn harness(s: Session) -> Harness<'static, App> {
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run_steps(3);
    harness.get_by_label("Formatted").click();
    harness.run_steps(3);
    harness
}

fn paragraph_with(h: &Harness<'_, App>, words: &str) -> bool {
    h.query_all_by_role(egui::accesskit::Role::Paragraph)
        .any(|n| n.accesskit_node().label().is_some_and(|l| l.replace('\u{a0}', " ").contains(words)))
}

#[test]
fn references_show_their_key_and_why_until_a_build_numbers_them() {
    let mut harness = harness(session().unwrap());
    // Before a build: the key, with a question mark.
    assert!(paragraph_with(&harness, "eq:solution?"), "an unresolved reference does not show its key");
    assert!(!paragraph_with(&harness, "(2)"));
    ui_cmd::run(harness.state_mut(), "build.compile");
    for _ in 0..200 {
        harness.run_steps(2);
        if harness.state_mut().session().aux().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run_steps(3);
    assert!(paragraph_with(&harness, "Equation (2) solves (1), as [1] says."), "the references do not read as the PDF does");
    // The numbered parts read as they do in the PDF.
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Heading, "2\u{2002}Model").is_some(), "the heading has no number");
    assert!(harness.query_by_label_contains("(1) (2)").is_some(), "the formula's rows have no numbers");
    assert!(harness.query_by_label("Theorem 1 (Small angles).").is_some(), "the theorem is not named in words");
    assert!(harness.query_by_label_contains("align").is_none(), "the environment's name still shows");
}

#[test]
fn an_included_file_is_shown_in_place_and_opens_from_edit_here() {
    let mut harness = harness(session().unwrap());
    assert!(paragraph_with(&harness, "A pendulum swings."), "the included file's text is not shown");
    assert!(paragraph_with(&harness, "It swings back."), "a file it includes is not shown in its place");
    assert!(harness.query_by_label("sections/intro.tex").is_some(), "the included file is not named");
    assert!(harness.query_by_label_contains("missing is not among the project's files").is_some(), "a missing file is not said in words");
    // Its text is not the shown file's: the view's lines stay those of main.tex.
    harness.get_all_by_label("Edit Here").next().unwrap().click();
    harness.run_steps(4);
    assert_eq!(harness.state_mut().session().current().map(|p| p.as_str().to_owned()).as_deref(), Some("sections/intro.tex"));
    assert!(paragraph_with(&harness, "A pendulum swings."), "Edit Here does not show the file formatted");
}
