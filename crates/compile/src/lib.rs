//! Building a project into a PDF.
//!
//! A [`Backend`] turns a [`Job`] (a snapshot of the project's files and its main document) into an
//! [`Outcome`]: the PDF when one was written, the diagnostics, the log and the time taken. The
//! engine session never calls a backend directly: it hands jobs to a [`Worker`], which runs them
//! off the UI thread and is polled each frame, so a long build never freezes the window.
//!
//! Backends:
//! - [`retex_backend::Retex`] (feature `retex`, not on wasm32): retex linked in process.
//! - [`Unavailable`]: says why there is no engine (the web build, for now). It never pretends to
//!   have built anything (AGENTS.md rule 15).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;

use texcraft_project::{Project, ProjectPath};

#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub mod retex_backend;
#[cfg(not(target_arch = "wasm32"))]
mod worker;
#[cfg(not(target_arch = "wasm32"))]
pub use worker::Worker;

/// What to build: every file of the project, and which one is the main document.
#[derive(Debug, Clone)]
pub struct Job {
    pub files: Vec<(ProjectPath, Arc<[u8]>)>,
    pub main: ProjectPath,
    /// Increases with every job a session starts, so an outcome can be matched to its job.
    pub id: u64,
}

impl Job {
    /// A snapshot of `project`; `None` when it has no main document.
    pub fn from_project(project: &Project, id: u64) -> Option<Job> {
        let main = project.main()?.clone();
        let files = project.files().map(|(p, c)| (p.clone(), Arc::<[u8]>::from(c.as_bytes()))).collect();
        Some(Job { files, main, id })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Hint,
    Note,
    Warning,
    Error,
}

/// One message of a build, placed in the source when the engine knows where.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// `LaTeX`, `Package hyperref`, `BibTeX`; `None` for TeX itself.
    pub origin: Option<String>,
    /// The project file, when the message names one inside the project.
    pub file: Option<String>,
    /// From 1.
    pub line: Option<u32>,
    /// From 1.
    pub column: Option<u32>,
    /// The column after the marked span, from 1.
    pub end_column: Option<u32>,
    pub help: Vec<String>,
    /// Edits that fix it, on its line.
    pub fixes: Vec<DiagnosticFix>,
}

/// An edit that fixes a diagnostic, on its line: the characters from `column` to before
/// `end_column` (from 1) become `replacement`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DiagnosticFix {
    pub column: u32,
    pub end_column: u32,
    pub replacement: String,
    /// What the fix does, in a few words ("Use \\textbf"), when retex says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", content = "reason", rename_all = "lowercase")]
pub enum Status {
    /// A PDF was written; it may still have errors in its diagnostics (TeX goes on after most).
    Built,
    /// No PDF: the diagnostics say why.
    Failed,
    /// No engine could run the job; the reason is for the user.
    Unavailable(String),
    /// The build was stopped before its end.
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub job: u64,
    pub status: Status,
    pub pdf: Option<Arc<Vec<u8>>>,
    pub diagnostics: Vec<Diagnostic>,
    pub log: String,
    pub seconds: f64,
    pub pages: u32,
    /// The language server's `symbols` record (labels, citations, commands), when the engine
    /// makes one; a failed build has one too.
    pub symbols: Option<serde_json::Value>,
    /// The SyncTeX data, for going between the source and the PDF.
    pub synctex: Option<Arc<Vec<u8>>>,
}

impl Outcome {
    pub fn unavailable(job: u64, reason: impl Into<String>) -> Outcome {
        Outcome {
            job,
            status: Status::Unavailable(reason.into()),
            pdf: None,
            diagnostics: Vec::new(),
            log: String::new(),
            seconds: 0.0,
            pages: 0,
            symbols: None,
            synctex: None,
        }
    }

    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }
}

/// What a build reports while it runs (retex's progress, from 2.1.0).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Event {
    /// A pass starts: its number (from 1) and why it runs (`cross-references`), none for the first.
    Pass { n: u32, reason: Option<String> },
    /// Pages shipped so far in this pass, and how many the last pass had.
    Pages { shipped: u32, expected: Option<u32> },
    /// A file is read, before the pass's first page.
    File { name: String },
    /// A step between passes, such as `running biber`.
    Step { what: String },
}

/// A running build's progress so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Progress {
    /// The pass (from 1; 0 before the engine says) and why it runs.
    pub pass: u32,
    pub reason: Option<String>,
    /// Pages shipped in this pass, and how many the last pass had.
    pub pages: u32,
    pub expected: Option<u32>,
    /// The file being read before the first page, or the step between passes.
    pub reading: Option<String>,
    pub step: Option<String>,
}

impl Progress {
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Pass { n, reason } => {
                // The pass before tells how many pages to expect.
                let expected = Some(self.pages).filter(|p| *p > 0).or(self.expected);
                *self = Progress { pass: n, reason, expected, ..Progress::default() };
            }
            Event::Pages { shipped, expected } => {
                self.pages = shipped;
                self.expected = expected.or(self.expected);
                self.reading = None;
                self.step = None;
            }
            Event::File { name } => self.reading = Some(name),
            Event::Step { what } => self.step = Some(what),
        }
    }
}

/// Stops a backend's running build.
pub type Canceller = Arc<dyn Fn() + Send + Sync>;

/// Something that can build a job.
pub trait Backend: Send {
    /// For the log and the settings, such as `retex 2.0.0`.
    fn name(&self) -> String;
    /// Builds `job`. Never panics; an engine failure is an outcome.
    fn compile(&mut self, job: &Job) -> Outcome;
    /// Builds `job`, reporting its progress as it runs (from the build's thread; `report` must
    /// not block). A backend that cannot report builds as [`Backend::compile`] does.
    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        let _ = report;
        self.compile(job)
    }
    /// Whether a running build can be stopped.
    fn can_cancel(&self) -> bool {
        false
    }
    /// Stops the running build when called (from any thread), for a backend that can.
    fn canceller(&self) -> Option<Canceller> {
        None
    }
}

/// A backend that builds nothing and says why.
pub struct Unavailable(pub String);

impl Backend for Unavailable {
    fn name(&self) -> String {
        "none".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, self.0.clone())
    }
}

/// The program that runs the build worker, once [`use_this_program_as_worker`] said so.
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
static WORKER_PROGRAM: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// For `main`, first: when this process was started as a build worker, runs it and returns its
/// exit code; `None` otherwise.
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub fn worker_entry() -> Option<std::process::ExitCode> {
    retex::worker_entry()
}

/// Builds go to a worker process running this program (which calls [`worker_entry`] first in
/// `main`), keeping the preamble snapshot between builds.
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub fn use_this_program_as_worker() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = WORKER_PROGRAM.set(exe);
    }
}

/// The best backend this build has.
pub fn default_backend(cache_dir: Option<std::path::PathBuf>) -> Box<dyn Backend> {
    #[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
    {
        let backend = retex_backend::Retex::new(cache_dir);
        match WORKER_PROGRAM.get() {
            Some(program) => Box::new(backend.with_worker(program.clone())),
            None => Box::new(backend),
        }
    }
    #[cfg(not(all(feature = "retex", not(target_arch = "wasm32"))))]
    {
        let _ = cache_dir;
        Box::new(Unavailable("This build of TeXCraft has no typesetting engine yet.".to_owned()))
    }
}
