use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use texcraft_compile::{Backend, Event, Job, Outcome, Progress, Worker};
use texcraft_project::ProjectPath;

#[test]
fn a_pass_expects_as_many_pages_as_the_pass_before() {
    let mut p = Progress::default();
    p.apply(Event::Pass { n: 1, reason: None });
    p.apply(Event::Pages { shipped: 12, expected: None });
    assert_eq!((p.pages, p.expected), (12, None));
    p.apply(Event::Pass { n: 2, reason: Some("cross-references".to_owned()) });
    assert_eq!((p.pass, p.pages, p.expected), (2, 0, Some(12)));
    p.apply(Event::Pages { shipped: 3, expected: None });
    assert_eq!((p.pages, p.expected), (3, Some(12)));
}

/// Reports two events, then waits until the test lets it finish.
struct Reporting(Arc<Mutex<Receiver<()>>>);

impl Backend for Reporting {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, "test")
    }
    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        report(Event::Pass { n: 1, reason: None });
        report(Event::Pages { shipped: 4, expected: None });
        if let Ok(rx) = self.0.lock() {
            let _ = rx.recv();
        }
        self.compile(job)
    }
}

#[test]
fn the_worker_passes_a_running_builds_progress_on() {
    let (go, wait) = channel();
    let mut worker = Worker::new(Box::new(Reporting(Arc::new(Mutex::new(wait)))));
    let job = Job { files: Vec::new(), main: ProjectPath::new("main.tex").unwrap(), id: 7 };
    worker.submit(job);
    let mut seen = None;
    for _ in 0..200 {
        if let Some((p, _)) = worker.progress().filter(|(p, _)| p.pages == 4) {
            seen = Some(p);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(seen.map(|p| (p.pass, p.pages)), Some((1, 4)), "the build's progress did not arrive while it ran");
    go.send(()).unwrap();
    assert!(worker.wait().is_some());
    assert!(worker.progress().is_none(), "a finished build still reports progress");
}
