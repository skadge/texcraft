//! The TeXCraft session: the open project, the files open in the editor, the builds and their
//! results. Every frontend (the egui window, the web page, the CLI, the agent tools) changes it
//! only through [`Session::execute`] and the commands registered in [`cmd`], so whatever a panel
//! can do, automation can do too (AGENTS.md rule 14).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cmd;
pub mod file_access;
pub mod guard;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use texcraft_compile::{Job, Outcome};
use texcraft_project::{Project, ProjectPath};

pub use texcraft_compile::{Diagnostic, Severity, Status};
pub use texcraft_project as project;

/// What a command returns: a JSON value for automation, or a message for the user.
pub type Result<T> = std::result::Result<T, String>;

/// Settings that change how the session behaves (the UI keeps its own appearance settings).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Build shortly after the user stops typing.
    pub auto_compile: bool,
    /// How long typing must pause before an automatic build, in milliseconds.
    pub auto_compile_delay_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto_compile: true, auto_compile_delay_ms: 800 }
    }
}

/// Where builds run.
enum Builder {
    #[cfg(not(target_arch = "wasm32"))]
    Thread(texcraft_compile::Worker),
    /// Runs on the caller's thread (wasm32, until the engine runs in a worker).
    #[cfg(target_arch = "wasm32")]
    Inline(Box<dyn texcraft_compile::Backend>, Option<Outcome>),
}

pub struct Session {
    project: Project,
    /// The folder the project was opened from or saved to.
    dir: Option<PathBuf>,
    /// Unsaved changes since the project was opened or saved.
    dirty: bool,
    /// Files open in the editor, in tab order, and the one shown.
    open: Vec<ProjectPath>,
    current: Option<ProjectPath>,
    /// Bumped by every change to a file, so the UI and the auto-build know what is stale.
    revision: u64,
    /// The revision the last finished build was started from.
    built_revision: Option<u64>,
    builder: Builder,
    next_job: u64,
    last: Option<Outcome>,
    /// The PDF of the last build that wrote one, kept when a later build fails.
    pdf: Option<Arc<Vec<u8>>>,
    pub settings: Settings,
    roots: Option<file_access::AutomationRoots>,
}

impl Session {
    /// A session with a blank project and the best backend this build has.
    pub fn new() -> Self {
        Self::with_backend(texcraft_compile::default_backend(None))
    }

    pub fn with_backend(backend: Box<dyn texcraft_compile::Backend>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let builder = Builder::Thread(texcraft_compile::Worker::new(backend));
        #[cfg(target_arch = "wasm32")]
        let builder = Builder::Inline(backend, None);
        let project = Project::blank();
        let current = project.main().cloned();
        Self {
            open: current.iter().cloned().collect(),
            current,
            project,
            dir: None,
            dirty: false,
            revision: 0,
            built_revision: None,
            builder,
            next_job: 1,
            last: None,
            pdf: None,
            settings: Settings::default(),
            roots: None,
        }
    }

    /// Confines every file access of this session to `roots` (agent sessions, RFC-0015).
    pub fn confine(&mut self, roots: file_access::AutomationRoots) {
        self.roots = Some(roots);
    }

    pub fn roots(&self) -> Option<&file_access::AutomationRoots> {
        self.roots.as_ref()
    }

    /// Runs command `id` with `params`: THE entry point for every frontend. Unknown commands,
    /// disabled commands and panics are errors, never crashes.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = cmd::find(id).ok_or_else(|| format!("unknown command `{id}`"))?;
        (spec.enabled)(self)?;
        guard::catch_panic(|| (spec.run)(self, params))?
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn dir(&self) -> Option<&PathBuf> {
        self.dir.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn open_files(&self) -> &[ProjectPath] {
        &self.open
    }

    pub fn current(&self) -> Option<&ProjectPath> {
        self.current.as_ref()
    }

    pub fn last_outcome(&self) -> Option<&Outcome> {
        self.last.as_ref()
    }

    pub fn pdf(&self) -> Option<&Arc<Vec<u8>>> {
        self.pdf.as_ref()
    }

    pub fn is_building(&self) -> bool {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.running().is_some(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => false,
        }
    }

    pub fn backend_name(&self) -> String {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.backend_name().to_owned(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(b, _) => b.name(),
        }
    }

    /// Whether the sources changed since the last build started.
    pub fn is_stale(&self) -> bool {
        self.built_revision != Some(self.revision)
    }

    /// Replaces the text of `path` (the editor calls this on every edit, through `file.write`).
    fn write_text(&mut self, path: &ProjectPath, text: String) -> Result<()> {
        self.project.set(path, texcraft_project::Contents::Text(text)).map_err(|e| e.to_string())?;
        self.touch();
        Ok(())
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    fn replace_project(&mut self, project: Project, dir: Option<PathBuf>) {
        self.current = project.main().cloned();
        self.open = self.current.iter().cloned().collect();
        self.project = project;
        self.dir = dir;
        self.dirty = false;
        self.touch();
        self.dirty = false;
        self.last = None;
        self.pdf = None;
    }

    /// Starts a build of the current sources; returns its job id.
    fn start_build(&mut self) -> Result<u64> {
        if self.roots.is_some() {
            // RFC-0015: every path a document names must be checked against the roots, and retex
            // does not yet let its host decide which files a build may read (docs/plan.md, R4).
            return Err("Building is not available to agents yet: the engine cannot confine the files a document reads.".to_owned());
        }
        let id = self.next_job;
        self.next_job += 1;
        let job = Job::from_project(&self.project, id).ok_or("The project has no main document to build.")?;
        self.built_revision = Some(self.revision);
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.submit(job),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(backend, slot) => *slot = Some(backend.compile(&job)),
        }
        Ok(id)
    }

    /// Takes a finished build's outcome, if one arrived. The frontends call this each frame.
    pub fn poll(&mut self) -> Option<&Outcome> {
        let outcome = match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.poll(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(_, slot) => slot.take(),
        }?;
        Some(self.accept(outcome))
    }

    /// Blocks until the running build finishes (CLI and agent tools).
    pub fn wait(&mut self) -> Option<&Outcome> {
        let outcome = match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.wait(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(_, slot) => slot.take(),
        }?;
        Some(self.accept(outcome))
    }

    fn accept(&mut self, outcome: Outcome) -> &Outcome {
        if let Some(pdf) = &outcome.pdf {
            self.pdf = Some(pdf.clone());
        }
        self.last.insert(outcome)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
