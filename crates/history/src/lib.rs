//! A project's history as a git repository (docs/ROADMAP.md, C8): a project folder that is not in
//! a repository becomes one on its first save, and each save is a commit; a folder already in a
//! repository is used as it is, and committed to only when asked.
//!
//! A commit's tree is what the caller hands over ([`Snapshot`]: the project's files as the session
//! holds them, paths and bytes), not what is on disk, so the same rules can serve projects that
//! have no folder (the browser, later). What a project does not load is kept as it is in the
//! repository: hidden files and folders (`.gitignore`, `.github/`), symbolic links and submodules.
//!
//! Git on disk is desktop-only; in the browser, where projects have no folder, [`store`] keeps
//! the same history as values (git's blob ids, versions that share unchanged files).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(not(target_arch = "wasm32"))]
mod git;
#[cfg(not(target_arch = "wasm32"))]
pub use git::History;

#[cfg(target_arch = "wasm32")]
mod stub;
#[cfg(target_arch = "wasm32")]
pub use stub::History;

pub mod store;

/// A project's history as it is read: a repository on disk, or the versions a shell keeps.
pub trait Versioned {
    /// The versions that changed the project, newest first, at most `limit`.
    fn log(&self, limit: usize) -> Result<Vec<Entry>>;
    /// File `path` of the project at version `commit` (a full id); none when it had no such file.
    fn file_at(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>>;
    /// The project's files at version `commit`: those a project loads, not hidden ones.
    fn files_at(&self, commit: &str) -> Result<Vec<(String, Vec<u8>)>>;
    /// The version before `commit`; none for a first one.
    fn parent_of(&self, commit: &str) -> Result<Option<String>>;
}

impl Versioned for History {
    fn log(&self, limit: usize) -> Result<Vec<Entry>> {
        History::log(self, limit)
    }
    fn file_at(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>> {
        History::file_at(self, commit, path)
    }
    fn files_at(&self, commit: &str) -> Result<Vec<(String, Vec<u8>)>> {
        History::files_at(self, commit)
    }
    fn parent_of(&self, commit: &str) -> Result<Option<String>> {
        History::parent_of(self, commit)
    }
}

/// What a call returns: the value, or an English message saying what went wrong.
pub type Result<T> = std::result::Result<T, String>;

/// The `.gitignore` written into a repository TeXCraft makes: the build files.
pub const GITIGNORE: &str = ".retex/\n";

/// The repository configuration key that says TeXCraft made the repository.
pub const OURS_KEY: &str = "texcraft.history";

/// Who owns the repository a project is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Made by TeXCraft for this project: every save is a commit.
    Ours,
    /// Made by someone else (the project's own, or a folder above it): commits only when asked.
    Theirs,
}

impl Owner {
    pub fn as_str(self) -> &'static str {
        match self {
            Owner::Ours => "ours",
            Owner::Theirs => "theirs",
        }
    }
}

/// The files to commit, by path from the project's folder (`/`-separated).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub files: Vec<(String, Vec<u8>)>,
    /// Hidden folders (`.texcraft/review`) whose files come from `files` too: what is in them in
    /// the repository and not in `files` is deleted. Other hidden entries keep their versions.
    pub owned_dirs: Vec<String>,
}

/// Who made a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub email: String,
}

impl Person {
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self { name: name.into(), email: email.into() }
    }
}

/// How a file changed in a commit, against its first parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
        }
    }
}

/// One commit of the project's history.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The full hexadecimal id.
    pub id: String,
    /// Its first parent's full id; none for the first commit.
    pub parent: Option<String>,
    pub time: chrono::DateTime<chrono::Utc>,
    pub author: String,
    pub message: String,
    /// The files of the project that changed, by path from the project's folder, sorted.
    pub changes: Vec<(String, ChangeKind)>,
}

/// Whether a project path is hidden (one of its parts starts with a dot): projects do not load
/// those, so a commit keeps their versions from the repository.
pub fn is_hidden(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}
