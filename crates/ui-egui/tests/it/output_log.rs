//! The build log's Accessibility and Print filters, and a print build's summary.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::{Backend, Diagnostic, Job, Outcome, Output, Preflight, PrintCheck, Severity, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn diagnostic(severity: Severity, message: &str, origin: Option<&str>, line: u32) -> Diagnostic {
    Diagnostic {
        severity,
        message: message.into(),
        origin: origin.map(str::to_owned),
        file: Some("main.tex".into()),
        line: Some(line),
        column: None,
        end_column: None,
        help: Vec::new(),
        fixes: Vec::new(),
        noticed: None,
    }
}

/// A print build with a low-resolution image placed on page 1, an accessibility finding and an
/// error of TeX's.
struct Print;

impl Backend for Print {
    fn name(&self) -> String {
        "print".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let check = PrintCheck {
            kind: "low-resolution".into(),
            title: "images of 150 ppi or more".into(),
            severity: Severity::Warning,
            page: 0,
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 200.0,
            count: 1,
            value: 5.0,
            limit: Some(150.0),
            detail: None,
            file: Some("main.tex".into()),
            line: Some(6),
            image: Some("low.png".into()),
        };
        Outcome {
            status: Status::Built,
            pdf: Some(Arc::new(b"%PDF".to_vec())),
            output: Output::Print,
            diagnostics: vec![
                diagnostic(Severity::Error, "Undefined control sequence", None, 3),
                diagnostic(Severity::Warning, "page 1: image low.png prints at 5 ppi, below 150 ppi", Some("print"), 6),
                diagnostic(Severity::Warning, "image `low.png` has no alternative text", Some("accessibility"), 6),
            ],
            preflight: Some(Preflight {
                standard: job.options.pdfx,
                pdfx: Some(Arc::new(b"%PDF-X".to_vec())),
                checks: vec![check],
                rules: Vec::new(),
            }),
            ..Outcome::unavailable(job.id, "")
        }
    }
}

#[test]
fn the_print_filter_lists_print_checks_under_their_summary_and_places_them_on_the_page() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Print));
    s.execute("file.new", &json!({"path": "other.tex"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    assert!(h.query_by_label("1 problem").is_some(), "the print build has no summary");
    assert!(h.query_by_label("Accessibility 1").is_some(), "no Accessibility filter");
    h.get_by_label("Print 1").click();
    h.run();
    assert!(h.query_by_label_contains("prints at 5 ppi").is_some(), "the print check is not listed");
    assert!(h.query_by_label("Undefined control sequence").is_none(), "the Print filter lists TeX's error");
    assert!(h.query_by_label_contains("alternative text").is_none(), "the Print filter lists an accessibility finding");
    h.get_by_label("Page 1").click();
    h.run();
    assert!(h.query_by_label("Build Log, 1 error").is_some(), "showing the check on its page did not go back to the PDF");
    Ok(())
}
