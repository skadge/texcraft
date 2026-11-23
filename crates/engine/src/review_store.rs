//! The project's review (docs/collaboration-roadmap.md, R1): its threads, loaded with the project
//! from `.texcraft/review/` and written there, one file per thread, when a thread changes. The
//! anchors follow every edit of the session ([`crate::Session::apply_edit`]); their quotes are
//! renewed when the project is saved. A project without a folder keeps its review in memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use texcraft_project::Project;
use texcraft_review::{Id, Review, format};

/// The review of the open project and what is on disk of it.
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

/// The folder of the review files in a project folder.
pub(crate) fn folder(dir: &Path) -> PathBuf {
    dir.join(format::DIR)
}

impl State {
    /// The review kept in `dir` (none without a folder), its anchors placed on `project`'s text.
    pub(crate) fn load(dir: Option<&Path>, project: &Project) -> Self {
        let mut state = Self::default();
        let Some(dir) = dir else { return state };
        let Ok(entries) = std::fs::read_dir(folder(dir)) else { return state };
        let mut names: Vec<PathBuf> =
            entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        names.sort();
        for file in names {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let text = match std::fs::read_to_string(&file) {
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

    /// Writes thread `id` to its file in `dir`, or deletes the file when the thread is gone.
    pub(crate) fn write(&mut self, dir: &Path, id: Id) -> Result<(), String> {
        let file = folder(dir).join(format::file_name(id));
        match self.review.get(id) {
            Some(thread) => {
                let text = format::write(thread).map_err(|e| e.to_string())?;
                if self.written.get(&id) == Some(&text) {
                    return Ok(());
                }
                std::fs::create_dir_all(folder(dir)).map_err(|e| format!("creating {}: {e}", folder(dir).display()))?;
                std::fs::write(&file, &text).map_err(|e| format!("writing {}: {e}", file.display()))?;
                self.written.insert(id, text);
            }
            None => {
                if file.exists() {
                    std::fs::remove_file(&file).map_err(|e| format!("deleting {}: {e}", file.display()))?;
                }
                self.written.remove(&id);
            }
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Renews every quote from `project`'s text and writes the threads that changed (saving the
    /// project into `dir`; a new folder gets every thread).
    pub(crate) fn save(&mut self, dir: &Path, project: &Project, new_folder: bool) -> Result<(), String> {
        for (path, contents) in project.files() {
            if let texcraft_project::Contents::Text(text) = contents {
                self.review.requote(path.as_str(), text);
            }
        }
        if new_folder {
            self.written.clear();
        }
        let ids: Vec<Id> = self.review.threads().iter().map(|t| t.id).collect();
        ids.into_iter().try_for_each(|id| self.write(dir, id))
    }
}
