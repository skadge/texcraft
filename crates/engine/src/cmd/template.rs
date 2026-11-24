//! Templates (docs/plan.md, phase 1, item 2b): listing the catalogue, downloading a template, and
//! starting a project from one.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use serde_json::{Value, json};
use texcraft_templates::Catalog;

use super::{CommandSpec, no_params, string};
#[cfg(not(target_arch = "wasm32"))]
use crate::file_access::Access;
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "template.list",
            label: "Templates",
            menu: &[],
            shortcut: None,
            params: no_params,
            enabled: has_store,
            run: list,
            changes: false,
        },
        CommandSpec {
            id: "template.fetch",
            label: "Download Template",
            menu: &[],
            shortcut: None,
            params: template_schema,
            enabled: can_download,
            run: fetch,
            changes: false,
        },
        CommandSpec {
            id: "project.from_template",
            label: "New Project from Template…",
            menu: &["File"],
            shortcut: None,
            params: from_schema,
            enabled: has_store,
            run: from_template,
            changes: true,
        },
    ]
}

fn template_schema() -> Value {
    json!({"type": "object", "properties": {"template": {"type": "string", "description": "A template id from template.list."}}, "required": ["template"], "additionalProperties": false})
}

fn from_schema() -> Value {
    json!({"type": "object", "properties": {
        "template": {"type": "string", "description": "A template id from template.list."},
        "folder": {"type": "string", "description": "A new or empty folder for the project."}
    }, "required": ["template", "folder"], "additionalProperties": false})
}

/// Templates are kept: in a folder, or in the browser's memory (`Session::keep_template`).
fn has_store(s: &Session) -> Result<()> {
    if s.templates.is_some() || s.library.is_some() { Ok(()) } else { Err("This build of TeXCraft has no folder to keep templates in.".to_owned()) }
}

fn can_download(s: &Session) -> Result<()> {
    has_store(s)?;
    if s.templates.is_none() {
        return Err("In the browser, the page fetches templates itself.".to_owned());
    }
    if s.roots.is_some() {
        // RFC-0015: no URL fetches while confined.
        return Err("Templates cannot be downloaded in an agent session; kept templates can be used.".to_owned());
    }
    Ok(())
}

fn catalog() -> Result<Catalog> {
    Catalog::builtin().map_err(|e| e.to_string())
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let templates: Vec<Value> = catalog()?
        .templates()
        .iter()
        .map(|t| {
            json!({
                "id": t.id, "title": t.title, "publisher": t.publisher, "category": t.category,
                "licence": t.licence, "source": t.source_text(), "main": t.main, "kept": kept(s, t),
            })
        })
        .collect();
    Ok(Value::Array(templates))
}

fn kept(s: &Session, t: &texcraft_templates::Template) -> bool {
    s.template_files.contains_key(&t.id) || s.templates.as_ref().is_some_and(|store| store.kept(t).is_some())
}

fn fetch(s: &mut Session, p: &Value) -> Result<Value> {
    can_download(s)?;
    let catalog = catalog()?;
    let template = catalog.find(string(p, "template")?).map_err(|e| e.to_string())?;
    let store = s.templates.as_ref().ok_or("no template folder")?;
    let dir = store.fetch(template).map_err(|e| e.to_string())?;
    Ok(json!({"template": template.id, "kept": dir.display().to_string()}))
}

fn from_template(s: &mut Session, p: &Value) -> Result<Value> {
    if s.library.is_some() { from_template_in_library(s, p) } else { from_template_in_folder(s, p) }
}

/// Makes a project of the library named `folder` from the template's files the page fetched,
/// and opens it.
fn from_template_in_library(s: &mut Session, p: &Value) -> Result<Value> {
    let catalog = catalog()?;
    let template = catalog.find(string(p, "template")?).map_err(|e| e.to_string())?;
    let name = crate::library::check_name(string(p, "folder")?)?;
    if s.library.as_ref().is_some_and(|l| l.contains(&name)) {
        return Err(format!("There is already a project named {name}: choose another name."));
    }
    let files = s.template_files.get(&template.id).ok_or("This template has not been fetched yet.")?;
    let mut project = texcraft_project::Project::new();
    for (path, bytes) in files {
        let path = texcraft_project::ProjectPath::new(path).map_err(|e| e.to_string())?;
        project.add(path, texcraft_project::Contents::from_bytes(bytes.clone())).map_err(|e| e.to_string())?;
    }
    let main = texcraft_project::ProjectPath::new(&template.main).map_err(|e| e.to_string())?;
    project.set_main(&main).map_err(|e| e.to_string())?;
    let now = s.now().timestamp_millis();
    if let Some(library) = &mut s.library {
        library.save(&name, &project, Default::default(), now);
    }
    s.replace_project(project, Some(std::path::PathBuf::from(&name)));
    Ok(json!({"folder": name, "main": template.main}))
}

#[cfg(target_arch = "wasm32")]
fn from_template_in_folder(_: &mut Session, _: &Value) -> Result<Value> {
    Err("Projects in the browser are not stored in folders.".to_owned())
}

/// Copies the template into a new or empty folder, with its licence note, and opens it.
#[cfg(not(target_arch = "wasm32"))]
fn from_template_in_folder(s: &mut Session, p: &Value) -> Result<Value> {
    let catalog = catalog()?;
    let template = catalog.find(string(p, "template")?).map_err(|e| e.to_string())?;
    let folder = PathBuf::from(string(p, "folder")?);
    let folder = match &s.roots {
        Some(roots) => roots.check(&folder, Access::Write)?,
        None => folder,
    };
    if std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("{} is not empty: choose a new or empty folder.", folder.display()));
    }
    let store = s.templates.as_ref().ok_or("no template folder")?;
    let kept = match store.kept(template) {
        Some(dir) => dir,
        None if s.roots.is_some() => return Err("This template has not been downloaded, and agent sessions cannot download.".to_owned()),
        None => store.fetch(template).map_err(|e| e.to_string())?,
    };
    let mut paths: Vec<String> = template.project_paths().into_iter().map(|(_, p)| p).collect();
    paths.push("TEMPLATE-LICENSE".to_owned());
    for rel in &paths {
        let (from, to) = (kept.join(rel), folder.join(rel));
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    let mut project = texcraft_project::Project::load_dir(&folder).map_err(|e| e.to_string())?;
    if let Ok(main) = texcraft_project::ProjectPath::new(&template.main) {
        let _ = project.set_main(&main);
    }
    s.replace_project(project, Some(folder.clone()));
    Ok(json!({"folder": folder.display().to_string(), "main": template.main}))
}
