//! The project's review (docs/collaboration-roadmap.md, R1): its threads, loaded with the project
//! and written, one file per thread, when a thread changes: into `.texcraft/review/` of the
//! project's folder, or into the project's entry of the library (the browser), as the same text.
//! The anchors follow every edit of the session ([`crate::Session::apply_edit`]); their quotes
//! are renewed when the project is saved. A project not saved yet keeps its review in memory.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use texcraft_project::Project;
use texcraft_review::{Id, Review, format};

use crate::library::Library;

/// The review of the open project and what is stored of it.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) review: Review,
    /// Each thread's file text as last read or written, so unchanged threads are not rewritten.
    written: HashMap<Id, String>,
    /// Files that could not be read (a newer format, a damaged file), with why.
    pub(crate) problems: Vec<String>,
    /// Goes up with every change of a thread (not with anchors moving), for the interface.
    pub(crate) revision: u64,
}

/// What to do to a thread's file.
enum Write {
    /// Write this text to the file of this name.
    Put(String, String),
    /// Delete the file of this name, if there is one.
    Remove(String),
}

/// The folder of the review files in a project folder.
pub(crate) fn folder(dir: &Path) -> PathBuf {
    dir.join(format::DIR)
}

impl State {
    /// The review kept in `dir` (none without a folder), its anchors placed on `project`'s text.
    pub(crate) fn load(dir: Option<&Path>, project: &Project) -> Self {
        let Some(dir) = dir else { return Self::default() };
        let Ok(entries) = std::fs::read_dir(folder(dir)) else { return Self::default() };
        let mut names: Vec<PathBuf> =
            entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        names.sort();
        let files = names.into_iter().map(|file| {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let text = std::fs::read_to_string(&file).map_err(|e| e.to_string());
            (name, text)
        });
        Self::read(files, project)
    }

    /// The review kept with project `name` of `library`, its anchors placed on `project`'s text.
    pub(crate) fn load_from_library(library: &Library, name: &str, project: &Project) -> Self {
        Self::read(library.review(name).iter().map(|(file, text)| (file.clone(), Ok(text.clone()))), project)
    }

    /// The threads of `files` (each file's name and its text, or why it could not be read).
    pub(crate) fn read(files: impl IntoIterator<Item = (String, Result<String, String>)>, project: &Project) -> Self {
        let mut state = Self::default();
        for (name, text) in files {
            let text = match text {
                Ok(t) => t,
                Err(e) => {
                    state.problems.push(format!("{name}: {e}"));
                    continue;
                }
            };
            match format::read(&text) {
                Ok(mut thread) => {
                    let source = texcraft_project::ProjectPath::new(&thread.anchor.path).ok().and_then(|p| project.text(&p).ok().map(str::to_owned));
                    match source {
                        Some(source) => thread.anchor.place(&source),
                        None => thread.anchor.placement = texcraft_review::Placement::Unanchored,
                    }
                    state.written.insert(thread.id, text);
                    state.review.insert(thread);
                }
                Err(e) => state.problems.push(format!("{name}: {e}")),
            }
        }
        state
    }

    /// What has to be written for thread `id`; `None` when its file is up to date.
    fn next_write(&self, id: Id) -> Result<Option<Write>, String> {
        let file = format::file_name(id);
        match self.review.get(id) {
            Some(thread) => {
                let text = format::write(thread).map_err(|e| e.to_string())?;
                Ok((self.written.get(&id) != Some(&text)).then_some(Write::Put(file, text)))
            }
            None => Ok(Some(Write::Remove(file))),
        }
    }

    /// `write` for thread `id` was done.
    fn wrote(&mut self, id: Id, write: Write) {
        match write {
            Write::Put(_, text) => {
                self.written.insert(id, text);
            }
            Write::Remove(_) => {
                self.written.remove(&id);
            }
        }
    }

    /// Writes thread `id` to its file in `dir`, or deletes the file when the thread is gone.
    pub(crate) fn write(&mut self, dir: &Path, id: Id) -> Result<(), String> {
        let Some(write) = self.next_write(id)? else { return Ok(()) };
        match &write {
            Write::Put(name, text) => {
                let file = folder(dir).join(name);
                std::fs::create_dir_all(folder(dir)).map_err(|e| format!("creating {}: {e}", folder(dir).display()))?;
                std::fs::write(&file, text).map_err(|e| format!("writing {}: {e}", file.display()))?;
            }
            Write::Remove(name) => {
                let file = folder(dir).join(name);
                if file.exists() {
                    std::fs::remove_file(&file).map_err(|e| format!("deleting {}: {e}", file.display()))?;
                }
            }
        }
        self.wrote(id, write);
        Ok(())
    }

    /// Writes thread `id` into project `name` of `library`, or removes it when the thread is gone.
    pub(crate) fn write_to_library(&mut self, library: &mut Library, name: &str, id: Id) -> Result<(), String> {
        let Some(write) = self.next_write(id)? else { return Ok(()) };
        match &write {
            Write::Put(file, text) => library.put_thread(name, file, text)?,
            Write::Remove(file) => library.remove_thread(name, file)?,
        }
        self.wrote(id, write);
        Ok(())
    }

    /// Renews every quote from `project`'s text.
    fn requote(&mut self, project: &Project) {
        for (path, contents) in project.files() {
            if let texcraft_project::Contents::Text(text) = contents {
                self.review.requote(path.as_str(), text);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Renews every quote from `project`'s text and writes the threads that changed (saving the
    /// project into `dir`; a new folder gets every thread).
    pub(crate) fn save(&mut self, dir: &Path, project: &Project, new_folder: bool) -> Result<(), String> {
        self.requote(project);
        if new_folder {
            self.written.clear();
        }
        let ids: Vec<Id> = self.review.threads().iter().map(|t| t.id).collect();
        ids.into_iter().try_for_each(|id| self.write(dir, id))
    }

    /// Every thread's file, by name, as the threads are now (no quote renewed).
    pub(crate) fn texts(&self) -> Result<BTreeMap<String, String>, String> {
        self.review.threads().iter().map(|t| Ok((format::file_name(t.id), format::write(t).map_err(|e| e.to_string())?))).collect()
    }

    /// Puts in the thread of a file's `text` (from a forge's branch), replacing the one with its
    /// id, its anchor placed on `project`'s text: its id.
    pub(crate) fn put(&mut self, text: &str, project: &Project) -> Result<Id, String> {
        let mut thread = format::read(text).map_err(|e| e.to_string())?;
        let source = texcraft_project::ProjectPath::new(&thread.anchor.path).ok().and_then(|p| project.text(&p).ok().map(str::to_owned));
        match source {
            Some(source) => thread.anchor.place(&source),
            None => thread.anchor.placement = texcraft_review::Placement::Unanchored,
        }
        let id = thread.id;
        self.review.insert(thread);
        Ok(id)
    }

    /// Drops the thread kept in file `name` (deleted on a forge's branch): its id, if it was one.
    pub(crate) fn drop_file(&mut self, name: &str) -> Option<Id> {
        let id = self.review.threads().iter().map(|t| t.id).find(|id| format::file_name(*id) == name)?;
        let mut kept = Review::default();
        for thread in self.review.threads().iter().filter(|t| t.id != id) {
            kept.insert(thread.clone());
        }
        self.review = kept;
        Some(id)
    }

    /// Renews every quote from `project`'s text and gives every thread's file, by name, to save
    /// with the project in the library.
    pub(crate) fn files(&mut self, project: &Project) -> Result<BTreeMap<String, String>, String> {
        self.requote(project);
        self.written.clear();
        let mut files = BTreeMap::new();
        for thread in self.review.threads() {
            let text = format::write(thread).map_err(|e| e.to_string())?;
            files.insert(format::file_name(thread.id), text.clone());
            self.written.insert(thread.id, text);
        }
        Ok(files)
    }
}
