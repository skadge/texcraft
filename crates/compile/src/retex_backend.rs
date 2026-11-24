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
//! From retex 2.3.0 both kinds of build below are a `retex::BuildHandle`, and Stop cancels the
//! one running through its `BuildCanceller`: a worker build stops and the worker keeps its
//! preamble snapshot.
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
    /// The running build, in process or through the worker: Stop cancels it.
    running: Arc<Mutex<Running>>,
    /// The worker, when builds go to one.
    worker: Option<WorkerSlot>,
    /// The files of the last job written to `<cache>/src/`, to remove those that leave it.
    written: std::collections::HashSet<String>,
}

/// What Stop reaches: the running build's canceller, or, before the build has one (while the
/// sources are written or the worker starts), that it is to be cancelled as soon as it starts.
#[derive(Default)]
struct Running {
    canceller: Option<retex::BuildCanceller>,
    stop: bool,
}

/// A `retex worker` process run from this program (see [`crate::use_this_program_as_worker`]).
struct WorkerSlot {
    program: PathBuf,
    /// Started on the first build; it and its preamble snapshot stay across builds, a stopped one
    /// too.
    process: Arc<Mutex<Option<retex::Worker>>>,
}

impl Retex {
    /// Builds in `cache_dir`, or in a folder of the system's temporary directory.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let dir = cache_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("texcraft-build-{}", std::process::id())));
        Self { dir, running: Arc::default(), worker: None, written: Default::default() }
    }

    /// Builds through a worker process running `program` (this program, which called
    /// `retex::worker_entry` first in `main`): it keeps the preamble snapshot between builds.
    pub fn with_worker(mut self, program: PathBuf) -> Self {
        self.worker = Some(WorkerSlot { program, process: Arc::new(Mutex::new(None)) });
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

    /// Waits for `build`, reporting its progress, while Stop can cancel it through its canceller.
    fn wait(&self, build: retex::BuildHandle, report: &mut dyn FnMut(Event)) -> retex::Outcome {
        let canceller = build.canceller();
        if let Ok(mut running) = self.running.lock() {
            if running.stop {
                canceller.cancel();
            }
            running.canceller = Some(canceller);
        }
        let outcome = build.wait_with_progress(|p| {
            if let Some(e) = event(p) {
                report(e);
            }
        });
        if let Ok(mut running) = self.running.lock() {
            running.canceller = None;
        }
        outcome
    }

    /// Builds in process, on retex's own thread (`Retex::spawn_file`).
    fn in_process(&self, main: &Path, options: Options, report: &mut dyn FnMut(Event)) -> retex::Outcome {
        let build = engine_options(options).keep_files(self.dir.join("out")).spawn_file(main);
        self.wait(build, report)
    }

    /// Builds through the worker; `None` when it cannot start or build (the caller builds in
    /// process instead).
    fn through_worker(&self, slot: &WorkerSlot, main: &Path, options: Options, report: &mut dyn FnMut(Event)) -> Option<retex::Outcome> {
        // From the main document's folder, as the in-process build runs.
        let dir = main.parent()?.to_path_buf();
        let name = main.file_name()?.to_string_lossy().into_owned();
        let build = {
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
        Some(self.wait(build, report))
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
        Some(Arc::new(move || {
            if let Ok(mut running) = running.lock() {
                // Through the worker, it sends the worker's `cancel`: the worker and its preamble
                // snapshot stay for the next build.
                if let Some(c) = &running.canceller {
                    c.cancel();
                }
                running.stop = true;
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

    fn exporter(&self) -> Option<crate::Exporter> {
        let dir = self.dir.join("export");
        Some(Arc::new(move |job: &Job, format: crate::Export, media: &str| export(&dir, job, format, media)))
    }

    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        let started = Instant::now();
        // A Stop for an earlier build does not reach this one.
        if let Ok(mut running) = self.running.lock() {
            running.stop = false;
        }
        let main = match self.write_sources(job) {
            Ok(m) => m,
            Err(e) => return Outcome { log: e.clone(), ..Outcome::unavailable(job.id, e) },
        };
        let main_dir = job.main.as_str().rsplit_once('/').map(|(d, _)| d.to_owned());
        let src = std::fs::canonicalize(self.dir.join("src")).unwrap_or_else(|_| self.dir.join("src"));
        let place = |d: &retex::Diagnostic| convert(d, &src, main_dir.as_deref());
        let via_worker = self.worker.as_ref().and_then(|slot| self.through_worker(slot, &main, job.options, report));
        let outcome = match via_worker {
            Some(o) => o,
            None => self.in_process(&main, job.options, report),
        };
        let seconds = started.elapsed().as_secs_f64();
        let symbols = outcome.outputs.symbols().cloned();
        let synctex = outcome.outputs.synctex().map(|s| Arc::new(s.to_vec()));
        let base = Outcome { symbols, synctex, seconds, ..Outcome::unavailable(job.id, "") };
        if outcome.cancelled {
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

/// Builds `job` in process in `dir` (its own copy of the project, apart from the shown build's)
/// and converts it to `format`.
fn export(dir: &Path, job: &Job, format: crate::Export, media: &str) -> Result<crate::Exported, String> {
    use crate::Export;
    let src = dir.join("src");
    // A fresh copy: files gone from the project are gone from it.
    match std::fs::remove_dir_all(&src) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("deleting {}: {e}", src.display())),
        _ => {}
    }
    for (path, bytes) in &job.files {
        let target = src.join(path.as_str());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("writing {}: {e}", target.display()))?;
    }
    let main = src.join(job.main.as_str());
    let options = job.options;
    let identity = match options.engine {
        Engine::Auto => retex::Identity::Auto,
        Engine::LuaLatex => retex::Identity::LuaLatex,
        Engine::PdfLatex => retex::Identity::PdfLatex,
    };
    let doc = retex::Retex::new()
        .identity(identity)
        .draft_images(options.draft_images)
        .keep_files(dir.join("out"))
        .compile_file(&main)
        .map_err(|e| reason(&e))?;
    let bytes = |r: Result<Vec<u8>, retex::Error>| r.map(|data| crate::Exported { data, media: Vec::new() }).map_err(|e| reason(&e));
    match format {
        Export::Pdf => Ok(crate::Exported { data: doc.pdf().to_vec(), media: Vec::new() }),
        Export::Odt => bytes(doc.to_odt()),
        Export::Docx => bytes(doc.to_docx()),
        Export::Odp => bytes(doc.to_odp()),
        Export::Pptx => bytes(doc.to_pptx()),
        Export::Html => bytes(doc.to_html().map(String::into_bytes)),
        Export::Svg => bytes(doc.to_svg().map(String::into_bytes)),
        Export::Markdown => {
            let md = doc.to_markdown(media).map_err(|e| reason(&e))?;
            Ok(crate::Exported {
                data: md.text.into_bytes(),
                media: md.files.into_iter().map(|(name, data)| (format!("{media}/{name}"), data)).collect(),
            })
        }
    }
}

/// Why retex failed, for the user: its first error when the build failed.
fn reason(e: &retex::Error) -> String {
    match e {
        retex::Error::Failed { diagnostics, .. } => match diagnostics.iter().find(|d| d.severity == retex::Severity::Error) {
            Some(d) => match (&d.file, d.line) {
                (Some(f), Some(l)) => format!("the build failed: {f}:{l}: {}", d.message),
                _ => format!("the build failed: {}", d.message),
            },
            None => "the build failed".to_owned(),
        },
        other => other.to_string(),
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
        retex::Progress::Draft { pass, pdf } => Event::Draft { pass, pdf: Arc::new(pdf) },
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
