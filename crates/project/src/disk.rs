//! Reading a project from a folder and writing it back. Symbolic links are not followed, hidden
//! entries (`.git`, `.retex`) are skipped and the limits in the crate root apply while reading,
//! so a hostile folder cannot make loading unbounded.

use std::path::Path;

use crate::{Contents, Error, MAX_FILE_BYTES, MAX_FILES, Project, ProjectPath, Result};

/// Deepest folder nesting read.
const MAX_DEPTH: usize = 32;

impl Project {
    /// Every regular file under `dir`. The main document is `main.tex` when there is one, else the
    /// first `.tex` file with `\documentclass`, else the first `.tex` file.
    pub fn load_dir(dir: &Path) -> Result<Self> {
        let mut project = Project::new();
        walk(dir, "", 0, &mut project)?;
        project.choose_main();
        Ok(project)
    }

    /// Writes every file under `dir`, creating folders. Files in `dir` that are not in the project
    /// are left alone.
    pub fn save_dir(&self, dir: &Path) -> Result<()> {
        for (path, contents) in &self.files {
            let target = dir.join(path.as_str());
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::Io(format!("creating {}: {e}", parent.display())))?;
            }
            std::fs::write(&target, contents.as_bytes()).map_err(|e| Error::Io(format!("writing {}: {e}", target.display())))?;
        }
        Ok(())
    }
}

fn walk(dir: &Path, prefix: &str, depth: usize, project: &mut Project) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::Io(format!("{} is nested more than {MAX_DEPTH} folders deep", dir.display())));
    }
    let entries = std::fs::read_dir(dir).map_err(|e| Error::Io(format!("reading {}: {e}", dir.display())))?;
    let mut names: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    names.sort_by_key(|e| e.file_name());
    for entry in names {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let rel = if prefix.is_empty() { name.to_owned() } else { format!("{prefix}/{name}") };
        if kind.is_dir() {
            walk(&entry.path(), &rel, depth + 1, project)?;
        } else if kind.is_file() {
            if project.files.len() >= MAX_FILES {
                return Err(Error::TooManyFiles);
            }
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if len > MAX_FILE_BYTES as u64 {
                return Err(Error::TooLarge(rel, usize::try_from(len).unwrap_or(usize::MAX)));
            }
            let Ok(path) = ProjectPath::new(&rel) else { continue };
            let bytes = std::fs::read(entry.path()).map_err(|e| Error::Io(format!("reading {rel}: {e}")))?;
            project.add(path, Contents::from_bytes(bytes))?;
        }
    }
    Ok(())
}
