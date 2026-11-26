//! Builds in the browser: the editor's side of the engine's Web Worker ([`crate::web_engine`]).
//! [`WebWorker`] has the desktop [`Worker`](crate)'s interface: one build at a time, a newer
//! submission replacing a waiting one, progress and drafts while it runs, polled each frame.
//!
//! Messages to the worker: `{type: "build", id, main, dir, names, contents, args, project}` (the
//! main document's name and folder, the project's files as their paths in the project and their
//! bytes, retex's extra arguments, which project it is: [`Job::project`]). From it: `{type: "line", id, text}` (a progress record), `{type: "draft", id,
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
use crate::{Diagnostic, Draft, Engine, Event, Exported, Job, Outcome, Progress, Severity};

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
    running: Option<(u64, Option<String>, crate::Options)>,
    /// The running job's files and main document, to place its errors where they start.
    sources: Option<Box<Job>>,
    waiting: Option<Job>,
    progress: Progress,
    draft: Option<Draft>,
    started: f64,
    /// Whether the worker said it takes messages; until then what to send waits here.
    ready: bool,
    unsent: Vec<JsValue>,
    /// The worker's messages about builds, apart from those about exports.
    builds: VecDeque<JsValue>,
    /// The export running (its message id), then its outcome: the files, or why there are none.
    export: Option<(f64, Option<Result<Exported, String>>)>,
    next_export: f64,
    /// The worker stopped: why.
    crashed: Option<String>,
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
            sources: None,
            waiting: None,
            progress: Progress::default(),
            draft: None,
            started: 0.0,
            ready: false,
            unsent: Vec::new(),
            builds: VecDeque::new(),
            export: None,
            next_export: 0.0,
            crashed: None,
            _onmessage: Some(onmessage),
        }
    }

    pub fn backend_name(&self) -> &str {
        "retex (in the browser)"
    }

    pub fn running(&self) -> Option<u64> {
        self.running.as_ref().map(|(id, _, _)| *id)
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
        let dir = main_dir(&job);
        let msg = message("build", job.id as f64, &job);
        self.send(msg.into());
        self.running = Some((job.id, dir, job.options));
        self.sources = Some(Box::new(job));
        self.progress = Progress::default();
        self.draft = None;
        self.started = js_sys::Date::now();
    }

    /// Sends `msg` to the worker, or keeps it until the worker is ready.
    fn send(&mut self, msg: JsValue) {
        let Some(worker) = &self.worker else { return };
        if !self.ready {
            self.unsent.push(msg);
        } else if let Err(e) = worker.post_message(&msg) {
            self.error = Some(format!("The typesetting engine did not take the build ({e:?})."));
        }
    }

    /// Exports `job` as `format` (retex's `-output-format` name: odt, docx, odp, pptx, html, md,
    /// svg); [`WebWorker::poll_export`] gives the files.
    pub fn export(&mut self, job: &Job, format: &str) -> Result<(), String> {
        if let Some(error) = self.error.as_ref().or(self.crashed.as_ref()) {
            return Err(error.clone());
        }
        if self.export.is_some() {
            return Err("An export is already running.".to_owned());
        }
        // Ids of their own, apart from the builds' (which count up from 1).
        self.next_export -= 1.0;
        let msg = message("export", self.next_export, job);
        let _ = Reflect::set(&msg, &"format".into(), &format.into());
        self.export = Some((self.next_export, None));
        self.send(msg.into());
        Ok(())
    }

    pub fn exporting(&self) -> bool {
        self.export.is_some()
    }

    /// The running export's files (the document first), once it ended, or why it failed.
    pub fn poll_export(&mut self) -> Option<Result<Exported, String>> {
        self.pump();
        if let Some(why) = &self.crashed
            && self.export.is_some()
        {
            self.export = None;
            return Some(Err(why.clone()));
        }
        match &mut self.export {
            Some((_, outcome @ Some(_))) => {
                let outcome = outcome.take();
                self.export = None;
                outcome
            }
            _ => None,
        }
    }

    /// Sorts the worker's messages: the handshake, its log, a crash and exports are handled here,
    /// builds' messages wait in `builds`.
    fn pump(&mut self) {
        loop {
            let Some(msg) = self.inbox.borrow_mut().pop_front() else { return };
            match get(&msg, "type").as_string().as_deref() {
                Some("ready") => {
                    self.ready = true;
                    for unsent in std::mem::take(&mut self.unsent) {
                        self.send(unsent);
                    }
                }
                Some("log") => {
                    let text = get(&msg, "text").as_string().unwrap_or_default();
                    match get(&msg, "level").as_string().as_deref() {
                        Some("ERROR") => log::error!("engine: {text}"),
                        Some("WARN") => log::warn!("engine: {text}"),
                        _ => log::info!("engine: {text}"),
                    }
                }
                Some("crash") => {
                    let why = get(&msg, "error").as_string().unwrap_or_default();
                    // The worker is gone: later builds fail at once with the same reason.
                    self.crashed = Some(format!("The typesetting engine stopped unexpectedly: {why}"));
                    self.worker = None;
                }
                Some("exported") => {
                    if let Some((id, outcome)) = &mut self.export
                        && get(&msg, "id").as_f64() == Some(*id)
                    {
                        *outcome = Some(match get(&msg, "error").as_string() {
                            Some(error) => Err(error),
                            None => Ok(exported(&msg)),
                        });
                    }
                }
                _ => self.builds.push_back(msg),
            }
        }
    }
}

/// The files of an `exported` message: the document, then its pictures.
fn exported(msg: &JsValue) -> Exported {
    let names: Array = get(msg, "names").unchecked_into();
    let contents: Array = get(msg, "contents").unchecked_into();
    let mut files: Vec<(String, Vec<u8>)> =
        names.iter().zip(contents.iter()).filter_map(|(n, c)| Some((n.as_string()?, c.dyn_into::<Uint8Array>().ok()?.to_vec()))).collect();
    let data = if files.is_empty() { Vec::new() } else { files.remove(0).1 };
    Exported { data, media: files }
}

/// retex's command-line flags for the kind of PDF `options` ask for (as `retex --help` has them):
/// `--accessible`, or the PDF and a PDF/X beside it (`<job>.pdf-x4.pdf`) with its bleed and crop
/// marks.
fn output_flags(options: &crate::Options) -> Vec<String> {
    match options.output {
        crate::Output::Screen => Vec::new(),
        crate::Output::Accessible => vec!["--accessible".to_owned()],
        crate::Output::Print => {
            let mut flags = vec![format!("-output-format=pdf,{}", options.pdfx.format())];
            flags.push(format!("--bleed={}mm", options.bleed_mm.clamp(0.0, crate::MAX_BLEED_MM)));
            if options.crop_marks {
                flags.push("--crop-marks".to_owned());
            }
            flags
        }
    }
}

/// A `build` or `export` message for `job`: the main document's name and folder, the project's
/// files at their paths, retex's extra arguments.
fn message(kind: &str, id: f64, job: &Job) -> Object {
    let dir = main_dir(job);
    let names = Array::new();
    let contents = Array::new();
    for (path, data) in &job.files {
        // The whole project: retex starts in the main document's folder (its `PWD`), so
        // `../shared/macros.tex` is found as on the desktop.
        names.push(&path.as_str().into());
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
    if job.options.draft_images {
        args.push(&"--draft-images".into());
    }
    // The kind of PDF, for a build: an export sets its own `-output-format`.
    if kind == "build" {
        for flag in output_flags(&job.options) {
            args.push(&flag.into());
        }
    }
    let msg = Object::new();
    for (k, v) in [
        ("type", JsValue::from(kind)),
        ("id", JsValue::from(id)),
        ("main", main.into()),
        ("dir", dir.unwrap_or_default().into()),
        ("names", names.into()),
        ("contents", contents.into()),
        ("args", args.into()),
        ("project", job.project.as_deref().map_or(JsValue::UNDEFINED, JsValue::from)),
    ] {
        let _ = Reflect::set(&msg, &k.into(), &v);
    }
    msg
}

impl WebWorker {
    /// Takes the messages that arrived; returns the running job's outcome when it ended.
    fn receive(&mut self) -> Option<Outcome> {
        let (id, dir, options) = self.running.clone()?;
        if let Some(error) = &self.error {
            return Some(Outcome::unavailable(id, error.clone()));
        }
        self.pump();
        if let Some(why) = &self.crashed {
            return Some(failure(id, why));
        }
        loop {
            let msg = self.builds.pop_front()?;
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
                    let written =
                        records::Written { pdf: bytes(get(&msg, "pdf")), synctex: bytes(get(&msg, "synctex")), pdfx: bytes(get(&msg, "pdfx")) };
                    let mut outcome = records::outcome(id, &output, dir.as_deref(), &options, written, seconds);
                    outcome.aux = get(&msg, "aux").as_string().map(Arc::from);
                    // TeX's own log, with the files it opened: the output has only the messages.
                    if let (Some(log), Some(job)) = (get(&msg, "log").as_string(), &self.sources) {
                        crate::places::place_where_started(&mut outcome.diagnostics, &log, &job.files, &job.main, dir.as_deref());
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
        self.sources = None;
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
        noticed: None,
    };
    Outcome { status: crate::Status::Failed, diagnostics: vec![diagnostic], ..Outcome::unavailable(job, "") }
}
