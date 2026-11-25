//! Projects kept by name, for a shell that has no folders: the browser, where the web app keeps
//! them in IndexedDB. The session keeps the library in memory, so its commands stay synchronous.
//! The shell fills it at start ([`Library::insert`]) and stores each change it takes
//! ([`Library::take_changes`]).
//!
//! Each project keeps its history (docs/ROADMAP.md, C8): a version per save that changed it, as
//! [`texcraft_history::store`] keeps them; the shell stores the versions and their files' bytes
//! apart from the project ([`Change::Versions`]), so a save adds only what is new.
//!
//! A project of the library has its name as the session's folder (`Session::dir`), so the
//! interface's title, recent projects and thumbnails work as they do for folders.

use std::collections::BTreeMap;

use texcraft_history::Snapshot;
use texcraft_history::store::{Version, Versions};
use texcraft_project::{Contents, Project, ProjectPath};

use crate::Result;

/// Longest project name, in characters.
const MAX_NAME: usize = 100;

/// A project as the library keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// Paths in the project and their bytes.
    pub files: Vec<(String, Vec<u8>)>,
    pub main: Option<String>,
    /// When it was last saved, in milliseconds since 1970 (UTC).
    pub saved: i64,
    /// The review's threads: each thread's file name and text, as a project folder keeps them in
    /// `.texcraft/review/` (docs/review-format.md).
    pub review: BTreeMap<String, String>,
    /// Its saved versions and their files.
    pub history: Versions,
    /// The forge branch it was opened from, with the base its changes are counted from (C5);
    /// never a token.
    pub forge: Option<texcraft_forge::Link>,
}

/// A change the shell has to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The project of this name, with its review, replacing what was stored of it. Its history is
    /// left empty here: what the history gains comes as [`Change::Versions`].
    Saved(String, Box<Stored>),
    /// Versions to add to the history of a project, with the files' bytes (by blob id) the stored
    /// history does not have yet.
    Versions {
        project: String,
        versions: Vec<Version>,
        blobs: Vec<(String, Vec<u8>)>,
    },
    Deleted(String),
    /// A review thread of a project changed: its file name and new text, or `None` when it was
    /// deleted. It is stored at once, as a project folder's thread file is written at once,
    /// whether or not the project's files are saved.
    Thread {
        project: String,
        file: String,
        text: Option<String>,
    },
}

#[derive(Debug, Default)]
pub struct Library {
    projects: BTreeMap<String, Stored>,
    changes: Vec<Change>,
}

/// `name` as a project's name: trimmed, not empty, one line, with no `/` or `\` (it also shows
/// where a folder's path would).
pub fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the project a name.".to_owned());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("A project's name has at most {MAX_NAME} characters."));
    }
    if name.contains(['/', '\\']) || name.chars().any(char::is_control) {
        return Err("A project's name cannot have a slash or a line break.".to_owned());
    }
    Ok(name.to_owned())
}

impl Library {
    /// A project the shell stored earlier; not a change to store.
    pub fn insert(&mut self, name: String, project: Stored) {
        self.projects.insert(name, project);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.projects.contains_key(name)
    }

    /// The projects' names, the last saved first.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<(&String, i64)> = self.projects.iter().map(|(n, p)| (n, p.saved)).collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        names.into_iter().map(|(n, _)| n.clone()).collect()
    }

    /// The project named `name`.
    pub fn load(&self, name: &str) -> Result<Project> {
        let stored = self.projects.get(name).ok_or_else(|| format!("There is no project named {name} in this browser."))?;
        let mut project = Project::new();
        for (path, bytes) in &stored.files {
            let path = ProjectPath::new(path).map_err(|e| e.to_string())?;
            project.add(path, Contents::from_bytes(bytes.clone())).map_err(|e| e.to_string())?;
        }
        if let Some(main) = &stored.main
            && let Ok(main) = ProjectPath::new(main)
            && project.get(&main).is_some()
        {
            project.set_main(&main).map_err(|e| e.to_string())?;
        }
        Ok(project)
    }

    /// The review threads kept with project `name` (file names and texts); none for a project
    /// the library does not have.
    pub fn review(&self, name: &str) -> &BTreeMap<String, String> {
        static NONE: BTreeMap<String, String> = BTreeMap::new();
        self.projects.get(name).map_or(&NONE, |p| &p.review)
    }

    /// Keeps `project`, its `review` (thread files by name) and the forge branch it is linked to
    /// as `name`, replacing the project of that name.
    pub fn save(&mut self, name: &str, project: &Project, review: BTreeMap<String, String>, saved: i64, forge: Option<texcraft_forge::Link>) {
        let stored = Stored {
            files: project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect(),
            main: project.main().map(|m| m.as_str().to_owned()),
            saved,
            review,
            history: Versions::default(),
            forge,
        };
        self.changes.push(Change::Saved(name.to_owned(), Box::new(stored.clone())));
        // The project of that name keeps its history.
        let history = self.projects.remove(name).map(|p| p.history).unwrap_or_default();
        self.projects.insert(name.to_owned(), Stored { history, ..stored });
    }

    /// The forge branch project `name` is linked to, if any.
    pub fn forge(&self, name: &str) -> Option<&texcraft_forge::Link> {
        self.projects.get(name).and_then(|p| p.forge.as_ref())
    }

    /// The history of project `name`; none for a project the library does not have.
    pub fn history(&self, name: &str) -> Option<&Versions> {
        self.projects.get(name).map(|p| &p.history)
    }

    /// Records `snapshot` as a version of project `name`: its id, none when nothing changed since
    /// the latest version.
    pub fn record(
        &mut self,
        name: &str,
        snapshot: &Snapshot,
        message: &str,
        author: &str,
        time: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<String>> {
        let project = self.projects.get_mut(name).ok_or_else(|| format!("There is no project named {name} in this browser."))?;
        let Some(recorded) = project.history.record(snapshot, message, author, time)? else { return Ok(None) };
        let id = recorded.version.id.clone();
        self.changes.push(Change::Versions { project: name.to_owned(), versions: vec![recorded.version], blobs: recorded.blobs });
        Ok(Some(id))
    }

    /// Keeps review thread `file` of project `name` with `text`.
    pub fn put_thread(&mut self, name: &str, file: &str, text: &str) -> Result<()> {
        let project = self.projects.get_mut(name).ok_or_else(|| format!("There is no project named {name} in this browser."))?;
        project.review.insert(file.to_owned(), text.to_owned());
        self.changes.push(Change::Thread { project: name.to_owned(), file: file.to_owned(), text: Some(text.to_owned()) });
        Ok(())
    }

    /// Removes review thread `file` of project `name`, if it has it.
    pub fn remove_thread(&mut self, name: &str, file: &str) -> Result<()> {
        let project = self.projects.get_mut(name).ok_or_else(|| format!("There is no project named {name} in this browser."))?;
        if project.review.remove(file).is_some() {
            self.changes.push(Change::Thread { project: name.to_owned(), file: file.to_owned(), text: None });
        }
        Ok(())
    }

    /// Keeps the project `from` as `to`, unchanged otherwise; refuses a name already taken.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        if self.projects.contains_key(to) {
            return Err(format!("There is already a project named {to}: choose another name."));
        }
        let stored = self.projects.remove(from).ok_or_else(|| format!("There is no project named {from} in this browser."))?;
        self.changes.push(Change::Deleted(from.to_owned()));
        self.changes.push(Change::Saved(to.to_owned(), Box::new(Stored { history: Versions::default(), ..stored.clone() })));
        // The history moves with it.
        if stored.history.head().is_some() {
            let blobs = stored.history.blobs().iter().map(|(id, bytes)| (id.clone(), bytes.clone())).collect();
            self.changes.push(Change::Versions { project: to.to_owned(), versions: stored.history.versions().to_vec(), blobs });
        }
        self.projects.insert(to.to_owned(), stored);
        Ok(())
    }

    /// Removes project `name`, with its history.
    pub fn delete(&mut self, name: &str) -> Result<()> {
        self.projects.remove(name).ok_or_else(|| format!("There is no project named {name} in this browser."))?;
        self.changes.push(Change::Deleted(name.to_owned()));
        Ok(())
    }

    /// The changes since the last call, oldest first, for the shell to store.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }
}
