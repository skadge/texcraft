use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use serde_json::json;
use texcraft_compile::{Backend, Event, Job, Outcome, Status};
use texcraft_engine::Session;

/// Writes a draft after its first pass, then waits for the test to say how the build ends.
struct Drafting(Arc<Mutex<Receiver<Status>>>);

impl Backend for Drafting {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, "test")
    }
    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        report(Event::Pass { n: 1, reason: None });
        report(Event::Draft { pass: 1, pdf: Arc::new(format!("draft of {}", job.id).into_bytes()) });
        report(Event::Pass { n: 2, reason: Some("cross-references".to_owned()) });
        let status = self.0.lock().ok().and_then(|rx| rx.recv().ok()).unwrap_or(Status::Failed);
        let pdf = (status == Status::Built).then(|| Arc::new(format!("final of {}", job.id).into_bytes()));
        Outcome { status, pdf, ..Outcome::unavailable(job.id, "") }
    }
}

/// Polls `s` until `done` says so.
fn until(s: &mut Session, mut done: impl FnMut(&mut Session) -> bool) -> bool {
    for _ in 0..400 {
        if done(s) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

fn shown(s: &Session) -> Option<String> {
    s.pdf().map(|p| String::from_utf8_lossy(p).into_owned())
}

#[test]
fn a_draft_is_shown_while_later_passes_run_and_never_after_the_build() {
    let (end, ends) = channel();
    let mut s = Session::with_backend(Box::new(Drafting(Arc::new(Mutex::new(ends)))));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}"})).unwrap();
    s.execute("file.set_main", &json!({"path": "main.tex"})).unwrap();

    // A build that ends with a PDF: the draft while it runs, the final PDF after.
    s.execute("build.compile", &json!({})).unwrap();
    assert!(until(&mut s, |s| s.draft_pdf().is_some()), "the running build's draft did not arrive");
    let draft = s.draft_pdf().map(|d| (d.pass, String::from_utf8_lossy(&d.pdf).into_owned()));
    assert_eq!(draft, Some((1, "draft of 1".to_owned())));
    assert_eq!(shown(&s), None, "the draft was taken for the build's PDF");
    let result = s.execute("build.result", &json!({})).unwrap();
    assert_eq!(result["draft"]["pass"], 1, "agents do not see the draft: {result}");
    assert_eq!(s.progress().map(|(p, _)| (p.pass, p.draft)), Some((2, Some(1))), "the next pass lost the draft");

    end.send(Status::Built).unwrap();
    assert!(until(&mut s, |s| s.poll().is_some()), "the build did not end");
    assert!(s.draft_pdf().is_none(), "the draft outlived its build");
    assert_eq!(shown(&s).as_deref(), Some("final of 1"));

    // A build that fails after its draft: the draft goes, the last PDF stays.
    s.execute("build.compile", &json!({})).unwrap();
    assert!(until(&mut s, |s| s.draft_pdf().is_some_and(|d| d.job != 1)), "the second build's draft did not arrive");
    end.send(Status::Failed).unwrap();
    assert!(until(&mut s, |s| s.poll().is_some()), "the build did not end");
    assert!(s.draft_pdf().is_none(), "a failed build's draft stayed");
    assert_eq!(shown(&s).as_deref(), Some("final of 1"));
}
