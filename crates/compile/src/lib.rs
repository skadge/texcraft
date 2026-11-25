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
//!
//! [`formulas`] (feature `retex`, not on wasm32) typesets a file's formulas on their own, for the
//! formatted view; [`scratch`] typesets a scratch document after the project's preamble, for a
//! preview of LaTeX a model wrote.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;

use texcraft_project::{Project, ProjectPath};

#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub mod formulas;
pub mod records;
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub mod retex_backend;
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub mod scratch;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub mod web_engine;
#[cfg(not(target_arch = "wasm32"))]
mod worker;
#[cfg(not(target_arch = "wasm32"))]
pub use worker::Worker;

/// What to build: every file of the project, and which one is the main document.
#[derive(Debug, Clone)]
pub struct Job {
    pub files: Vec<(ProjectPath, Arc<[u8]>)>,
    pub main: ProjectPath,
    pub options: Options,
    /// Increases with every job a session starts, so an outcome can be matched to its job.
    pub id: u64,
    /// Which project this is, for an engine that keeps something per project: the browser's
    /// worker keeps the TeX files a project's builds read, to fetch them at once next time.
    pub project: Option<String>,
}

impl Job {
    /// A snapshot of `project`; `None` when it has no main document.
    pub fn from_project(project: &Project, id: u64) -> Option<Job> {
        let main = project.main()?.clone();
        let files = project.files().map(|(p, c)| (p.clone(), Arc::<[u8]>::from(c.as_bytes()))).collect();
        Some(Job { files, main, id, options: Options::default(), project: None })
    }
}

/// The engine a build is like.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// Chosen from the document: pdfLaTeX's mode for a document written for it, LuaLaTeX's
    /// otherwise.
    #[default]
    Auto,
    LuaLatex,
    PdfLatex,
}

/// The kind of PDF a build makes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    /// A PDF for reading on a screen.
    #[default]
    Screen,
    /// A tagged PDF (PDF/UA), checked for what screen readers need.
    Accessible,
    /// The PDF, and beside it a PDF/X for the press, with its preflight checks.
    Print,
}

/// The PDF/X standard of a print build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PdfX {
    /// PDF/X-4: colour profiles and live transparency.
    #[default]
    X4,
    /// PDF/X-1a: CMYK only, transparency flattened.
    X1a,
}

impl PdfX {
    /// The standard's name, as printers write it.
    pub fn name(self) -> &'static str {
        match self {
            PdfX::X4 => "PDF/X-4",
            PdfX::X1a => "PDF/X-1a",
        }
    }

    /// retex's name for it in `-output-format` (`pdf-x4`).
    pub fn format(self) -> &'static str {
        match self {
            PdfX::X4 => "pdf-x4",
            PdfX::X1a => "pdf-x1a",
        }
    }
}

/// The largest bleed a print build takes, in millimetres.
pub const MAX_BLEED_MM: f32 = 20.0;

/// How a build runs (Settings › Building, the Compile button's menu).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Options {
    pub engine: Engine,
    /// Stop at the first error instead of going on and reporting every error.
    pub halt_on_error: bool,
    /// Draw images as framed boxes with their names, for faster builds.
    pub draft_images: bool,
    /// The kind of PDF: for the screen, accessible, or for print.
    pub output: Output,
    /// For print: the PDF/X standard.
    pub pdfx: PdfX,
    /// For print: how far the art goes past the page's edges, in millimetres.
    pub bleed_mm: f32,
    /// For print: crop marks outside the bleed.
    pub crop_marks: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            engine: Engine::Auto,
            halt_on_error: false,
            draft_images: false,
            output: Output::Screen,
            pdfx: PdfX::X4,
            bleed_mm: 3.0,
            crop_marks: true,
        }
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

impl Diagnostic {
    /// A finding of the accessibility checks (an accessible build's).
    pub fn is_accessibility(&self) -> bool {
        self.origin.as_deref() == Some(ACCESSIBILITY)
    }

    /// A finding of the print checks or the PDF/X conversion (a print build's).
    pub fn is_print(&self) -> bool {
        matches!(self.origin.as_deref(), Some(PRINT | PDFX))
    }
}

/// The origin of accessibility findings.
pub const ACCESSIBILITY: &str = "accessibility";
/// The origin of print checks.
pub const PRINT: &str = "print";
/// The origin of the PDF/X conversion's messages.
pub const PDFX: &str = "PDF/X";

/// A diagnostic's origin as this crate keeps it. retex 2.5.0 gives its own accessibility checks
/// the origin `accessibility`, but rewrites LaTeX's tagging messages (an image without
/// alternative text, formulas without MathML, the PDF's language, a class without tagging) with
/// no origin; those are accessibility findings too.
pub(crate) fn origin(origin: Option<String>, message: &str) -> Option<String> {
    let tagging = message.ends_with("has no alternative text")
        || message == "formulas are tagged without MathML"
        || message.starts_with("the PDF's language is taken to be")
        || message.ends_with("does not support LaTeX's tagging yet");
    if origin.is_none() && tagging { Some(ACCESSIBILITY.to_owned()) } else { origin }
}

/// A finding of a print build's preflight, placed on the page and in the source (retex's
/// `PrintCheck`, after GWG 2022).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PrintCheck {
    /// `low-resolution`, `ink-coverage`, `hairline`, `near-trim`, `no-bleed`, ...
    pub kind: String,
    /// The check in words, with its limit (`images of 150 ppi or more`).
    pub title: String,
    pub severity: Severity,
    /// The page, from 0.
    pub page: u32,
    /// The worst object's box, in PDF points from the page's top left corner, on the page as
    /// designed (the PDF the preview shows).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// How many objects.
    pub count: u32,
    /// The worst value, and the limit it was compared with, in the check's unit.
    pub value: f64,
    pub limit: Option<f64>,
    pub detail: Option<String>,
    /// The project file and line, when retex found them.
    pub file: Option<String>,
    pub line: Option<u32>,
    /// The image file, for images.
    pub image: Option<String>,
}

/// A rule of the PDF/X standard, and what breaks it (none: met).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PdfxRule {
    pub rule: String,
    pub problems: Vec<String>,
}

/// What a print build made besides the PDF: the PDF/X and its checks.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Preflight {
    pub standard: PdfX,
    #[serde(skip)]
    pub pdfx: Option<Arc<Vec<u8>>>,
    pub checks: Vec<PrintCheck>,
    pub rules: Vec<PdfxRule>,
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
    /// The kind of PDF the build was asked for.
    pub output: Output,
    /// A print build's PDF/X and checks.
    pub preflight: Option<Preflight>,
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
            output: Output::Screen,
            preflight: None,
        }
    }

    /// The findings of the accessibility checks.
    pub fn accessibility(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.is_accessibility())
    }

    /// The problems a print build found: its print and PDF/X messages that are warnings or
    /// errors. A print build that wrote no PDF/X has one more. `None` for other builds.
    pub fn print_problems(&self) -> Option<usize> {
        let preflight = self.preflight.as_ref()?;
        let found = self.diagnostics.iter().filter(|d| d.is_print() && d.severity >= Severity::Warning).count();
        Some(found + usize::from(preflight.pdfx.is_none()))
    }

    /// The print check behind a print message, which places it on the page: the one at its source
    /// line, or for a message without one, on the page it names first.
    pub fn place_on_page(&self, d: &Diagnostic) -> Option<&PrintCheck> {
        if d.origin.as_deref() != Some(PRINT) {
            return None;
        }
        let checks = &self.preflight.as_ref()?.checks;
        if d.line.is_some() {
            return checks.iter().find(|c| c.line == d.line && c.file == d.file);
        }
        // `page 3: …`, `pages 2–4: …`, `pages 2, 5 and 7: …`: the pages the check's worst object
        // may be on.
        let (pages, _) = d.message.strip_prefix("page")?.split_once(':')?;
        let pages: Vec<u32> = pages.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect();
        let on = |page: u32| match (pages.as_slice(), d.message.contains('–')) {
            ([a, b], true) => (*a..=*b).contains(&page),
            (list, _) => list.contains(&page),
        };
        checks.iter().find(|c| c.line.is_none() && on(c.page + 1))
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
    /// The PDF of a pass that is not the last (retex 2.3.0), to show while the next passes run;
    /// its references may be undefined.
    Draft {
        pass: u32,
        #[serde(skip)]
        pdf: Arc<Vec<u8>>,
    },
}

/// The running build's draft: the PDF of its last pass so far that was not the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub job: u64,
    /// The pass that wrote it, from 1.
    pub pass: u32,
    pub pdf: Arc<Vec<u8>>,
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
    /// The pass whose PDF is the build's draft, when one was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<u32>,
}

impl Progress {
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Pass { n, reason } => {
                // The pass before tells how many pages to expect; its draft stays the draft.
                let expected = Some(self.pages).filter(|p| *p > 0).or(self.expected);
                *self = Progress { pass: n, reason, expected, draft: self.draft, ..Progress::default() };
            }
            Event::Pages { shipped, expected } => {
                self.pages = shipped;
                self.expected = expected.or(self.expected);
                self.reading = None;
                self.step = None;
            }
            Event::File { name } => self.reading = Some(name),
            Event::Step { what } => self.step = Some(what),
            Event::Draft { pass, .. } => self.draft = Some(pass),
        }
    }
}

/// Stops a backend's running build.
pub type Canceller = Arc<dyn Fn() + Send + Sync>;

/// Deletes a backend's build files (call it with no build running); the next build starts from
/// scratch.
pub type Cleaner = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// A format a project can be exported to, besides the build's own PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Export {
    Pdf,
    Odt,
    Docx,
    Odp,
    Pptx,
    Html,
    Markdown,
    Svg,
}

impl Export {
    pub const ALL: [Export; 8] = [Export::Pdf, Export::Odt, Export::Docx, Export::Odp, Export::Pptx, Export::Html, Export::Markdown, Export::Svg];

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Export::Pdf => "pdf",
            Export::Odt => "odt",
            Export::Docx => "docx",
            Export::Odp => "odp",
            Export::Pptx => "pptx",
            Export::Html => "html",
            Export::Markdown => "md",
            Export::Svg => "svg",
        }
    }

    /// Whether it is a presentation format (a slide per page or frame).
    pub fn is_presentation(self) -> bool {
        matches!(self, Export::Odp | Export::Pptx)
    }

    /// The format of a name or extension (`docx`, `md`, `markdown`).
    pub fn parse(name: &str) -> Option<Export> {
        let name = name.to_ascii_lowercase();
        Export::ALL
            .into_iter()
            .find(|f| f.extension() == name || serde_json::to_value(f).ok().and_then(|v| v.as_str().map(|s| s == name)).unwrap_or(false))
    }
}

/// An exported document: its file, and the files it refers to (Markdown's pictures, by their
/// path relative to the document's folder).
#[derive(Debug, Clone, Default)]
pub struct Exported {
    pub data: Vec<u8>,
    pub media: Vec<(String, Vec<u8>)>,
}

/// Builds a job again and converts it to a format (from any thread; it blocks until done). The
/// `&str` is the folder Markdown's pictures go to, relative to the document.
pub type Exporter = Arc<dyn Fn(&Job, Export, &str) -> Result<Exported, String> + Send + Sync>;

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
    /// Deletes the build files, for a backend that keeps some.
    fn cleaner(&self) -> Option<Cleaner> {
        None
    }
    /// Exports a job to other formats, for a backend that can.
    fn exporter(&self) -> Option<Exporter> {
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
