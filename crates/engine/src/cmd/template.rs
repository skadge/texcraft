//! Templates (docs/ROADMAP.md): listing the catalogue, downloading a template, and
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
    let mut forge = super::git::repository_properties();
    if let Some(props) = forge.as_object_mut() {
        props.remove("private");
    }
    json!({"type": "object", "properties": {
        "template": {"type": "string", "description": "A template id from template.list."},
        "folder": {"type": "string", "description": "A new or empty folder for the project (in the browser: its name in the browser's library)."},
        "forge": {"type": "object", "properties": forge, "required": ["service", "name"], "additionalProperties": false,
            "description": "Keep the project in a new repository on GitLab or GitHub, made first: on the desktop the folder is then its clone (the template written, committed, origin set and pushed); in the browser the template is its first commit and the project is linked to it. Uses a connection made already (a token kept or given, or a sign-in): agents never sign in. A push that fails leaves the project and the repository, and says why (push_error); Sync sends it later."},
        "background": {"type": "boolean", "description": "With forge: return at once; the outcome (operation new_project) arrives later, in the window's status line. Default: wait for it (not in the browser)."}
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
                "id": t.id, "title": t.title, "description": t.description, "example": t.is_example(), "publisher": t.publisher, "category": t.category,
                "licence": t.licence, "source": t.source_text(), "main": t.main, "kept": kept(s, t),
            })
        })
        .collect();
    Ok(Value::Array(templates))
}

fn kept(s: &Session, t: &texcraft_templates::Template) -> bool {
    s.template_files.contains_key(&t.id) || t.is_builtin() || s.templates.as_ref().is_some_and(|store| store.kept(t).is_some())
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
    let (id, folder) = (string(p, "template")?, string(p, "folder")?);
    match p.get("forge").filter(|f| !f.is_null()) {
        Some(forge) => on_forge(s, p, forge),
        None if s.library.is_some() => from_template_in_library(s, id, folder),
        None => from_template_in_folder(s, id, folder),
    }
}

/// A new project kept in a new repository: everything checked, then the repository made first;
/// the project is made in it once it is ([`made_on_forge`]).
fn on_forge(s: &mut Session, p: &Value, forge: &Value) -> Result<Value> {
    let catalog = catalog()?;
    let template = catalog.find(string(p, "template")?).map_err(|e| e.to_string())?;
    super::git::check_repository(s, forge)?;
    let folder = string(p, "folder")?;
    let (folder, files) = if s.library.is_some() {
        let name = free_name(s, folder)?;
        let files = library_files(s, template)?;
        (name, files)
    } else {
        (checked_folder(s, folder, template)?, Vec::new())
    };
    let background = super::forge::background(p);
    let mut params = forge.clone();
    params["background"] = json!(background);
    let made = super::git::FromTemplate {
        template: template.id.clone(),
        folder,
        files,
        message: format!("New project from the template {}", template.title),
        #[cfg(not(target_arch = "wasm32"))]
        background,
    };
    super::git::start_create(s, &params, Some(made)).map(|done| done.unwrap_or_else(|| json!({"operation": "new_project", "started": true})))
}

/// The repository for a new project was made: the project made in it, and opened. The browser:
/// the template was its first commit, and the project of the library is linked to it. The
/// desktop: the template written into the folder, committed, and pushed to it (`Ok(None)` while
/// the push runs in the background). Nothing made on the forge is removed when what follows fails:
/// the error says what is there.
pub(super) fn made_on_forge(s: &mut Session, made: &super::git::Made, t: super::git::FromTemplate) -> Result<Option<Value>> {
    let mut result = super::git::made_json(made);
    result["operation"] = json!("new_project");
    let project = result["project"].as_str().unwrap_or_default().to_owned();
    let forge = result["service"].as_str().and_then(texcraft_forge::Service::parse).unwrap_or_default().name();
    if s.library.is_some() {
        let opened = from_template_in_library(s, &t.template, &t.folder).map_err(|e| {
            format!("{project} was made on {forge} with the template as its first commit, but the project could not be made in this browser: {e}")
        })?;
        if let Some(link) = super::git::made_link(made) {
            super::git::link_project(s, link)?;
        }
        result["folder"] = opened["folder"].clone();
        result["main"] = opened["main"].clone();
        result["linked"] = json!(true);
        result["pushed"] = json!(true);
        return Ok(Some(result));
    }
    #[cfg(target_arch = "wasm32")]
    return Err("Projects in the browser are not stored in folders.".to_owned());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let opened = from_template_in_folder(s, &t.template, &t.folder).map_err(|e| {
            format!("{project} was made on {forge}, but the template could not be written into {}: {e} The repository is empty.", t.folder)
        })?;
        result["folder"] = opened["folder"].clone();
        result["main"] = opened["main"].clone();
        super::git::push_new_project(s, made, &t.message, t.background, result)
    }
}

/// `name`, checked as a project of the library's, and free there.
fn free_name(s: &Session, name: &str) -> Result<String> {
    let name = crate::library::check_name(name)?;
    if s.library.as_ref().is_some_and(|l| l.contains(&name)) {
        return Err(format!("There is already a project named {name}: choose another name."));
    }
    Ok(name)
}

/// The browser: the template's files, which the page fetched (a built-in template's are in the
/// program).
fn library_files(s: &Session, template: &texcraft_templates::Template) -> Result<Vec<(String, Vec<u8>)>> {
    match s.template_files.get(&template.id) {
        Some(files) => Ok(files.clone()),
        None if template.is_builtin() => Ok(template.builtin_files()),
        None => Err("This template has not been fetched yet.".to_owned()),
    }
}

#[cfg(target_arch = "wasm32")]
fn checked_folder(_: &Session, _: &str, _: &texcraft_templates::Template) -> Result<String> {
    Err("Projects in the browser are not stored in folders.".to_owned())
}

/// The desktop: `folder` checked (an agent's roots; new or empty), and the template kept
/// (downloaded now when it is not and this session can). Its path, as the session reaches it.
#[cfg(not(target_arch = "wasm32"))]
fn checked_folder(s: &Session, folder: &str, template: &texcraft_templates::Template) -> Result<String> {
    let folder = target_folder(s, folder)?;
    kept_dir(s, template)?;
    Ok(folder.display().to_string())
}

/// The new project's folder: inside an agent's roots, and new or empty.
#[cfg(not(target_arch = "wasm32"))]
fn target_folder(s: &Session, folder: &str) -> Result<PathBuf> {
    let folder = PathBuf::from(folder);
    let folder = match &s.roots {
        Some(roots) => roots.check(&folder, Access::Write)?,
        None => folder,
    };
    if std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("{} is not empty: choose a new or empty folder.", folder.display()));
    }
    Ok(folder)
}

/// Where the template's files are kept, downloaded first when they are not and this session can.
#[cfg(not(target_arch = "wasm32"))]
fn kept_dir(s: &Session, template: &texcraft_templates::Template) -> Result<PathBuf> {
    let store = s.templates.as_ref().ok_or("no template folder")?;
    match store.kept(template) {
        Some(dir) => Ok(dir),
        None if s.roots.is_some() && !template.is_builtin() => {
            Err("This template has not been downloaded, and agent sessions cannot download.".to_owned())
        }
        None => store.fetch(template).map_err(|e| e.to_string()),
    }
}

/// Makes a project of the library named `folder` from the template's files the page fetched,
/// and opens it.
fn from_template_in_library(s: &mut Session, id: &str, folder: &str) -> Result<Value> {
    let catalog = catalog()?;
    let template = catalog.find(id).map_err(|e| e.to_string())?;
    let name = free_name(s, folder)?;
    let files = library_files(s, template)?;
    let mut project = texcraft_project::Project::new();
    for (path, bytes) in &files {
        let path = texcraft_project::ProjectPath::new(path).map_err(|e| e.to_string())?;
        project.add(path, texcraft_project::Contents::from_bytes(bytes.clone())).map_err(|e| e.to_string())?;
    }
    let main = texcraft_project::ProjectPath::new(&template.main).map_err(|e| e.to_string())?;
    project.set_main(&main).map_err(|e| e.to_string())?;
    let now = s.now().timestamp_millis();
    if let Some(library) = &mut s.library {
        library.save(&name, &project, Default::default(), now, None);
    }
    s.replace_project(project, Some(std::path::PathBuf::from(&name)));
    Ok(json!({"folder": name, "main": template.main}))
}

#[cfg(target_arch = "wasm32")]
fn from_template_in_folder(_: &mut Session, _: &str, _: &str) -> Result<Value> {
    Err("Projects in the browser are not stored in folders.".to_owned())
}

/// Copies the template into a new or empty folder, with its licence note, and opens it.
#[cfg(not(target_arch = "wasm32"))]
fn from_template_in_folder(s: &mut Session, id: &str, folder: &str) -> Result<Value> {
    let catalog = catalog()?;
    let template = catalog.find(id).map_err(|e| e.to_string())?;
    let folder = target_folder(s, folder)?;
    let kept = kept_dir(s, template)?;
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
