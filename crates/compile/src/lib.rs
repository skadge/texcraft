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
    pub help: Vec<String>,
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
}

impl Outcome {
    pub fn unavailable(job: u64, reason: impl Into<String>) -> Outcome {
        Outcome { job, status: Status::Unavailable(reason.into()), pdf: None, diagnostics: Vec::new(), log: String::new(), seconds: 0.0, pages: 0 }
    }

    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }
}

/// Something that can build a job.
pub trait Backend: Send {
    /// For the log and the settings, such as `retex 2.0.0`.
    fn name(&self) -> String;
    /// Builds `job`. Never panics; an engine failure is an outcome.
    fn compile(&mut self, job: &Job) -> Outcome;
    /// Whether a running build can be stopped. retex cannot yet (docs/plan.md, retex changes).
    fn can_cancel(&self) -> bool {
        false
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

/// The best backend this build has.
pub fn default_backend(cache_dir: Option<std::path::PathBuf>) -> Box<dyn Backend> {
    #[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
    {
        Box::new(retex_backend::Retex::new(cache_dir))
    }
    #[cfg(not(all(feature = "retex", not(target_arch = "wasm32"))))]
    {
        let _ = cache_dir;
        Box::new(Unavailable("This build of TeXCraft has no typesetting engine yet.".to_owned()))
    }
}
