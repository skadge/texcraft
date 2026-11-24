//! Projects kept by name, for a shell that has no folders: the browser, where the web app keeps
//! them in IndexedDB. The session keeps the library in memory, so its commands stay synchronous.
//! The shell fills it at start ([`Library::insert`]) and stores each change it takes
//! ([`Library::take_changes`]).
//!
//! A project of the library has its name as the session's folder (`Session::dir`), so the
//! interface's title, recent projects and thumbnails work as they do for folders.

use std::collections::BTreeMap;

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
}

/// A change the shell has to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Saved(String, Stored),
    Deleted(String),
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

    /// Keeps `project` as `name`, replacing the project of that name.
    pub fn save(&mut self, name: &str, project: &Project, saved: i64) {
        let stored = Stored {
            files: project.files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect(),
            main: project.main().map(|m| m.as_str().to_owned()),
            saved,
        };
        self.projects.insert(name.to_owned(), stored.clone());
        self.changes.push(Change::Saved(name.to_owned(), stored));
    }

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
