//! The TeXCraft session: the open project, the files open in the editor, the builds and their
//! results. Every frontend (the egui window, the web page, the CLI, the agent tools) changes it
//! only through [`Session::execute`] and the commands registered in [`cmd`], so whatever a panel
//! can do, automation can do too (AGENTS.md rule 14).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cmd;
mod edits;
pub mod file_access;
pub mod find;
mod language;
mod sync;
pub use edits::FileEdit;
pub use language::{Completion, Fix, Lint, TextEdit};
pub use sync::PageRect;
pub use texcraft_buffer::{Change, Origin, Transaction};
pub mod guard;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use texcraft_compile::{Job, Outcome};
use texcraft_project::{Project, ProjectPath};

pub use texcraft_compile::{Diagnostic, Engine, Options as BuildOptions, Progress, Severity, Status};
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
    /// The engine, stopping at the first error, draft images (`build.options`).
    pub build: texcraft_compile::Options,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto_compile: true, auto_compile_delay_ms: 800, build: texcraft_compile::Options::default() }
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
    /// The language server, over the project's text files.
    language: language::Language,
    /// The SyncTeX data of the last build that wrote some.
    sync: Option<sync::Sync>,
    /// Where templates are kept; none without a data folder.
    templates: Option<texcraft_templates::Store>,
    /// The latest edits of the files, for the editor and the review anchors.
    edits: edits::Log,
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
            language: language::Language::new(None),
            sync: None,
            templates: None,
            edits: edits::Log::default(),
        }
    }

    /// Keeps templates in `dir` (the shell's data folder).
    pub fn set_template_store(&mut self, dir: PathBuf) {
        self.templates = Some(texcraft_templates::Store::new(dir));
    }

    pub fn template_store(&self) -> Option<&texcraft_templates::Store> {
        self.templates.as_ref()
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

    /// Whether the engine can stop a running build.
    pub fn can_cancel(&self) -> bool {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.can_cancel(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => false,
        }
    }

    /// Stops the running build; false when none runs or the engine cannot stop one.
    pub(crate) fn stop_build(&mut self) -> bool {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.cancel(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => false,
        }
    }

    /// The running build's progress and how long it has run, in seconds.
    pub fn progress(&mut self) -> Option<(Progress, f64)> {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.progress(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => None,
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

    /// Sets the build options; a change makes the PDF stale, so the automatic build runs again.
    pub fn set_build_options(&mut self, options: texcraft_compile::Options) {
        if options != self.settings.build {
            self.settings.build = options;
            self.built_revision = None;
        }
    }

    /// Whether the engine keeps build files that `build.clean` can delete.
    pub fn can_clean(&self) -> bool {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.can_clean(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(b, _) => b.cleaner().is_some(),
        }
    }

    /// Deletes the build files (not the PDF shown); the next build starts from scratch.
    pub(crate) fn clean_build(&mut self) -> Result<()> {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.clean()?,
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(b, _) => b.cleaner().ok_or("This typesetting engine keeps no build files.")?()?,
        }
        self.built_revision = None;
        Ok(())
    }

    /// Whether the sources changed since the last build started.
    pub fn is_stale(&self) -> bool {
        self.built_revision != Some(self.revision)
    }

    /// Replaces the text of `path` as the smallest edit that makes it (`file.write`).
    fn write_text(&mut self, path: &ProjectPath, text: String, origin: Origin) -> Result<()> {
        let old = self.project.text(path).map_err(|e| e.to_string())?;
        let tx = Transaction::diff(old, &text);
        self.apply_edit(path, &tx, origin)
    }

    /// Makes `tx` (character offsets in the text as it is) in the text file `path`, and records it
    /// for [`Session::edits_since`]. Every change of a file's text goes through here.
    pub(crate) fn apply_edit(&mut self, path: &ProjectPath, tx: &Transaction, origin: Origin) -> Result<()> {
        let old = self.project.text(path).map_err(|e| e.to_string())?;
        let new = edits::apply(old, tx).ok_or_else(|| format!("the edits do not fit in `{path}` ({} characters)", old.chars().count()))?;
        if tx.is_empty() {
            return Ok(());
        }
        self.project.set(path, texcraft_project::Contents::Text(new)).map_err(|e| e.to_string())?;
        self.touch();
        self.edits.push(FileEdit { revision: self.revision, path: path.clone(), changes: tx.clone(), origin });
        Ok(())
    }

    /// The edits made to `path` after `revision`, oldest first; `None` when some of them are no
    /// longer kept (then compare the texts). Changes that are not edits (a file replaced by
    /// `file.import`, a project opened) are not in it.
    pub fn edits_since(&self, path: &ProjectPath, revision: u64) -> Option<Vec<&FileEdit>> {
        self.edits.since(path, revision)
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn replace_project(&mut self, project: Project, dir: Option<PathBuf>) {
        self.current = project.main().cloned();
        self.open = self.current.iter().cloned().collect();
        self.project = project;
        self.language = language::Language::new(dir.as_deref());
        self.sync = None;
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
        let mut job = Job::from_project(&self.project, id).ok_or("The project has no main document to build.")?;
        job.options = self.settings.build;
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

    fn language(&mut self) -> &mut language::Language {
        self.language.sync(&self.project, self.revision);
        &mut self.language
    }

    /// Completions at a line and column (characters, from 0) of a text file.
    pub fn complete(&mut self, path: &ProjectPath, line: usize, column: usize) -> Vec<Completion> {
        self.language().complete(path, line, column)
    }

    /// What the language server says about the thing at a line and column, as Markdown.
    pub fn hover(&mut self, path: &ProjectPath, line: usize, column: usize) -> Option<String> {
        self.language().hover(path, line, column)
    }

    /// Where the thing at a line and column is defined.
    pub fn definition(&mut self, path: &ProjectPath, line: usize, column: usize) -> Option<(ProjectPath, usize, usize)> {
        self.language().definition(path, line, column)
    }

    /// The project file and line (from 1) that made a point of the PDF (`page` from 0, `x` and
    /// `y` in PDF points from the page's top left corner).
    pub fn source_at(&self, page: u32, x: f32, y: f32) -> Option<(ProjectPath, u32)> {
        self.sync.as_ref()?.source(&self.project, page, x, y)
    }

    /// Where line `line` (from 1) of `path` is in the PDF: rectangles of its first page that has it.
    pub fn locate(&self, path: &ProjectPath, line: u32) -> Vec<PageRect> {
        self.sync.as_ref().map(|s| s.locate(&self.project, path, line)).unwrap_or_default()
    }

    /// The fixes the language server suggests for the problems on a line (from 0).
    pub fn fixes(&mut self, path: &ProjectPath, line: usize) -> Vec<Fix> {
        self.language().fixes(path, line)
    }

    /// The problems the language server's lint finds in a text file, as typed.
    pub fn lint(&mut self, path: &ProjectPath) -> Vec<Lint> {
        self.language().lint(path).to_vec()
    }

    fn accept(&mut self, outcome: Outcome) -> &Outcome {
        let main = self.project.main().cloned();
        // The language server learns the document's commands, environments, labels and
        // citations from the build.
        let symbols = outcome.symbols.clone();
        self.language().build_finished(main.as_ref(), symbols);
        // A build that wrote a PDF brings its SyncTeX data; a failed one keeps the last PDF's.
        if outcome.pdf.is_some() {
            self.sync = outcome.synctex.as_deref().and_then(|d| sync::Sync::parse(d));
        }
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

/// `text` with `edits` made (lines and columns in characters, from 0; edits must not overlap).
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> Result<String> {
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let byte = |line: usize, column: usize| -> Result<usize> {
        let start = *starts.get(line).ok_or_else(|| format!("line {} is past the end", line + 1))?;
        let rest = &text[start..];
        let end = rest.find('\n').unwrap_or(rest.len());
        Ok(start + rest[..end].char_indices().nth(column).map_or(end, |(i, _)| i))
    };
    let mut spans =
        edits.iter().map(|e| Ok((byte(e.line, e.column)?, byte(e.end_line, e.end_column)?, e.text.as_str()))).collect::<Result<Vec<_>>>()?;
    spans.sort_by_key(|(s, _, _)| *s);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (s, e, t) in spans {
        if s < at || e < s {
            return Err("The fix's edits overlap.".to_owned());
        }
        out.push_str(&text[at..s]);
        out.push_str(t);
        at = e;
    }
    out.push_str(&text[at..]);
    Ok(out)
}
