//! Opening, saving and describing the project.

use serde_json::{Value, json};
use texcraft_project::Project;

#[cfg(not(target_arch = "wasm32"))]
use super::string;
use super::{CommandSpec, always, no_params};
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
            enabled: on_disk,
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
            enabled: on_disk,
            run: save_as,
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
    json!({"type": "object", "properties": {"path": {"type": "string", "description": "A folder on disk."}}, "required": ["path"], "additionalProperties": false})
}

fn on_disk(_: &Session) -> Result<()> {
    if cfg!(target_arch = "wasm32") { Err("Projects in the browser are not stored in folders.".to_owned()) } else { Ok(()) }
}

fn has_dir(s: &Session) -> Result<()> {
    on_disk(s)?;
    if s.dir.is_some() { Ok(()) } else { Err("The project has no folder yet: use Save Project As.".to_owned()) }
}

fn new(s: &mut Session, _: &Value) -> Result<Value> {
    s.replace_project(Project::blank(), None);
    Ok(json!({}))
}

#[cfg(not(target_arch = "wasm32"))]
fn folder(s: &Session, params: &Value, access: crate::file_access::Access) -> Result<std::path::PathBuf> {
    let path = std::path::PathBuf::from(string(params, "path")?);
    match &s.roots {
        Some(roots) => roots.check(&path, access),
        None => Ok(path),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open(s: &mut Session, params: &Value) -> Result<Value> {
    let dir = folder(s, params, crate::file_access::Access::Read)?;
    let project = Project::load_dir(&dir).map_err(|e| e.to_string())?;
    let files = project.len();
    s.replace_project(project, Some(dir));
    Ok(json!({"files": files, "main": s.project.main().map(|p| p.to_string())}))
}

#[cfg(not(target_arch = "wasm32"))]
fn save(s: &mut Session, _: &Value) -> Result<Value> {
    let dir = s.dir.clone().ok_or("The project has no folder yet.")?;
    if let Some(roots) = &s.roots {
        roots.check(&dir, crate::file_access::Access::Write)?;
    }
    s.project.save_dir(&dir).map_err(|e| e.to_string())?;
    s.dirty = false;
    Ok(json!({"path": dir.display().to_string()}))
}

#[cfg(not(target_arch = "wasm32"))]
fn save_as(s: &mut Session, params: &Value) -> Result<Value> {
    let dir = folder(s, params, crate::file_access::Access::Write)?;
    s.project.save_dir(&dir).map_err(|e| e.to_string())?;
    s.dir = Some(dir.clone());
    s.dirty = false;
    Ok(json!({"path": dir.display().to_string()}))
}

#[cfg(target_arch = "wasm32")]
fn open(_: &mut Session, _: &Value) -> Result<Value> {
    Err("Projects in the browser are not stored in folders.".to_owned())
}

#[cfg(target_arch = "wasm32")]
fn save(_: &mut Session, _: &Value) -> Result<Value> {
    Err("Projects in the browser are not stored in folders.".to_owned())
}

#[cfg(target_arch = "wasm32")]
fn save_as(_: &mut Session, _: &Value) -> Result<Value> {
    Err("Projects in the browser are not stored in folders.".to_owned())
}

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
