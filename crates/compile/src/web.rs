//! Builds in the browser: the editor's side of the engine's Web Worker ([`crate::web_engine`]).
//! [`WebWorker`] has the desktop [`Worker`](crate)'s interface: one build at a time, a newer
//! submission replacing a waiting one, progress and drafts while it runs, polled each frame.
//!
//! Messages to the worker: `{type: "build", id, main, names, contents, args}` (the project's
//! files as paths relative to the main document's folder and their bytes, retex's extra
//! arguments). From it: `{type: "line", id, text}` (a progress record), `{type: "draft", id,
//! pass, pdf}`, `{type: "done", id, code, output, pdf?, synctex?, seconds}` or `{type: "done",
//! id, error}`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, Worker};

use crate::records::{self, Record};
use crate::{Diagnostic, Draft, Engine, Event, Job, Outcome, Progress, Severity};

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn bytes(v: JsValue) -> Option<Vec<u8>> {
    v.dyn_into::<Uint8Array>().ok().map(|a| a.to_vec())
}

/// The running job's folder of the main document, in the project, which retex's file names
/// start from.
fn main_dir(job: &Job) -> Option<String> {
    job.main.as_str().rsplit_once('/').map(|(dir, _)| dir.to_owned())
}

pub struct WebWorker {
    worker: Option<Worker>,
    inbox: Rc<RefCell<VecDeque<JsValue>>>,
    /// Why there is no worker, when it could not start.
    error: Option<String>,
    running: Option<(u64, Option<String>)>,
    waiting: Option<Job>,
    progress: Progress,
    draft: Option<Draft>,
    started: f64,
    /// Whether the worker said it takes messages; until then the build to send waits here.
    ready: bool,
    unsent: Option<JsValue>,
    /// Notes for the log about options the browser build ignores.
    notes: Vec<String>,
    _onmessage: Option<Closure<dyn FnMut(MessageEvent)>>,
}

impl WebWorker {
    /// Starts the engine's worker from the script at `url` (the worker build's loader).
    pub fn new(url: &str) -> WebWorker {
        let inbox: Rc<RefCell<VecDeque<JsValue>>> = Rc::default();
        let sink = inbox.clone();
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| sink.borrow_mut().push_back(e.data()));
        let (worker, error) = match Worker::new(url) {
            Ok(w) => {
                w.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
                (Some(w), None)
            }
            Err(e) => (None, Some(format!("The typesetting engine could not start in this browser ({e:?})."))),
        };
        WebWorker {
            worker,
            inbox,
            error,
            running: None,
            waiting: None,
            progress: Progress::default(),
            draft: None,
            started: 0.0,
            ready: false,
            unsent: None,
            notes: Vec::new(),
            _onmessage: Some(onmessage),
        }
    }

    pub fn backend_name(&self) -> &str {
        "retex (in the browser)"
    }

    pub fn running(&self) -> Option<u64> {
        self.running.as_ref().map(|(id, _)| *id)
    }

    /// Starts `job` now, or after the running one (replacing a job already waiting).
    pub fn submit(&mut self, job: Job) {
        if self.running.is_some() {
            self.waiting = Some(job);
        } else {
            self.start(job);
        }
    }

    /// Drops a waiting job; the running one can't be stopped yet.
    pub fn cancel(&mut self) -> bool {
        self.waiting = None;
        false
    }

    fn start(&mut self, job: Job) {
        let Some(worker) = &self.worker else {
            self.running = Some((job.id, None));
            return;
        };
        let dir = main_dir(&job);
        let prefix = dir.as_ref().map(|d| format!("{d}/"));
        let names = Array::new();
        let contents = Array::new();
        let mut skipped = 0;
        for (path, data) in &job.files {
            // The module's files are those of the main document's folder: retex resolves
            // names from its current directory, `/`.
            let name = match &prefix {
                Some(p) => match path.as_str().strip_prefix(p.as_str()) {
                    Some(rest) => rest.to_owned(),
                    None => {
                        skipped += 1;
                        continue;
                    }
                },
                None => path.as_str().to_owned(),
            };
            names.push(&name.into());
            contents.push(&Uint8Array::from(&data[..]).into());
        }
        let main = job.main.as_str().rsplit('/').next().unwrap_or(job.main.as_str()).to_owned();
        let args = Array::new();
        match job.options.engine {
            Engine::Auto => {}
            Engine::LuaLatex => {
                args.push(&"--compat=lualatex".into());
            }
            Engine::PdfLatex => {
                args.push(&"--compat=pdflatex".into());
            }
        }
        if job.options.halt_on_error {
            args.push(&"--halt-on-error".into());
        }
        self.notes.clear();
        if job.options.draft_images {
            self.notes.push("Draft images are not available in the browser yet: images are drawn in full.".into());
        }
        if skipped > 0 {
            self.notes.push(format!("{skipped} file(s) outside the main document's folder were left out: the browser build only sees that folder."));
        }
        let msg = Object::new();
        for (k, v) in [
            ("type", JsValue::from("build")),
            ("id", JsValue::from(job.id as f64)),
            ("main", main.into()),
            ("names", names.into()),
            ("contents", contents.into()),
            ("args", args.into()),
        ] {
            let _ = Reflect::set(&msg, &k.into(), &v);
        }
        if !self.ready {
            self.unsent = Some(msg.into());
        } else if let Err(e) = worker.post_message(&msg) {
            self.error = Some(format!("The typesetting engine did not take the build ({e:?})."));
        }
        self.running = Some((job.id, dir));
        self.progress = Progress::default();
        self.draft = None;
        self.started = js_sys::Date::now();
    }

    /// Takes the messages that arrived; returns the running job's outcome when it ended.
    fn receive(&mut self) -> Option<Outcome> {
        let (id, dir) = self.running.clone()?;
        if let Some(error) = &self.error {
            return Some(Outcome::unavailable(id, error.clone()));
        }
        loop {
            let msg = self.inbox.borrow_mut().pop_front()?;
            match get(&msg, "type").as_string().as_deref() {
                Some("ready") => {
                    self.ready = true;
                    if let (Some(worker), Some(unsent)) = (&self.worker, self.unsent.take())
                        && let Err(e) = worker.post_message(&unsent)
                    {
                        self.error = Some(format!("The typesetting engine did not take the build ({e:?})."));
                    }
                    continue;
                }
                Some("log") => {
                    let text = get(&msg, "text").as_string().unwrap_or_default();
                    match get(&msg, "level").as_string().as_deref() {
                        Some("ERROR") => log::error!("engine: {text}"),
                        Some("WARN") => log::warn!("engine: {text}"),
                        _ => log::info!("engine: {text}"),
                    }
                    continue;
                }
                Some("crash") => {
                    let why = get(&msg, "error").as_string().unwrap_or_default();
                    // The worker is gone: later builds fail at once with the same reason.
                    self.error = Some(format!("The typesetting engine stopped unexpectedly: {why}"));
                    self.worker = None;
                    return Some(failure(id, &format!("The typesetting engine stopped unexpectedly: {why}")));
                }
                _ => {}
            }
            if get(&msg, "id").as_f64() != Some(id as f64) {
                continue;
            }
            match get(&msg, "type").as_string().as_deref() {
                Some("line") => match get(&msg, "text").as_string().and_then(|t| records::parse(&t, dir.as_deref())) {
                    Some(Record::Event(e)) => self.progress.apply(e),
                    Some(Record::Pass { n, reason }) => self.progress.apply(Event::Pass { n, reason }),
                    _ => {}
                },
                Some("draft") => {
                    if let Some(pdf) = bytes(get(&msg, "pdf")) {
                        let pass = get(&msg, "pass").as_f64().unwrap_or(1.0) as u32;
                        self.progress.apply(Event::Draft { pass, pdf: Arc::new(Vec::new()) });
                        self.draft = Some(Draft { job: id, pass, pdf: Arc::new(pdf) });
                    }
                }
                Some("done") => {
                    if let Some(error) = get(&msg, "error").as_string() {
                        return Some(failure(id, &error));
                    }
                    let output = get(&msg, "output").as_string().unwrap_or_default();
                    let seconds = get(&msg, "seconds").as_f64().unwrap_or(0.0);
                    let mut outcome = records::outcome(id, &output, dir.as_deref(), bytes(get(&msg, "pdf")), bytes(get(&msg, "synctex")), seconds);
                    for note in &self.notes {
                        outcome.log = format!("{note}\n{}", outcome.log);
                    }
                    return Some(outcome);
                }
                _ => {}
            }
        }
    }

    /// The running job's progress and how long it has run, in seconds.
    pub fn progress(&mut self) -> Option<(Progress, f64)> {
        self.running.as_ref()?;
        Some((self.progress.clone(), (js_sys::Date::now() - self.started) / 1000.0))
    }

    /// The running job's draft PDF.
    pub fn draft(&mut self) -> Option<&Draft> {
        self.running.as_ref()?;
        self.draft.as_ref()
    }

    /// A finished outcome, without blocking.
    pub fn poll(&mut self) -> Option<Outcome> {
        let outcome = self.receive()?;
        self.running = None;
        self.draft = None;
        if let Some(next) = self.waiting.take() {
            self.start(next);
        }
        Some(outcome)
    }
}

/// A build the engine could not run to its end: the reason, as an error.
fn failure(job: u64, message: &str) -> Outcome {
    let diagnostic = Diagnostic {
        severity: Severity::Error,
        message: message.to_owned(),
        origin: Some("retex".to_owned()),
        file: None,
        line: None,
        column: None,
        end_column: None,
        help: Vec::new(),
        fixes: Vec::new(),
    };
    Outcome { status: crate::Status::Failed, diagnostics: vec![diagnostic], ..Outcome::unavailable(job, "") }
}
