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
