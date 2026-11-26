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
            id: "project.import",
            label: "New Project from Files",
            menu: &[],
            shortcut: None,
            params: import_schema,
            enabled: always,
            run: import,
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
            id: "project.write",
            label: "Write Changes",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: has_dir,
            run: write,
            changes: true,
        },
        CommandSpec {
            id: "project.autosave",
            label: "Save Automatically",
            menu: &[],
            shortcut: None,
            params: on_schema,
            enabled: always,
            run: autosave,
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
            id: "project.rename",
            label: "Rename Project",
            menu: &[],
            shortcut: None,
            params: name_schema,
            enabled: has_dir,
            run: rename,
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
            id: "project.download_zip",
            label: "Download Project as Zip",
            menu: &["File"],
            shortcut: None,
            params: zip_schema,
            enabled: always,
            run: download_zip,
            changes: false,
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

fn on_schema() -> Value {
    json!({"type": "object", "properties": {"on": {"type": "boolean", "description": "Whether the window writes changes shortly after typing stops, and makes versions by itself (after a pause in typing, when the project closes)."}}, "required": ["on"], "additionalProperties": false})
}

fn name_schema() -> Value {
    json!({"type": "object", "properties": {"name": {"type": "string", "description": "A project's name: in the browser, the name it is kept by; on disk, its folder's name."}}, "required": ["name"], "additionalProperties": false})
}

/// Projects are opened and saved: from folders, or from the browser's library.
fn stored(s: &Session) -> Result<()> {
    if s.library.is_some() || cfg!(not(target_arch = "wasm32")) {
        Ok(())
    } else {
        Err("This build of TeXCraft has nowhere to keep projects.".to_owned())
    }
}

fn zip_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string", "description": "Where to write the zip file. In the browser, the name of the file to download (by default the project's name, with `.zip`)."}}, "additionalProperties": false})
}

/// The project's files as a zip, its review's threads in `.texcraft/review/` as a project folder
/// keeps them: a copy to keep elsewhere, which opens again as the same project (Import a Zip File,
/// or unzipped as a folder).
fn zip_project(s: &mut Session) -> Result<Vec<u8>> {
    let zipped = |e: craft_zip::Error| format!("The project could not be zipped: {e}");
    let mut zip = craft_zip::Writer::new();
    for (path, contents) in s.project.files() {
        zip.add(path.as_str(), contents.as_bytes(), craft_zip::Compression::Deflated).map_err(zipped)?;
    }
    let threads = s.review.files(&s.project).map_err(|e| format!("The review could not be zipped: {e}"))?;
    for (file, text) in &threads {
        zip.add(&format!(".texcraft/review/{file}"), text.as_bytes(), craft_zip::Compression::Deflated).map_err(zipped)?;
    }
    zip.finish().map_err(zipped)
}

/// Writes the project as a zip file (`zip_project`) to `path`; in the browser, hands it to the
/// shell to download.
fn download_zip(s: &mut Session, p: &Value) -> Result<Value> {
    let data = zip_project(s)?;
    let files = s.project.len();
    let name = match s.dir.as_ref().and_then(|d| d.file_name()) {
        Some(name) => format!("{}.zip", name.to_string_lossy()),
        None => "project.zip".to_owned(),
    };
    let path = p.get("path").and_then(Value::as_str).map(str::to_owned);
    write_zip(s, name, path, data, files)
}

#[cfg(target_arch = "wasm32")]
fn write_zip(s: &mut Session, name: String, path: Option<String>, data: Vec<u8>, files: usize) -> Result<Value> {
    let name = path.as_deref().and_then(|p| Path::new(p).file_name()).map_or(name, |n| n.to_string_lossy().into_owned());
    let bytes = data.len();
    s.hand_out(name.clone(), data);
    Ok(json!({"path": name, "files": files, "bytes": bytes, "download": true}))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_zip(s: &mut Session, _: String, path: Option<String>, data: Vec<u8>, files: usize) -> Result<Value> {
    let path = PathBuf::from(path.ok_or("the parameter `path` (a string) is missing: where to write the zip file")?);
    let path = match &s.roots {
        Some(roots) => roots.check(&path, Access::Write)?,
        None => path,
    };
    std::fs::write(&path, &data).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(json!({"path": path.display().to_string(), "files": files, "bytes": data.len()}))
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
    super::history::find(s);
    Ok(json!({"files": files, "main": s.project.main().map(|p| p.to_string()), "history": super::history::status_of(s)}))
}

fn import_schema() -> Value {
    json!({"type": "object", "properties": {
        "source": {"type": "string", "description": "A zip file or a folder on disk whose files make the project (a folder is copied, not opened in place: `project.open` does that)."},
        "data": {"type": "string", "description": "Or a zip file's contents, in base64."},
        "files": {"type": "array", "description": "Or files by path and contents (a folder the browser read); the one folder they are all in is left out of their paths.", "items": {"type": "object", "properties": {
            "path": {"type": "string"},
            "data": {"type": "string", "description": "Its contents, in base64."}
        }, "required": ["path", "data"], "additionalProperties": false}}
    }, "additionalProperties": false})
}

/// Makes a new project, not saved yet, from a zip file, a folder's files or files given by
/// contents, with the filter and limits of an import, and opens it. Its main document is chosen as
/// when a folder is opened. Save Project As keeps it.
fn import(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::import::{Gathered, Limits};
    let limits = Limits::default();
    let gathered = if p.get("files").is_some() {
        Gathered::from_files(super::file::files_param(p)?, limits, true)
    } else if p.get("data").is_some() {
        Gathered::from_zip(&super::file::base64_param(p, "data")?, limits)?
    } else {
        let source = super::file::source_param(s, p)?;
        if source.is_dir() {
            Gathered::from_folder(&source, limits, false, s.roots.as_ref())?
        } else {
            Gathered::from_zip(&super::file::read_source(&source, limits.bytes)?, limits)?
        }
    };
    if gathered.files.is_empty() {
        return Err("There is no file to make a project from: everything was left out or the source is empty.".to_owned());
    }
    let mut project = Project::new();
    let mut skipped = gathered.skipped;
    for (path, bytes) in gathered.files {
        let added = texcraft_project::ProjectPath::new(&path).and_then(|at| project.add(at, texcraft_project::Contents::from_bytes(bytes)));
        if added.is_err() {
            skipped.push(crate::import::Skipped { path, reason: crate::import::Skip::Unreadable });
        }
    }
    project.choose_main();
    let (files, main) = (project.len(), project.main().map(|m| m.to_string()));
    s.replace_project(project, None);
    // Nowhere yet: closing it asks to save it.
    s.dirty = true;
    Ok(json!({"files": files, "main": main, "skipped": skipped, "folder": gathered.stripped}))
}

/// Writes the project's changes where it is kept, without making a version: what the window does
/// shortly after typing stops. Nothing is written when nothing changed.
fn write(s: &mut Session, _: &Value) -> Result<Value> {
    let dir = s.dir.clone().ok_or("The project has no folder yet.")?;
    if !s.dirty {
        return Ok(json!({"path": dir.display().to_string(), "written": false}));
    }
    let mut result = write_to(s, &dir)?;
    result["written"] = json!(true);
    Ok(result)
}

fn autosave(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).ok_or("`on` (true or false) is missing")?;
    s.settings.autosave = on;
    Ok(json!({"autosave": on}))
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

/// Writes the project to `dir` (a folder, or its name in the library), makes it the project's
/// folder, and records a version of it (in a repository TeXCraft made, or the library).
pub(super) fn save_to(s: &mut Session, dir: &Path) -> Result<Value> {
    let moved = s.dir.as_deref() != Some(dir);
    let mut result = write_to(s, dir)?;
    // The project is saved either way; the history says what became of the version.
    result["history"] =
        if s.library.is_some() { super::history::after_library_save(s, &dir.display().to_string()) } else { super::history::after_save(s, moved) };
    Ok(result)
}

/// Writes the project to `dir` (a folder, or its name in the library) and makes it the
/// project's folder, without making a version.
pub(super) fn write_to(s: &mut Session, dir: &Path) -> Result<Value> {
    let now = s.now().timestamp_millis();
    let mut result = json!({"path": dir.display().to_string()});
    match &mut s.library {
        Some(library) => {
            let review = s.review.files(&s.project).map_err(|e| format!("The review could not be saved: {e}"))?;
            library.save(&dir.display().to_string(), &s.project, review, now, s.forge.clone());
        }
        None => result["removed"] = save_dir(s, dir)?,
    }
    s.dir = Some(dir.to_path_buf());
    s.dirty = false;
    Ok(result)
}

/// Renames the open project: its entry in the library, or its folder on disk (in the same
/// parent folder). Unsaved changes stay unsaved.
fn rename(s: &mut Session, params: &Value) -> Result<Value> {
    let name = crate::library::check_name(string(params, "name")?)?;
    if name == "." || name == ".." {
        return Err("A project's name cannot be “.” or “..”.".to_owned());
    }
    let old = s.dir.clone().ok_or("The project has no name yet: use Save Project As.")?;
    let new = match &mut s.library {
        Some(library) => {
            let from = old.display().to_string();
            if from != name {
                library.rename(&from, &name)?;
            }
            PathBuf::from(&name)
        }
        None => {
            let new = old.parent().map_or_else(|| PathBuf::from(&name), |p| p.join(&name));
            if new != old {
                rename_dir(s, &old, &new)?;
            }
            new
        }
    };
    s.dir = Some(new.clone());
    if s.library.is_none() {
        // The repository moved with the folder (or was above it).
        super::history::find(s);
    }
    Ok(json!({"path": new.display().to_string(), "previous": old.display().to_string()}))
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

/// Writes the project into `dir`, then removes from the folder the files the project had there
/// and no longer has (issue #2). Says what was removed, and what could not be.
#[cfg(not(target_arch = "wasm32"))]
fn save_dir(s: &mut Session, dir: &Path) -> Result<Value> {
    if let Some(roots) = &s.roots {
        roots.check(dir, Access::Write)?;
    }
    s.project.save_dir(dir).map_err(|e| e.to_string())?;
    let new_folder = s.dir.as_deref() != Some(dir);
    s.save_review(dir, new_folder)?;
    // In another folder, the files there are not the project's: none is removed.
    let gone: Vec<texcraft_project::ProjectPath> =
        if new_folder { Vec::new() } else { s.on_disk.iter().filter(|p| s.project.get(p).is_none()).cloned().collect() };
    let (mut deleted, mut trashed, mut left) = (Vec::new(), Vec::new(), Vec::new());
    let mut still = Vec::new();
    for path in gone {
        match remove_dropped(s, dir, &path) {
            Ok(Removed::Deleted) => deleted.push(path.as_str().to_owned()),
            Ok(Removed::Trashed) => trashed.push(path.as_str().to_owned()),
            Ok(Removed::Absent) => {}
            Err(e) => {
                log::warn!("{e}");
                left.push(json!({"path": path.as_str(), "error": e}));
                still.push(path);
            }
        }
    }
    // What could not be removed is tried again at the next save.
    s.on_disk = s.project.paths().cloned().chain(still).collect();
    Ok(json!({"deleted": deleted, "trashed": trashed, "left": left}))
}

/// How a file the project no longer has left its folder.
#[cfg(not(target_arch = "wasm32"))]
enum Removed {
    /// Deleted: the repository's last commit holds it as it was.
    Deleted,
    /// Moved to the system's trash, where it can be brought back from.
    Trashed,
    /// It was not there any more (or is not a file now).
    Absent,
}

/// Removes `path`, a file the project had in `dir` and no longer has. A file the repository's
/// last commit holds as it is on disk is deleted (the history keeps it); any other goes to the
/// system's trash, so nothing the user could not get back is lost. Agents have no trash outside
/// their roots: theirs stay.
#[cfg(not(target_arch = "wasm32"))]
fn remove_dropped(s: &Session, dir: &Path, path: &texcraft_project::ProjectPath) -> Result<Removed> {
    let file = dir.join(path.as_str());
    if !std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()) {
        return Ok(Removed::Absent);
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("{path} was removed from the project but could not be read in its folder: {e}"))?;
    let committed = s.history.as_ref().and_then(|h| {
        let head = h.head().ok()??;
        h.file_at(&head, path.as_str()).ok()?
    });
    let removed = if committed.as_deref() == Some(bytes.as_slice()) {
        std::fs::remove_file(&file).map_err(|e| format!("{path} was removed from the project but could not be deleted from its folder: {e}"))?;
        Removed::Deleted
    } else if s.roots.is_some() {
        return Err(format!(
            "{path} was removed from the project but stays in its folder: its last version is not in the project's history, and an agent cannot move it to the trash."
        ));
    } else {
        trash::delete(&file).map_err(|e| format!("{path} was removed from the project but could not be moved to the trash: {e}"))?;
        Removed::Trashed
    };
    // The folders it leaves empty go too, up to the project's folder.
    for parent in file.ancestors().skip(1).take_while(|p| *p != dir && p.starts_with(dir)) {
        if std::fs::remove_dir(parent).is_err() {
            break;
        }
    }
    Ok(removed)
}

/// Renames the project's folder `old` to `new`, a sibling that must not exist yet.
#[cfg(not(target_arch = "wasm32"))]
fn rename_dir(s: &mut Session, old: &Path, new: &Path) -> Result<()> {
    if let Some(roots) = &s.roots {
        roots.check(old, Access::Write)?;
        roots.check(new, Access::Write)?;
    }
    if new.exists() {
        return Err(format!("There is already a file or folder at {}: choose another name.", new.display()));
    }
    std::fs::rename(old, new).map_err(|e| format!("The project's folder {} could not be renamed: {e}", old.display()))?;
    // The language server names files by their place on disk.
    s.language = crate::language::Language::new(Some(new));
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn rename_dir(_: &mut Session, _: &Path, _: &Path) -> Result<()> {
    Err(NO_FOLDERS.to_owned())
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
fn save_dir(_: &mut Session, _: &Path) -> Result<Value> {
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
