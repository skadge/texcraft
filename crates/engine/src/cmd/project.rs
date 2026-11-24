//! Opening, saving and describing the project.

use serde_json::{Value, json};
use texcraft_project::Project;

use std::path::{Path, PathBuf};

use super::{CommandSpec, always, no_params, string};
use crate::file_access::Access;
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "project.new",
            label: "New Project",
            menu: &["File"],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: new,
            changes: true,
        },
        CommandSpec {
            id: "project.open",
            label: "Open Project Folder…",
            menu: &["File"],
            shortcut: Some("Cmd+O"),
            params: path_schema,
            enabled: stored,
            run: open,
            changes: true,
        },
        CommandSpec {
            id: "project.save",
            label: "Save",
            menu: &["File"],
            shortcut: Some("Cmd+S"),
            params: no_params,
            enabled: has_dir,
            run: save,
            changes: true,
        },
        CommandSpec {
            id: "project.save_as",
            label: "Save Project As…",
            menu: &["File"],
            shortcut: Some("Cmd+Shift+S"),
            params: path_schema,
            enabled: stored,
            run: save_as,
            changes: true,
        },
        CommandSpec {
            id: "project.delete",
            label: "Delete Project",
            menu: &[],
            shortcut: None,
            params: name_schema,
            enabled: has_library,
            run: delete,
            changes: true,
        },
        CommandSpec {
            id: "project.inspect",
            label: "Describe Project",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: always,
            run: inspect,
            changes: false,
        },
    ]
}

fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string", "description": "A folder on disk, or in the browser a project's name."}}, "required": ["path"], "additionalProperties": false})
}

fn name_schema() -> Value {
    json!({"type": "object", "properties": {"name": {"type": "string", "description": "A project's name in the browser."}}, "required": ["name"], "additionalProperties": false})
}

/// Projects are opened and saved: from folders, or from the browser's library.
fn stored(s: &Session) -> Result<()> {
    if s.library.is_some() || cfg!(not(target_arch = "wasm32")) {
        Ok(())
    } else {
        Err("This build of TeXCraft has nowhere to keep projects.".to_owned())
    }
}

fn has_dir(s: &Session) -> Result<()> {
    stored(s)?;
    if s.dir.is_some() { Ok(()) } else { Err("The project has no folder yet: use Save Project As.".to_owned()) }
}

fn has_library(s: &Session) -> Result<()> {
    if s.library.is_some() { Ok(()) } else { Err("Projects are kept by name only in the browser.".to_owned()) }
}

fn new(s: &mut Session, _: &Value) -> Result<Value> {
    s.replace_project(Project::blank(), None);
    Ok(json!({}))
}

fn open(s: &mut Session, params: &Value) -> Result<Value> {
    let path = string(params, "path")?;
    let (project, dir) = match &s.library {
        Some(library) => (library.load(path)?, PathBuf::from(path)),
        None => open_dir(s, path)?,
    };
    let files = project.len();
    s.replace_project(project, Some(dir));
    Ok(json!({"files": files, "main": s.project.main().map(|p| p.to_string())}))
}

fn save(s: &mut Session, _: &Value) -> Result<Value> {
    let dir = s.dir.clone().ok_or("The project has no folder yet.")?;
    save_to(s, &dir)
}

fn save_as(s: &mut Session, params: &Value) -> Result<Value> {
    let path = string(params, "path")?;
    let dir = match &s.library {
        Some(library) => {
            let name = crate::library::check_name(path)?;
            if library.contains(&name) && s.dir.as_ref().and_then(|d| d.to_str()) != Some(name.as_str()) {
                return Err(format!("There is already a project named {name}: choose another name."));
            }
            PathBuf::from(name)
        }
        None => folder(s, path, Access::Write)?,
    };
    save_to(s, &dir)
}

/// Writes the project to `dir` (a folder, or its name in the library) and makes it the
/// project's folder.
fn save_to(s: &mut Session, dir: &Path) -> Result<Value> {
    let now = s.now().timestamp_millis();
    match &mut s.library {
        Some(library) => {
            let review = s.review.files(&s.project).map_err(|e| format!("The review could not be saved: {e}"))?;
            library.save(&dir.display().to_string(), &s.project, review, now);
        }
        None => save_dir(s, dir)?,
    }
    s.dir = Some(dir.to_path_buf());
    s.dirty = false;
    Ok(json!({"path": dir.display().to_string()}))
}

fn delete(s: &mut Session, params: &Value) -> Result<Value> {
    let name = string(params, "name")?;
    s.library.as_mut().ok_or("no library")?.delete(name)?;
    if s.dir.as_ref().and_then(|d| d.to_str()) == Some(name) {
        // The open project stays open, with nowhere to save it but a new name.
        s.dir = None;
        s.dirty = true;
    }
    Ok(json!({"deleted": name}))
}

#[cfg(not(target_arch = "wasm32"))]
fn folder(s: &Session, path: &str, access: Access) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    match &s.roots {
        Some(roots) => roots.check(&path, access),
        None => Ok(path),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_dir(s: &Session, path: &str) -> Result<(Project, PathBuf)> {
    let dir = folder(s, path, Access::Read)?;
    let project = Project::load_dir(&dir).map_err(|e| e.to_string())?;
    Ok((project, dir))
}

#[cfg(not(target_arch = "wasm32"))]
fn save_dir(s: &mut Session, dir: &Path) -> Result<()> {
    if let Some(roots) = &s.roots {
        roots.check(dir, Access::Write)?;
    }
    s.project.save_dir(dir).map_err(|e| e.to_string())?;
    let new_folder = s.dir.as_deref() != Some(dir);
    s.save_review(dir, new_folder)
}

#[cfg(target_arch = "wasm32")]
fn folder(_: &Session, _: &str, _: Access) -> Result<PathBuf> {
    Err(NO_FOLDERS.to_owned())
}

#[cfg(target_arch = "wasm32")]
fn open_dir(_: &Session, _: &str) -> Result<(Project, PathBuf)> {
    Err(NO_FOLDERS.to_owned())
}

#[cfg(target_arch = "wasm32")]
fn save_dir(_: &mut Session, _: &Path) -> Result<()> {
    Err(NO_FOLDERS.to_owned())
}

#[cfg(target_arch = "wasm32")]
const NO_FOLDERS: &str = "Projects in the browser are not stored in folders.";

fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    let files: Vec<Value> = s
        .project
        .files()
        .map(|(p, c)| json!({"path": p.as_str(), "bytes": c.len(), "text": matches!(c, texcraft_project::Contents::Text(_))}))
        .collect();
    Ok(json!({
        "folder": s.dir.as_ref().map(|d| d.display().to_string()),
        "main": s.project.main().map(|p| p.to_string()),
        "files": files,
        "open": s.open.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        "current": s.current.as_ref().map(|p| p.as_str()),
        "unsaved": s.dirty,
        "revision": s.revision,
    }))
}
