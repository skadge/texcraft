//! The TeXCraft session: the open project, the files open in the editor, the builds and their
//! results. Every frontend (the egui window, the web page, the CLI, the agent tools) changes it
//! only through [`Session::execute`] and the commands registered in [`cmd`], so whatever a panel
//! can do, automation can do too (AGENTS.md rule 14).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod ai;
pub mod cmd;
pub mod describe;
mod edits;
pub mod file_access;
pub mod find;
mod formulas;
pub mod grammar;
pub mod hidden;
mod language;
pub mod numbering;
pub mod spelling;
pub use grammar::{GrammarOptions, GrammarProblem};
pub use spelling::{Misspelling, SpellingOptions};
mod sync;
pub use edits::FileEdit;
pub use formulas::{Formula, FormulaBox, FormulaState, Formulas};
pub use language::{Completion, Fix, Lint, TextEdit};
pub use sync::PageRect;
pub use texcraft_buffer::{Change, Origin, Transaction};
pub mod guard;
pub mod import;
pub mod library;
mod review_store;
pub use texcraft_review as review;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use texcraft_compile::Job;
use texcraft_project::{Project, ProjectPath};

/// Why the last forge pull or commit was refused ([`Session::forge_problem`]).
pub use cmd::clone::{ClonePhase, ImportProblem, folder_free, free_folder, is_ssh, repository_name, typed_address};
pub use cmd::forge::Problem as ForgeProblem;
/// Where a sign-in to a forge stands, and why one is not offered.
pub use cmd::forge_auth::{Progress as SignInProgress, Unavailable as SignInUnavailable};
/// Which forge lookup an outcome is of ([`Session::poll_forge_lookup`]).
pub use cmd::forge_pick::Lookup as ForgeLookup;
/// A git hosting service recognised by a remote's address (`git.remotes`' `hosting`).
pub use cmd::git::{Hosting as GitHosting, hosting as git_hosting};
pub use texcraft_compile::engine_choice;
pub use texcraft_compile::{
    Diagnostic, Draft, Engine, Export, MAX_BLEED_MM, Options as BuildOptions, Outcome, Output, PdfX, Progress, Severity, Status,
};
pub use texcraft_forge as forge;
#[cfg(not(target_arch = "wasm32"))]
pub use texcraft_gitwire as gitwire;
pub use texcraft_history as history;
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
    /// Build the sources with their open suggestions applied, without writing them
    /// (`build.suggestions`); the normal build ignores suggestions.
    pub with_suggestions: bool,
    /// Spell checking: on, the language, comments (`language.spelling_options`).
    #[serde(default)]
    pub spelling: SpellingOptions,
    /// Grammar checking: on, the rules and groups switched off (`grammar.options`, `grammar.rule`).
    #[serde(default)]
    pub grammar: GrammarOptions,
    /// The kinds of build message hidden in every project (`build.hide_messages`, [`hidden`]).
    #[serde(default)]
    pub hidden_messages: Vec<String>,
    /// The window writes changes shortly after typing stops and makes versions by itself
    /// (`project.autosave`); the session only keeps the setting, the window does the timing.
    #[serde(default = "yes")]
    pub autosave: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_compile: true,
            auto_compile_delay_ms: 800,
            build: texcraft_compile::Options::default(),
            with_suggestions: false,
            spelling: SpellingOptions::default(),
            grammar: GrammarOptions::default(),
            hidden_messages: Vec::new(),
            autosave: true,
        }
    }
}

/// A print build's PDF/X and its standard, or why it wrote none.
type PrintPdf = std::result::Result<(texcraft_compile::PdfX, Arc<Vec<u8>>), String>;

/// The kinds of file a build can make, in the order the build log lists them.
pub const OUTPUT_KINDS: [&str; 4] = ["pdf", "pdfx", "synctex", "log"];

/// A file the last build made, as the build log lists it (`build.outputs`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BuildOutput {
    /// One of [`OUTPUT_KINDS`]: the PDF, a print build's PDF/X, the SyncTeX data, the raw log.
    pub kind: &'static str,
    /// Its file name, after the main document's.
    pub name: String,
    pub bytes: usize,
    /// Where it was last saved in this session (`build.save_output`), if it was.
    pub saved: Option<String>,
}

/// Where builds run.
enum Builder {
    #[cfg(not(target_arch = "wasm32"))]
    Thread(texcraft_compile::Worker),
    /// Runs on the caller's thread (wasm32, until the engine runs in a worker).
    #[cfg(target_arch = "wasm32")]
    Inline(Box<dyn texcraft_compile::Backend>, Option<Outcome>),
    /// retex's WebAssembly module in a Web Worker (the browser build).
    #[cfg(target_arch = "wasm32")]
    Web(texcraft_compile::web::WebWorker),
}

pub struct Session {
    project: Project,
    /// The folder the project was opened from or saved to.
    dir: Option<PathBuf>,
    /// Unsaved changes since the project was opened or saved.
    dirty: bool,
    /// The file and review revisions the latest version was made at (or the project opened at):
    /// changes since are in no version yet ([`Session::unversioned`]).
    versioned: (u64, u64),
    /// Files open in the editor, in tab order, and the one shown.
    open: Vec<ProjectPath>,
    current: Option<ProjectPath>,
    /// Bumped by every change to a file, so the UI and the auto-build know what is stale.
    revision: u64,
    /// The revision the last finished build was started from.
    built_revision: Option<u64>,
    /// The review revision the last build started at, for builds with suggestions.
    built_review: u64,
    /// The engine the document chose for the last build started, when the settings left it to
    /// the document and it said ([`texcraft_compile::engine_choice`]).
    built_engine: Option<texcraft_compile::engine_choice::Chosen>,
    builder: Builder,
    next_job: u64,
    last: Option<Outcome>,
    /// The PDF of the last build that wrote one, kept when a later build fails.
    pdf: Option<Arc<Vec<u8>>>,
    /// The PDF/X written beside `pdf`, when the build that wrote it was a print build, and its
    /// standard; `Err` with why there is none for a print build that wrote none.
    pdfx: Option<PrintPdf>,
    /// The kind of PDF `pdf` is.
    pdf_output: texcraft_compile::Output,
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
    /// The project's review threads.
    review: review_store::State,
    /// Who comments: set by the shell from the preferences (or for agents); none until then.
    author: Option<review::Author>,
    /// Projects kept by name, where there are no folders (the browser): then `dir` is the open
    /// project's name in it.
    library: Option<library::Library>,
    /// Templates' files (project path, bytes) by template id, fetched by the browser's page;
    /// the desktop keeps them in `templates`' folder.
    template_files: std::collections::BTreeMap<String, Vec<(String, Vec<u8>)>>,
    /// Folders made with `file.new_folder` that hold no file yet. A project stores files only, so
    /// these live in the session: shown in the file tree until a file is put in them, and not
    /// saved (a folder is written when it has a file).
    folders: std::collections::BTreeSet<ProjectPath>,
    /// The project's files in its folder as last opened or saved there (issue #2): a save removes
    /// from the folder those the project no longer has (deleted, renamed, moved), and no other.
    #[cfg(not(target_arch = "wasm32"))]
    on_disk: std::collections::BTreeSet<ProjectPath>,
    /// The git repository the project's folder is in (docs/ROADMAP.md, C8): found when the project
    /// is opened, made on its first save when there is none.
    history: Option<texcraft_history::History>,
    /// The latest automatic version this session made (after a pause, on closing, before a
    /// restore), which the next version amends: its commit (or the library's version) id. Only a
    /// version this process made is ever amended, and only while it is still the branch's tip and
    /// on no other ref (not pushed).
    own_version: Option<String>,
    /// The formulas of the file last asked for, and the engine that typesets them.
    formulas: formulas::State,
    /// What the last build that wrote an `.aux` numbered, and the document's numbers from it.
    numbering: numbering::State,
    /// The export running on its own thread (`build.export` with `background`).
    #[cfg(not(target_arch = "wasm32"))]
    export: Option<std::sync::mpsc::Receiver<Result<Value>>>,
    /// In the browser: the running export's file name, then its files, for the shell to download.
    #[cfg(target_arch = "wasm32")]
    export_name: Option<String>,
    #[cfg(target_arch = "wasm32")]
    exported: Vec<(String, Vec<u8>)>,
    /// The forge branch the project is linked to (docs/ROADMAP.md, C5).
    forge: Option<texcraft_forge::Link>,
    /// Personal access tokens by forge instance, for this session only: never saved or logged.
    forge_tokens: std::collections::BTreeMap<String, texcraft_forge::Token>,
    /// The forge operation running in the background.
    forge_task: Option<cmd::forge::Task>,
    /// How forges are reached: this build's transport, or a fake in tests.
    forge_transport: cmd::forge::Connect,
    /// The files changed since the forge branch's base, at a revision.
    forge_changes: Option<((u64, u64), Vec<texcraft_forge::FileChange>)>,
    /// Why the last pull or commit was refused, when the interface offers a way out.
    forge_problem: Option<cmd::forge::Problem>,
    /// Sign-ins to GitLab and GitHub (C5, step 4): kept in the store the shell gives.
    forge_auth: cmd::forge_auth::State,
    /// The projects and branches looked up for the Open dialog, and GitHub's listing kept.
    forge_lookups: cmd::forge_pick::State,
    /// The push, pull or new repository running in the background (docs/forges.md, step 5).
    git_task: Option<cmd::git::Task>,
    /// Import a Project's check of a git address and clone (C11).
    import: cmd::clone::State,
    /// How git's smart HTTP protocol reaches servers: over this build's transport, or a fake.
    #[cfg(not(target_arch = "wasm32"))]
    git_http: cmd::git::GitHttp,
    /// AI assistance: the connection, its requests and their answers (docs/ROADMAP.md, AI1, AI2).
    ai: ai::State,
    /// Spell checking: the dictionary, the word lists and each file's words.
    spelling: spelling::State,
    /// The build messages hidden by kind for this project.
    hidden: hidden::State,
    grammar: grammar::State,
    /// Where each kind of build output was last saved (`build.save_output`).
    saved_outputs: std::collections::BTreeMap<&'static str, PathBuf>,
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
            versioned: (0, 0),
            revision: 0,
            built_revision: None,
            built_review: 0,
            built_engine: None,
            builder,
            next_job: 1,
            last: None,
            pdf: None,
            pdf_output: texcraft_compile::Output::Screen,
            pdfx: None,
            settings: Settings::default(),
            roots: None,
            language: language::Language::new(None),
            sync: None,
            templates: None,
            edits: edits::Log::default(),
            review: review_store::State::default(),
            author: None,
            library: None,
            template_files: std::collections::BTreeMap::new(),
            folders: std::collections::BTreeSet::new(),
            #[cfg(not(target_arch = "wasm32"))]
            on_disk: std::collections::BTreeSet::new(),
            history: None,
            own_version: None,
            formulas: formulas::State::default(),
            numbering: numbering::State::default(),
            #[cfg(not(target_arch = "wasm32"))]
            export: None,
            #[cfg(target_arch = "wasm32")]
            export_name: None,
            #[cfg(target_arch = "wasm32")]
            exported: Vec::new(),
            forge: None,
            forge_tokens: std::collections::BTreeMap::new(),
            forge_task: None,
            forge_transport: std::sync::Arc::new(texcraft_forge::transport),
            forge_changes: None,
            forge_problem: None,
            forge_auth: cmd::forge_auth::State::default(),
            forge_lookups: cmd::forge_pick::State::default(),
            git_task: None,
            import: cmd::clone::State::default(),
            #[cfg(not(target_arch = "wasm32"))]
            git_http: cmd::git::default_http(),
            ai: ai::State::default(),
            spelling: spelling::State::default(),
            hidden: hidden::State::default(),
            grammar: grammar::State::default(),
            saved_outputs: std::collections::BTreeMap::new(),
        }
    }

    /// A session for the browser, building with retex's module in the Web Worker whose script
    /// is at `worker_url`.
    #[cfg(target_arch = "wasm32")]
    pub fn in_browser(worker_url: &str) -> Self {
        let mut s = Self::new();
        s.builder = Builder::Web(texcraft_compile::web::WebWorker::new(worker_url));
        s.library = Some(library::Library::default());
        s
    }

    /// Keeps projects by name in `library` instead of folders (the browser; tests).
    pub fn use_library(&mut self, library: library::Library) {
        self.library = Some(library);
    }

    pub fn library(&self) -> Option<&library::Library> {
        self.library.as_ref()
    }

    /// Keeps the files of template `id` (project paths and bytes, its `TEMPLATE-LICENSE` among
    /// them), fetched by the shell, for `project.from_template`.
    pub fn keep_template(&mut self, id: String, files: Vec<(String, Vec<u8>)>) {
        self.template_files.insert(id, files);
    }

    /// Whether template `id`'s files were kept with [`Session::keep_template`].
    pub fn has_template(&self, id: &str) -> bool {
        self.template_files.contains_key(id)
    }

    /// The library's changes since the last call, for the shell to store.
    pub fn take_library_changes(&mut self) -> Vec<library::Change> {
        self.library.as_mut().map(library::Library::take_changes).unwrap_or_default()
    }

    /// The shell could not store `change` (see [`library::out_of_room`] for why): what the library
    /// holds in memory is not kept, so the open project, when it is the change's, has unsaved
    /// changes again; the next save stores it all again, and leaving the page warns first.
    pub fn not_stored(&mut self, change: &library::Change) {
        let open = self.library.is_some() && self.dir.as_ref().and_then(|d| d.to_str()) == Some(change.project());
        if open && !matches!(change, library::Change::Deleted(_)) {
            self.dirty = true;
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

    /// The folders made in this session that no file is in yet (`file.new_folder`).
    pub fn empty_folders(&self) -> impl Iterator<Item = &ProjectPath> {
        self.folders.iter().filter(|f| !self.project.paths().any(|p| p.as_str().starts_with(&format!("{}/", f.as_str()))))
    }

    pub fn dir(&self) -> Option<&PathBuf> {
        self.dir.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether the project changed since its latest version (or since it was opened): saved or
    /// not, those changes are in no version of its history yet.
    pub fn unversioned(&self) -> bool {
        (self.revision, self.review.revision) != self.versioned
    }

    /// The session's versions were sent (pushed, synced): none of them is amended any more.
    pub(crate) fn versions_sent(&mut self) {
        self.own_version = None;
    }

    /// The project as it is now is the latest version.
    pub(crate) fn mark_versioned(&mut self) {
        self.versioned = (self.revision, self.review.revision);
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

    /// The engine the document chose for the last build started (Settings › Building › Engine
    /// "Chosen from the document"), and why; none when the settings name one, or the document
    /// did not say (retex then chooses).
    pub fn built_engine(&self) -> Option<&texcraft_compile::engine_choice::Chosen> {
        self.built_engine.as_ref()
    }

    /// The engine the document chooses now, as [`Self::built_engine`] would be for a build of
    /// the project as it is.
    pub fn engine_choice(&self) -> Option<texcraft_compile::engine_choice::Chosen> {
        if self.settings.build.engine != texcraft_compile::Engine::Auto {
            return None;
        }
        let main = self.project.main()?;
        texcraft_compile::engine_choice::choose_with(main, |p| self.project.text(p).ok().map(str::to_owned))
    }

    pub fn pdf(&self) -> Option<&Arc<Vec<u8>>> {
        self.pdf.as_ref()
    }

    /// The PDF to save or export: a print build's PDF/X, or the PDF; and its kind (`screen`,
    /// `PDF/UA`, `PDF/X-4`). A print build that wrote no PDF/X has nothing to save.
    pub fn pdf_to_save(&self) -> Result<(Arc<Vec<u8>>, &'static str)> {
        let pdf = self.pdf.clone().ok_or("There is no PDF yet: compile the project first.")?;
        match &self.pdfx {
            None if self.pdf_output == texcraft_compile::Output::Accessible => Ok((pdf, "PDF/UA")),
            None => Ok((pdf, "screen")),
            Some(Ok((standard, pdfx))) => Ok((pdfx.clone(), standard.name())),
            Some(Err(why)) => Err(why.clone()),
        }
    }

    /// The files the last build made: its PDF, a print build's PDF/X, the SyncTeX data and the
    /// raw log, each named after the main document. A build that failed has its log only.
    pub fn outputs(&self) -> Vec<BuildOutput> {
        OUTPUT_KINDS
            .iter()
            .filter_map(|kind| {
                let bytes = self.output_data(kind)?.len();
                let saved = self.saved_outputs.get(kind).map(|p| p.display().to_string());
                Some(BuildOutput { kind, name: self.output_name(kind), bytes, saved })
            })
            .collect()
    }

    /// The contents of the last build's output of `kind`, if it made one.
    pub(crate) fn output_data(&self, kind: &str) -> Option<Arc<Vec<u8>>> {
        let last = self.last.as_ref()?;
        match kind {
            "pdf" => last.pdf.clone(),
            "pdfx" => last.preflight.as_ref()?.pdfx.clone(),
            "synctex" => last.synctex.clone(),
            "log" => (!last.log.is_empty()).then(|| Arc::new(last.log.clone().into_bytes())),
            _ => None,
        }
    }

    /// The file name of an output of `kind`: the main document's name with the output's
    /// extension (`main.pdf`, `main-print.pdf` for the PDF/X, `main.synctex`, `main.log`).
    pub(crate) fn output_name(&self, kind: &str) -> String {
        let stem = self.project.main().map(|m| m.file_name().rsplit_once('.').map_or(m.file_name(), |(s, _)| s)).unwrap_or("document");
        match kind {
            "pdfx" => format!("{stem}-print.pdf"),
            other => format!("{stem}.{other}"),
        }
    }

    /// Notes where an output of `kind` was saved, for Show in Folder.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn output_saved(&mut self, kind: &'static str, path: PathBuf) {
        self.saved_outputs.insert(kind, path);
    }

    /// Where the output of `kind` was last saved in this session.
    pub fn saved_output(&self, kind: &str) -> Option<&std::path::Path> {
        self.saved_outputs.get(kind).map(PathBuf::as_path)
    }

    pub fn is_building(&self) -> bool {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.running().is_some(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => false,
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.running().is_some(),
        }
    }

    /// Whether the engine can stop a running build.
    pub fn can_cancel(&self) -> bool {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.can_cancel(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) | Builder::Web(_) => false,
        }
    }

    /// Stops the running build; false when none runs or the engine cannot stop one.
    pub(crate) fn stop_build(&mut self) -> bool {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.cancel(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => false,
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.cancel(),
        }
    }

    /// The running build's progress and how long it has run, in seconds.
    pub fn progress(&mut self) -> Option<(Progress, f64)> {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.progress(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => None,
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.progress(),
        }
    }

    /// The running build's draft PDF: that of a pass that is not the last, written while the
    /// next passes run (its references may be undefined). None once the build has ended: then
    /// [`Session::pdf`] is the PDF to show.
    pub fn draft_pdf(&mut self) -> Option<&Draft> {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.draft(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(..) => None,
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.draft(),
        }
    }

    pub fn backend_name(&self) -> String {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.backend_name().to_owned(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(b, _) => b.name(),
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.backend_name().to_owned(),
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
            #[cfg(target_arch = "wasm32")]
            Builder::Web(_) => false,
        }
    }

    /// The engine's exporter, when it can export to other formats than its PDF.
    pub(crate) fn exporter(&self) -> Option<texcraft_compile::Exporter> {
        match &self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.exporter(),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(b, _) => b.exporter(),
            #[cfg(target_arch = "wasm32")]
            Builder::Web(_) => None,
        }
    }

    /// Whether an export runs on its thread.
    pub fn exporting(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.export.is_some();
        #[cfg(target_arch = "wasm32")]
        match &self.builder {
            Builder::Web(w) => w.exporting(),
            Builder::Inline(..) => false,
        }
    }

    /// In the browser: starts exporting the project as `format` in the engine's worker, the file
    /// named `name`; [`Session::poll_export`] says when it is done.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn start_web_export(&mut self, format: texcraft_compile::Export, name: String) -> Result<()> {
        let job = self.export_job()?;
        match &mut self.builder {
            Builder::Web(w) => w.export(&job, format.extension())?,
            Builder::Inline(..) => return Err("This build of TeXCraft cannot export to other formats than PDF.".to_owned()),
        }
        self.export_name = Some(name);
        Ok(())
    }

    /// In the browser: `name` for the shell to download (a PDF exported as it is).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn hand_out(&mut self, name: String, data: Vec<u8>) {
        self.exported = vec![(name, data)];
    }

    /// In the browser: the last export's files (the document first, then its pictures), for the
    /// shell to hand to the browser as downloads.
    pub fn take_exported_files(&mut self) -> Vec<(String, Vec<u8>)> {
        #[cfg(target_arch = "wasm32")]
        return std::mem::take(&mut self.exported);
        #[cfg(not(target_arch = "wasm32"))]
        Vec::new()
    }

    /// The outcome of the export running on its thread, once it has ended (what was written, or
    /// why it failed).
    pub fn poll_export(&mut self) -> Option<Result<Value>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let outcome = match self.export.as_ref()?.try_recv() {
                Ok(outcome) => outcome,
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("The export stopped unexpectedly.".to_owned()),
            };
            self.export = None;
            Some(outcome)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let outcome = match &mut self.builder {
                Builder::Web(w) => w.poll_export()?,
                Builder::Inline(..) => return None,
            };
            let name = self.export_name.take().unwrap_or_else(|| "document".to_owned());
            Some(outcome.and_then(|exported| {
                let names: Vec<String> = std::iter::once(name.clone()).chain(exported.media.iter().map(|(n, _)| n.clone())).collect();
                // A download is one file: a document with pictures (Markdown) goes as a zip of
                // both, so its links to them still work.
                let download = if exported.media.is_empty() {
                    (name, exported.data)
                } else {
                    let mut zip = craft_zip::Writer::new();
                    let zipped = |e: craft_zip::Error| format!("The export could not be zipped: {e}");
                    zip.add(&name, &exported.data, craft_zip::Compression::Deflated).map_err(zipped)?;
                    for (path, data) in &exported.media {
                        zip.add(path, data, craft_zip::Compression::Deflated).map_err(zipped)?;
                    }
                    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
                    (format!("{stem}.zip"), zip.finish().map_err(zipped)?)
                };
                let result = serde_json::json!({"path": download.0, "files": names});
                self.exported = vec![download];
                Ok(result)
            }))
        }
    }

    /// Whether the engine converts projects to other formats than its PDF (`build.export`).
    pub fn can_convert(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if matches!(self.builder, Builder::Web(_)) {
            return true;
        }
        self.exporter().is_some()
    }

    /// Which project this is, for the engine (see [`Job::project`]): its name in the browser's
    /// library or its folder, or for a project kept nowhere yet, its main document's path.
    fn project_key(&self) -> Option<String> {
        match &self.dir {
            Some(dir) => Some(format!("project:{}", dir.to_string_lossy())),
            None => self.project.main().map(|main| format!("untitled:{}", main.as_str())),
        }
    }

    /// A job of the current sources and build options, for an export.
    pub(crate) fn export_job(&self) -> Result<Job> {
        let mut job = Job::from_project(&self.project, 0).ok_or("The project has no main document to export.")?;
        job.options = self.settings.build;
        job.choose_engine();
        job.project = self.project_key();
        Ok(job)
    }

    /// Whether the main document is a beamer presentation (`\documentclass[…]{beamer}`), so
    /// presentation formats come first among the exports.
    pub fn is_presentation(&self) -> bool {
        let Some(main) = self.project.main() else { return false };
        self.project.text(main).is_ok_and(class_is_beamer)
    }

    /// Deletes the build files (not the PDF shown); the next build starts from scratch.
    pub(crate) fn clean_build(&mut self) -> Result<()> {
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.clean()?,
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(b, _) => b.cleaner().ok_or("This typesetting engine keeps no build files.")?()?,
            #[cfg(target_arch = "wasm32")]
            Builder::Web(_) => return Err("This typesetting engine keeps no build files.".to_owned()),
        }
        self.built_revision = None;
        Ok(())
    }

    /// Whether the sources changed since the last build started.
    pub fn is_stale(&self) -> bool {
        self.built_revision != Some(self.revision) || (self.settings.with_suggestions && self.built_review != self.review.revision)
    }

    /// Builds with the open suggestions applied, or not (`build.suggestions`); a change makes the
    /// PDF stale.
    pub(crate) fn set_with_suggestions(&mut self, on: bool) {
        if on != self.settings.with_suggestions {
            self.settings.with_suggestions = on;
            self.built_revision = None;
        }
    }

    /// The project as it would be with every open suggestion that can be applied, for a build
    /// with suggestions; the files are not changed.
    fn with_suggestions(&self) -> Project {
        let mut project = self.project.clone();
        let filter = review::Filter { open: Some(true), kind: Some(review::Kind::Suggestion), ..Default::default() };
        let list = self.review.review.to_accept_all(&filter);
        let mut rest = list.as_slice();
        while let Some(first) = rest.first() {
            let n = rest.iter().take_while(|a| a.path == first.path).count();
            let (group, next) = rest.split_at(n);
            rest = next;
            let changes = group.iter().map(|a| Change { start: a.start, end: a.end, text: a.replacement.clone() }).collect();
            let (Ok(path), Ok(tx)) = (ProjectPath::new(&first.path), Transaction::new(changes)) else { continue };
            let made = project.text(&path).ok().and_then(|text| edits::apply(text, &tx));
            if let Some(text) = made {
                let _ = project.set(&path, texcraft_project::Contents::Text(text));
            }
        }
        project
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
        // The anchors follow; an undo or redo may also take back or make again an accept.
        let changed = if matches!(origin, Origin::Undo | Origin::Redo) {
            let changes: Vec<(usize, usize, String)> = tx.changes().iter().map(|c| (c.start, c.end, c.text.clone())).collect();
            let now = self.now();
            self.review.review.follow_undo(path.as_str(), &changes, self.author.as_ref(), now)
        } else {
            let changes: Vec<(usize, usize, usize)> = tx.changes().iter().map(|c| (c.start, c.end, c.text.chars().count())).collect();
            self.review.review.follow(path.as_str(), &changes)
        };
        self.edits.push(FileEdit { revision: self.revision, path: path.clone(), changes: tx.clone(), origin });
        // Suggestions the edit outdated, reopened or accepted again: the edit is made either way,
        // and a review file that could not be written is reported with the review's problems.
        for id in changed {
            if let Err(e) = self.review_changed(id) {
                self.review.problems.push(e);
            }
        }
        Ok(())
    }

    /// The project's review threads, their anchors on the text as it is now.
    pub fn review(&self) -> &review::Review {
        &self.review.review
    }

    /// Goes up with every change of a thread (not with its anchor following an edit).
    pub fn review_revision(&self) -> u64 {
        self.review.revision
    }

    /// Review files of the project that could not be read, with why.
    pub fn review_problems(&self) -> &[String] {
        &self.review.problems
    }

    /// Who comments from now on (the preferences' name and key).
    pub fn set_author(&mut self, author: Option<review::Author>) {
        self.author = author;
    }

    pub fn author(&self) -> Option<&review::Author> {
        self.author.as_ref()
    }

    fn author_or_refuse(&self) -> Result<review::Author> {
        self.author.clone().ok_or_else(|| "Choose the name your comments carry first (Settings › Review).".to_owned())
    }

    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    /// A new id for a thread or a message: the time and 80 random bits.
    fn new_id(&self) -> Result<review::Id> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| format!("no random source for a new id: {e}"))?;
        let millis = u64::try_from(self.now().timestamp_millis()).unwrap_or_default();
        Ok(review::Id::new(millis, u128::from_le_bytes(bytes)))
    }

    /// Thread `id` changed: it is written at once to the project's folder, or to the project's
    /// entry in the library, when the project has one.
    fn review_changed(&mut self, id: review::Id) -> Result<()> {
        self.review.revision += 1;
        let Some(dir) = self.dir.clone() else { return Ok(()) };
        let written = match &mut self.library {
            Some(library) => self.review.write_to_library(library, &dir.display().to_string(), id),
            None => {
                if let Some(roots) = &self.roots {
                    roots.check(&review_store::folder(&dir), file_access::Access::Write)?;
                }
                self.review.write(&dir, id)
            }
        };
        written.map_err(|e| format!("The review is changed but could not be saved: {e}"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Saves the review with the project into `dir`, its quotes renewed from the text.
    pub(crate) fn save_review(&mut self, dir: &std::path::Path, new_folder: bool) -> Result<()> {
        self.review.save(dir, &self.project, new_folder)
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
        self.folders.clear();
        // A project of the library has a name, not a folder on disk.
        let folder = dir.as_deref().filter(|_| self.library.is_none());
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.on_disk = if folder.is_some() { self.project.paths().cloned().collect() } else { Default::default() };
        }
        self.language = language::Language::new(folder);
        self.review = match (&self.library, &dir) {
            (Some(library), Some(name)) => review_store::State::load_from_library(library, &name.display().to_string(), &self.project),
            _ => review_store::State::load(folder, &self.project),
        };
        self.sync = None;
        self.history = None;
        self.own_version = None;
        // A project of the library keeps its forge link; any other opens unlinked.
        let link = match (&self.library, &dir) {
            (Some(library), Some(name)) => library.forge(&name.display().to_string()).cloned(),
            _ => None,
        };
        self.set_forge(link);
        self.dir = dir;
        self.dirty = false;
        self.touch();
        self.dirty = false;
        self.mark_versioned();
        self.last = None;
        self.pdf = None;
        self.pdfx = None;
        self.numbering = numbering::State::default();
        self.saved_outputs.clear();
    }

    /// Starts a build of the current sources; returns its job id.
    fn start_build(&mut self) -> Result<u64> {
        if self.roots.is_some() {
            // RFC-0015: every path a document names must be checked against the roots, and retex
            // does not yet let its host decide which files a build may read (docs/retex-changes.md, R4).
            return Err("Building is not available to agents yet: the engine cannot confine the files a document reads.".to_owned());
        }
        let id = self.next_job;
        self.next_job += 1;
        let suggested = self.settings.with_suggestions.then(|| self.with_suggestions());
        let mut job = Job::from_project(suggested.as_ref().unwrap_or(&self.project), id).ok_or("The project has no main document to build.")?;
        job.options = self.settings.build;
        self.built_engine = job.choose_engine();
        job.project = self.project_key();
        self.built_revision = Some(self.revision);
        self.built_review = self.review.revision;
        match &mut self.builder {
            #[cfg(not(target_arch = "wasm32"))]
            Builder::Thread(w) => w.submit(job),
            #[cfg(target_arch = "wasm32")]
            Builder::Inline(backend, slot) => *slot = Some(backend.compile(&job)),
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.submit(job),
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
            #[cfg(target_arch = "wasm32")]
            Builder::Web(w) => w.poll(),
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
            // A browser can't block: the build's outcome arrives through `poll`.
            #[cfg(target_arch = "wasm32")]
            Builder::Web(_) => None,
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
        if let Some(aux) = &outcome.aux {
            self.numbering.built(aux);
        }
        // A build that wrote a PDF brings its SyncTeX data; a failed one keeps the last PDF's.
        if outcome.pdf.is_some() {
            self.sync = outcome.synctex.as_deref().and_then(|d| sync::Sync::parse(d));
        }
        if let Some(pdf) = &outcome.pdf {
            self.pdf = Some(pdf.clone());
            self.pdf_output = outcome.output;
            self.pdfx = outcome.preflight.as_ref().map(|p| match &p.pdfx {
                Some(x) => Ok((p.standard, x.clone())),
                None => Err(format!("The print build wrote no {}: see the Print messages in the build log.", p.standard.name())),
            });
        }
        self.last.insert(outcome)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// A new review identity's key pair, as hex: the ed25519 public key (the [`review::Author`]'s
/// `key`) and its secret key, which the shell keeps with the preferences.
pub fn new_key_pair() -> Result<(String, String)> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| format!("no random source for a new key: {e}"))?;
    let key = ed25519_dalek::SigningKey::from_bytes(&secret);
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok((hex(key.verifying_key().as_bytes()), hex(&secret)))
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

/// Whether a LaTeX source's `\documentclass` (outside comments) is beamer.
fn class_is_beamer(text: &str) -> bool {
    for line in text.lines() {
        let line = line.split('%').next().unwrap_or("");
        let Some(at) = line.find("\\documentclass") else { continue };
        let rest = &line[at + "\\documentclass".len()..];
        let rest = rest.trim_start();
        let rest = match rest.strip_prefix('[') {
            Some(r) => r.split_once(']').map_or("", |(_, after)| after),
            None => rest,
        };
        let class = rest.trim_start().strip_prefix('{').and_then(|r| r.split_once('}')).map(|(c, _)| c.trim());
        return class == Some("beamer");
    }
    false
}
