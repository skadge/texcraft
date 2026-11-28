//! Copying the build log's messages.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::{Backend, Diagnostic, Job, Outcome, Severity, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

/// A build that fails with one error placed in the source.
struct Failing;

impl Backend for Failing {
    fn name(&self) -> String {
        "failing".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let mut outcome = Outcome::unavailable(job.id, "");
        outcome.status = Status::Failed;
        outcome.log = "raw log line".into();
        outcome.diagnostics = vec![Diagnostic {
            severity: Severity::Error,
            message: "Undefined control sequence".into(),
            origin: None,
            file: Some("main.tex".into()),
            line: Some(3),
            column: None,
            end_column: None,
            help: Vec::new(),
            fixes: Vec::new(),
            noticed: None,
        }];
        outcome
    }
}

fn copied(h: &Harness<'_, App>) -> Vec<String> {
    h.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_message_copies_with_its_place_and_its_place_still_opens_it() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Failing));
    s.execute("file.new", &json!({"path": "other.tex"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    h.get_by_label("Copy the message").click();
    h.step();
    let text = copied(&h);
    assert!(text.iter().any(|t| t.starts_with("main.tex:3:") && t.contains("Undefined control sequence")), "{text:?}");
    h.get_by_label("main.tex:3").click();
    h.run_steps(10);
    let current = h.state().session().current().map(|p| p.as_str().to_owned());
    assert_eq!(current.as_deref(), Some("main.tex"), "the place opens the file");
    Ok(())
}

/// A build whose error starts in an included file and was noticed in main.tex.
struct NoticedElsewhere;

impl Backend for NoticedElsewhere {
    fn name(&self) -> String {
        "noticed".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let mut outcome = Outcome::unavailable(job.id, "");
        outcome.status = Status::Failed;
        outcome.diagnostics = vec![Diagnostic {
            severity: Severity::Error,
            message: "\\begin{itemize} is never closed".into(),
            origin: None,
            file: Some("sections/intro.tex".into()),
            line: Some(3),
            column: None,
            end_column: None,
            help: Vec::new(),
            fixes: Vec::new(),
            noticed: Some(texcraft_compile::Place { file: "main.tex".into(), line: Some(6) }),
        }];
        outcome
    }
}

#[test]
fn a_message_that_starts_elsewhere_links_both_places_and_the_log_button_says_its_count() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(NoticedElsewhere));
    s.execute("file.new", &json!({"path": "other.tex"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Build Log, 1 error").click();
    h.run();
    let _ = h.get_by_label("sections/intro.tex:3");
    h.get_by_label("Copy the message").click();
    h.step();
    let text = copied(&h);
    assert!(text.iter().any(|t| t.starts_with("sections/intro.tex:3:") && t.contains("noticed at main.tex:6")), "{text:?}");
    h.get_by_label("TeX noticed it at main.tex:6").click();
    h.run_steps(10);
    let current = h.state().session().current().map(|p| p.as_str().to_owned());
    assert_eq!(current.as_deref(), Some("main.tex"), "where TeX noticed it opens");
    Ok(())
}
