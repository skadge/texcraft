//! The running build's progress beside the PDF (B7).

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use texcraft_compile::{Backend, Event, Job, Outcome};
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

/// Reaches page 3 of 7 of its second pass, then waits for the test.
struct Paused(Arc<Mutex<Receiver<()>>>);

impl Backend for Paused {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, "test")
    }
    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        report(Event::Pass { n: 1, reason: None });
        report(Event::Pages { shipped: 7, expected: None });
        report(Event::Pass { n: 2, reason: None });
        report(Event::Pages { shipped: 3, expected: Some(7) });
        let _ = self.0.lock().ok().and_then(|rx| rx.recv().ok());
        Outcome::unavailable(job.id, "test")
    }
}

const SHOWN: &str = "Compiling page 3 of 7 · pass 2";

/// Runs frames until `done` (the build runs on its own thread).
fn until(h: &mut Harness<'_, App>, done: impl Fn(&Harness<'_, App>) -> bool) -> bool {
    for _ in 0..400 {
        h.step();
        if done(h) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn the_page_being_built_shows_in_the_pdf_pane_while_the_build_runs() {
    let (resume, paused) = channel();
    let s = Session::with_backend(Box::new(Paused(Arc::new(Mutex::new(paused)))));
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    ui_cmd::run(harness.state_mut(), "view.logs");
    ui_cmd::run(harness.state_mut(), "build.compile");
    // In the status bar (announced) and beside the PDF (not announced again).
    assert!(until(&mut harness, |h| h.query_all_by_label(SHOWN).count() >= 2), "the progress is not shown in both places");
    let live = harness.query_all_by_label(SHOWN).filter(|n| n.accesskit_node().live() != egui::accesskit::Live::Off).count();
    assert_eq!(live, 1, "the progress is announced twice");
    resume.send(()).ok();
    assert!(until(&mut harness, |h| h.query_all_by_label_contains("Compiling page").count() == 0), "the progress stayed after the build");
}
