//! retex linked in process, through its embedding API (`retex::Retex`, retex's
//! `crates/retex-cli/src/embed.rs`).
//!
//! The project is written to `<cache>/src/` and built there with the build files kept in
//! `<cache>/out/`, so the auxiliary files of one build (`.aux`, `.toc`, `.bbl`) are read by the
//! next and a rebuild usually needs one pass.
//!
//! From retex 2.1.0 a build reports its progress, can be cancelled, and returns the language
//! server's `symbols` record, the SyncTeX data, and its diagnostics' spans and fixes.
//!
//! In the desktop app builds go to a `retex worker` process (this program, run again), which
//! keeps the preamble snapshot between builds: a rebuild after a change in the body starts at
//! `\begin{document}`. Elsewhere (the CLI, tests, a worker that does not start) they run in
//! process and read the preamble every time.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::{Backend, Canceller, Cleaner, Diagnostic, DiagnosticFix, Engine, Event, Job, Options, Outcome, Severity, Status};

pub struct Retex {
    dir: PathBuf,
    /// The running in-process build's cancel token.
    running: Arc<Mutex<Option<retex::CancelToken>>>,
    /// The worker, when builds go to one.
    worker: Option<WorkerSlot>,
    /// The files of the last job written to `<cache>/src/`, to remove those that leave it.
    written: std::collections::HashSet<String>,
}

/// A `retex worker` process run from this program (see [`crate::use_this_program_as_worker`]).
struct WorkerSlot {
    program: PathBuf,
    /// Started on the first build, again after a Stop.
    process: Arc<Mutex<Option<retex::Worker>>>,
    /// Set by Stop: the build that ends with the worker gone was cancelled.
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl Retex {
    /// Builds in `cache_dir`, or in a folder of the system's temporary directory.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let dir = cache_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("texcraft-build-{}", std::process::id())));
        Self { dir, running: Arc::new(Mutex::new(None)), worker: None, written: Default::default() }
    }

    /// Builds through a worker process running `program` (this program, which called
    /// `retex::worker_entry` first in `main`): it keeps the preamble snapshot between builds.
    pub fn with_worker(mut self, program: PathBuf) -> Self {
        self.worker = Some(WorkerSlot { program, process: Arc::new(Mutex::new(None)), stopped: Arc::default() });
        self
    }

    /// Writes the job's files to `<cache>/src/`: only those that changed (so the worker's preamble
    /// snapshot sees an unchanged preamble as such), removing those no longer in the project. The
    /// build's own files stay.
    fn write_sources(&mut self, job: &Job) -> Result<PathBuf, String> {
        let src = self.dir.join("src");
        let mut now = std::collections::HashSet::new();
        for (path, bytes) in &job.files {
            let target = src.join(path.as_str());
            now.insert(path.as_str().to_owned());
            if std::fs::read(&target).is_ok_and(|old| old.as_slice() == &bytes[..]) {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
            }
            std::fs::write(&target, bytes).map_err(|e| format!("writing {}: {e}", target.display()))?;
        }
        for gone in self.written.difference(&now) {
            let _ = std::fs::remove_file(src.join(gone));
        }
        self.written = now;
        Ok(src.join(job.main.as_str()))
    }

    /// Builds in process, reporting its progress from retex's build thread.
    fn in_process(&self, main: &Path, options: Options, report: &mut dyn FnMut(Event)) -> Option<retex::Outcome> {
        let token = retex::CancelToken::new();
        if let Ok(mut slot) = self.running.lock() {
            *slot = Some(token.clone());
        }
        // retex calls back on its build thread; the events go to `report` here, on ours.
        let (tx, rx) = std::sync::mpsc::channel();
        let retex = engine_options(options).keep_files(self.dir.join("out")).cancel_token(token).on_progress(move |p| {
            let _ = tx.send(p);
        });
        let handle = std::thread::scope(|scope| {
            let build = scope.spawn(|| retex.compile_file_outcome(main));
            while !build.is_finished() {
                while let Ok(p) = rx.try_recv() {
                    if let Some(e) = event(p) {
                        report(e);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            build.join()
        });
        if let Ok(mut slot) = self.running.lock() {
            *slot = None;
        }
        handle.ok()
    }

    /// Builds through the worker; `None` when it cannot start or build (the caller builds in
    /// process instead).
    fn through_worker(slot: &WorkerSlot, main: &Path, options: Options, report: &mut dyn FnMut(Event)) -> Option<(retex::Outcome, bool)> {
        // From the main document's folder, as the in-process build runs.
        let dir = main.parent()?.to_path_buf();
        let name = main.file_name()?.to_string_lossy().into_owned();
        let request = {
            let mut process = slot.process.lock().ok()?;
            if process.is_none() {
                let mut config = retex::WorkerConfig::new();
                config.program = Some(slot.program.clone());
                match retex::Worker::spawn(config) {
                    Ok(w) => *process = Some(w),
                    Err(e) => {
                        log::warn!("the build worker did not start ({e}); building in process");
                        return None;
                    }
                }
            }
            slot.stopped.store(false, std::sync::atomic::Ordering::SeqCst);
            let mut request = retex::BuildRequest::new(dir, name);
            request.options = engine_options(options);
            match process.as_ref()?.build(request) {
                Ok(handle) => handle,
                Err(e) => {
                    log::warn!("the build worker refused the build ({e}); building in process");
                    *process = None;
                    return None;
                }
            }
        };
        let outcome = request.wait_with_progress(|p| {
            if let Some(e) = event(p) {
                report(e);
            }
        });
        Some((outcome, slot.stopped.load(std::sync::atomic::Ordering::SeqCst)))
    }
}

impl Backend for Retex {
    fn name(&self) -> String {
        "retex".to_owned()
    }

    fn compile(&mut self, job: &Job) -> Outcome {
        self.compile_reporting(job, &mut |_| {})
    }

    fn can_cancel(&self) -> bool {
        true
    }

    fn canceller(&self) -> Option<Canceller> {
        let running = self.running.clone();
        let worker = self.worker.as_ref().map(|w| (w.process.clone(), w.stopped.clone()));
        Some(Arc::new(move || {
            if let Ok(slot) = running.lock()
                && let Some(token) = slot.as_ref()
            {
                token.cancel();
            }
            // A build being waited on cannot be cancelled through its handle: the worker is
            // ended instead (it stops its build), and the next build starts a new one.
            if let Some((process, stopped)) = &worker
                && let Some(w) = process.lock().ok().and_then(|mut p| p.take())
            {
                stopped.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = std::thread::Builder::new().name("texcraft-stop-worker".to_owned()).spawn(move || drop(w));
            }
        }))
    }

    fn cleaner(&self) -> Option<Cleaner> {
        let dir = self.dir.clone();
        let worker = self.worker.as_ref().map(|w| w.process.clone());
        Some(Arc::new(move || {
            // The worker's snapshots are build files too: the next build starts a new worker.
            if let Some(w) = worker.as_ref().and_then(|p| p.lock().ok().and_then(|mut p| p.take())) {
                drop(w);
            }
            match std::fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("deleting {}: {e}", dir.display())),
                _ => Ok(()),
            }
        }))
    }

    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        let started = Instant::now();
        let main = match self.write_sources(job) {
            Ok(m) => m,
            Err(e) => return Outcome { log: e.clone(), ..Outcome::unavailable(job.id, e) },
        };
        let main_dir = job.main.as_str().rsplit_once('/').map(|(d, _)| d.to_owned());
        let src = std::fs::canonicalize(self.dir.join("src")).unwrap_or_else(|_| self.dir.join("src"));
        let place = |d: &retex::Diagnostic| convert(d, &src, main_dir.as_deref());
        let via_worker = self.worker.as_ref().and_then(|slot| Self::through_worker(slot, &main, job.options, report));
        let (outcome, stopped) = match via_worker {
            Some(found) => found,
            None => match self.in_process(&main, job.options, report) {
                Some(o) => (o, false),
                None => {
                    let seconds = started.elapsed().as_secs_f64();
                    return Outcome {
                        seconds,
                        ..Outcome::unavailable(job.id, "The typesetting engine stopped unexpectedly; the build was abandoned.")
                    };
                }
            },
        };
        let seconds = started.elapsed().as_secs_f64();
        let symbols = outcome.outputs.symbols().cloned();
        let synctex = outcome.outputs.synctex().map(|s| Arc::new(s.to_vec()));
        let base = Outcome { symbols, synctex, seconds, ..Outcome::unavailable(job.id, "") };
        if outcome.cancelled || stopped {
            return Outcome { status: Status::Cancelled, log: "The build was stopped.".to_owned(), ..base };
        }
        match outcome.result {
            Ok(doc) => Outcome {
                status: Status::Built,
                pdf: Some(Arc::new(doc.pdf().to_vec())),
                diagnostics: doc.diagnostics().iter().map(place).collect(),
                log: doc.log().to_owned(),
                pages: doc.pages(),
                ..base
            },
            Err(retex::Error::Failed { diagnostics, log }) => {
                Outcome { status: Status::Failed, diagnostics: diagnostics.iter().map(place).collect(), log, ..base }
            }
            Err(other) => {
                Outcome { log: other.to_string(), symbols: base.symbols, synctex: base.synctex, seconds, ..failure(job.id, &other.to_string()) }
            }
        }
    }
}

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
    Outcome { status: Status::Failed, diagnostics: vec![diagnostic], ..Outcome::unavailable(job, "") }
}

/// The project path of a file retex names: relative to the main document's folder, or absolute
/// inside the build's copy of the project (`src`). Files elsewhere (the TeX tree) have none.
fn project_file(file: &str, src: &Path, main_dir: Option<&str>) -> Option<String> {
    let path = Path::new(file);
    if path.is_absolute() {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let rel = canonical.strip_prefix(src).ok()?;
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    Some(match main_dir {
        Some(dir) => format!("{dir}/{file}"),
        None => file.to_owned(),
    })
}

fn convert(d: &retex::Diagnostic, src: &Path, main_dir: Option<&str>) -> Diagnostic {
    let file = d.file.as_deref().and_then(|f| project_file(f, src, main_dir));
    if d.file.is_some() && file.is_none() {
        log::debug!("diagnostic outside the project: {:?}", d.file);
    }
    Diagnostic {
        severity: match d.severity {
            retex::Severity::Hint => Severity::Hint,
            retex::Severity::Note => Severity::Note,
            retex::Severity::Warning => Severity::Warning,
            retex::Severity::Error => Severity::Error,
        },
        message: d.message.clone(),
        origin: d.origin.clone(),
        file,
        line: d.line,
        column: d.column,
        end_column: d.end_column,
        help: d.help.clone(),
        fixes: d
            .fixes
            .iter()
            .map(|f| DiagnosticFix { column: f.column, end_column: f.end_column, replacement: f.replacement.clone(), title: f.title.clone() })
            .collect(),
    }
}

fn event(p: retex::Progress) -> Option<Event> {
    Some(match p {
        retex::Progress::Pass { n, reason } => Event::Pass { n, reason },
        retex::Progress::Pages { shipped, expected } => Event::Pages { shipped, expected },
        retex::Progress::File { name } => Event::File { name },
        retex::Progress::Step { what } => Event::Step { what },
        _ => return None,
    })
}

/// retex's options for a build: always the language server's symbols and SyncTeX data.
fn engine_options(options: Options) -> retex::Retex {
    let identity = match options.engine {
        Engine::Auto => retex::Identity::Auto,
        Engine::LuaLatex => retex::Identity::LuaLatex,
        Engine::PdfLatex => retex::Identity::PdfLatex,
    };
    retex::Retex::new().symbols(true).synctex(true).identity(identity).halt_on_error(options.halt_on_error).draft_images(options.draft_images)
}
