//! Issue #21: the build log hides the messages of a kind (all the overfull boxes, say), says how
//! many it hides, and Settings › Building shows them again.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::{Backend, Diagnostic, Job, Outcome, Severity, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::{App, ui_cmd};

/// A build with two overfull boxes and an undefined reference.
struct Warning;

fn warning(message: &str, line: u32) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        message: message.into(),
        origin: None,
        file: Some("main.tex".into()),
        line: Some(line),
        column: None,
        end_column: None,
        help: Vec::new(),
        fixes: Vec::new(),
        noticed: None,
    }
}

impl Backend for Warning {
    fn name(&self) -> String {
        "warning".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let mut outcome = Outcome::unavailable(job.id, "");
        outcome.status = Status::Built;
        outcome.diagnostics = vec![
            warning("Overfull \\hbox (1.5pt too wide) in paragraph at lines 1--2", 1),
            warning("Overfull \\hbox (12.0pt too wide) in paragraph at lines 3--4", 3),
            Diagnostic { origin: Some("LaTeX".into()), ..warning("Reference `fig:a' on page 1 undefined on input line 2", 2) },
        ];
        outcome
    }
}

fn hidden(h: &mut Harness<'_, App>) -> u64 {
    h.state_mut().control("engine.execute", &json!({"command": "build.result"}))["result"]["hidden"].as_u64().unwrap_or(0)
}

#[test]
fn hiding_a_kind_hides_its_messages_and_settings_shows_them_again() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Warning));
    s.execute("file.write", &json!({"path": "main.tex", "text": "One.\nTwo.\nThree.\n"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    ui_cmd::run(h.state_mut(), "view.logs");
    h.run();
    assert!(h.query_by_label("Back to PDF").is_some());
    // The first overfull box's card: Hide Like This, in this project.
    h.query_all_by_label("Hide Like This").next().ok_or("no card offers to hide its kind")?.click();
    h.run();
    h.get_by_label("In This Project").click();
    h.run();
    h.run();
    assert_eq!(hidden(&mut h), 2, "both overfull boxes are not hidden");
    assert!(h.query_by_label("2 hidden messages").is_some(), "the log does not say how many it hides");
    assert_eq!(h.query_all_by_label("Hide Like This").count(), 1, "the hidden messages are still listed");
    ui_cmd::run(h.state_mut(), "view.logs");
    h.run();
    assert!(h.query_by_label("Build Log, 1 warning").is_some(), "the hidden warnings are still counted");
    ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": "building"}));
    h.run();
    h.run();
    let show = h.query_all_by_label_contains("Show Again: TeX: Overfull \\hbox").next().ok_or("Settings does not list the hidden kind")?;
    // From the keyboard: the button may be below the tab's fold.
    show.focus();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(hidden(&mut h), 0, "Show Again did not show them");
    Ok(())
}
