use std::sync::{Arc, Mutex};

use serde_json::json;
use texcraft_compile::{Backend, Job, Options, Outcome};
use texcraft_engine::Session;

/// Records the options of the jobs it is given.
struct Recording(Arc<Mutex<Vec<Options>>>);

impl Backend for Recording {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(job.options);
        }
        Outcome::unavailable(job.id, "")
    }
}

#[test]
fn build_options_reach_the_engine_and_a_change_asks_for_a_new_build() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_backend(Box::new(Recording(seen.clone())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}"})).unwrap();
    s.execute("file.set_main", &json!({"path": "main.tex"})).unwrap();
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    assert!(!s.is_stale());

    let set = s.execute("build.options", &json!({"engine": "pdflatex", "draft_images": true})).unwrap();
    assert_eq!(
        (set["engine"].as_str(), set["draft_images"].as_bool(), set["halt_on_error"].as_bool()),
        (Some("pdflatex"), Some(true), Some(false)),
        "{set}"
    );
    assert!(s.is_stale(), "a changed option did not ask for a new build");
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    let last = seen.lock().unwrap().last().copied().unwrap();
    assert_eq!(last.engine, texcraft_compile::Engine::PdfLatex);
    assert!(last.draft_images && !last.halt_on_error);

    assert!(s.execute("build.options", &json!({"engine": "xetex"})).is_err(), "an unknown engine was taken");
    assert!(s.execute("build.clean", &json!({})).is_err(), "an engine without build files said it deleted them");
}

#[test]
fn the_kind_of_pdf_and_print_options_round_trip_and_reach_the_engine() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_backend(Box::new(Recording(seen.clone())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}"})).unwrap();
    s.execute("file.set_main", &json!({"path": "main.tex"})).unwrap();
    let set = s.execute("build.options", &json!({"output": "print", "pdfx": "x1a", "bleed_mm": 5, "crop_marks": false})).unwrap();
    assert_eq!(
        (set["output"].as_str(), set["pdfx"].as_str(), set["bleed_mm"].as_f64(), set["crop_marks"].as_bool()),
        (Some("print"), Some("x1a"), Some(5.0), Some(false)),
        "{set}"
    );
    // Kept as the preferences keep them.
    let stored: texcraft_compile::Options = serde_json::from_value(set).unwrap();
    assert_eq!(stored, s.settings.build);
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    let last = seen.lock().unwrap().last().copied().unwrap();
    assert_eq!((last.output, last.pdfx), (texcraft_compile::Output::Print, texcraft_compile::PdfX::X1a));

    for bad in [json!({"output": "paper"}), json!({"bleed_mm": -1}), json!({"bleed_mm": 100}), json!({"pdfx": "x3"})] {
        assert!(s.execute("build.options", &bad).is_err(), "{bad} was taken");
    }
    // Preferences saved before these options existed still read, with the defaults.
    let old: texcraft_compile::Options = serde_json::from_value(json!({"engine": "auto", "draft_images": true})).unwrap();
    assert_eq!(old.output, texcraft_compile::Output::Screen);
    assert!(old.draft_images);
}

/// Builds a PDF and, for a print build, a PDF/X beside it with a problem.
struct Printing;

impl Backend for Printing {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let print = job.options.output == texcraft_compile::Output::Print;
        let problem = texcraft_compile::Diagnostic {
            severity: texcraft_compile::Severity::Warning,
            message: "page 1: an image prints at 5 ppi".to_owned(),
            origin: Some("print".to_owned()),
            file: None,
            line: None,
            column: None,
            end_column: None,
            help: Vec::new(),
            fixes: Vec::new(),
        };
        Outcome {
            status: texcraft_compile::Status::Built,
            pdf: Some(Arc::new(b"%PDF screen".to_vec())),
            output: job.options.output,
            diagnostics: if print { vec![problem] } else { Vec::new() },
            preflight: print.then(|| texcraft_compile::Preflight {
                standard: job.options.pdfx,
                pdfx: Some(Arc::new(b"%PDF press".to_vec())),
                checks: Vec::new(),
                rules: Vec::new(),
            }),
            ..Outcome::unavailable(job.id, "")
        }
    }
}

#[test]
fn a_print_build_saves_its_pdfx_and_reports_its_problems() {
    let dir = std::env::temp_dir().join(format!("texcraft-save-pdfx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_backend(Box::new(Printing));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}"})).unwrap();
    s.execute("file.set_main", &json!({"path": "main.tex"})).unwrap();
    s.execute("build.options", &json!({"output": "print"})).unwrap();
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    let result = s.execute("build.result", &json!({})).unwrap();
    assert_eq!((result["print"]["ready"].as_bool(), result["print"]["problems"].as_u64()), (Some(false), Some(1)), "{result}");
    let path = dir.join("out.pdf");
    let saved = s.execute("build.save_pdf", &json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"%PDF press", "the print build did not save its PDF/X");
    assert_eq!(saved["kind"].as_str(), Some("PDF/X-4"));

    s.execute("build.options", &json!({"output": "screen"})).unwrap();
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    s.execute("build.save_pdf", &json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"%PDF screen", "a screen build saved the last print build's PDF/X");
    assert!(s.execute("build.result", &json!({})).unwrap()["print"].is_null());
    let _ = std::fs::remove_dir_all(&dir);
}
