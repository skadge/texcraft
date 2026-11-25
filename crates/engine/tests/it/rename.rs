//! Renaming the open project (`project.rename`): in the browser's library, and its folder on disk.

use std::path::PathBuf;

use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::file_access::AutomationRoots;
use texcraft_engine::library::{Change, Library};

type R = Result<(), Box<dyn std::error::Error>>;

fn session() -> Session {
    Session::with_backend(Box::new(Unavailable("tests".into())))
}

fn in_library() -> Session {
    let mut s = session();
    s.use_library(Library::default());
    s
}

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-rename-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn a_library_project_renamed_is_kept_under_its_new_name_only() -> R {
    let mut s = in_library();
    s.execute("project.save_as", &json!({"path": "Draft"}))?;
    s.take_library_changes();
    s.execute("file.write", &json!({"path": "main.tex", "text": "edited"}))?;
    s.execute("project.rename", &json!({"name": " Thesis "}))?;
    assert_eq!(s.dir().and_then(|d| d.to_str()), Some("Thesis"));
    assert_eq!(s.library().map(Library::names), Some(vec!["Thesis".to_owned()]));
    assert!(s.is_dirty(), "renaming does not save the edits");
    let changes = s.take_library_changes();
    assert!(
        matches!(&changes[..], [Change::Deleted(old), Change::Saved(new, _), Change::Versions { project, .. }] if old == "Draft" && new == "Thesis" && project == "Thesis"),
        "the shell moves the stored project and its history: {changes:?}"
    );
    Ok(())
}

#[test]
fn a_library_project_is_not_renamed_over_another() -> R {
    let mut s = in_library();
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    s.execute("project.new", &json!({}))?;
    s.execute("project.save_as", &json!({"path": "Notes"}))?;
    s.take_library_changes();
    assert!(s.execute("project.rename", &json!({"name": "Paper"})).is_err());
    assert!(s.execute("project.rename", &json!({"name": "a/b"})).is_err());
    assert_eq!(s.dir().and_then(|d| d.to_str()), Some("Notes"));
    assert!(s.take_library_changes().is_empty(), "nothing to store after a refusal");
    Ok(())
}

#[test]
fn an_untitled_project_has_nothing_to_rename() {
    let mut s = in_library();
    assert!(s.execute("project.rename", &json!({"name": "Paper"})).is_err(), "it is saved under a name first");
}

#[test]
fn a_folder_project_renamed_moves_its_folder_and_saves_there() -> R {
    let base = temp("folder")?;
    let old = base.join("draft");
    let mut s = session();
    s.execute("project.save_as", &json!({"path": old.display().to_string()}))?;
    s.execute("project.rename", &json!({"name": "thesis"}))?;
    let new = base.join("thesis");
    assert!(new.join("main.tex").is_file() && !old.exists());
    assert_eq!(s.dir(), Some(&new));
    s.execute("file.write", &json!({"path": "main.tex", "text": "after"}))?;
    s.execute("project.save", &json!({}))?;
    assert_eq!(std::fs::read_to_string(new.join("main.tex"))?, "after");
    assert!(!old.exists(), "saving goes to the renamed folder");
    Ok(())
}

#[test]
fn a_folder_project_is_not_renamed_over_another_folder() -> R {
    let base = temp("taken")?;
    let old = base.join("draft");
    std::fs::create_dir_all(base.join("taken"))?;
    std::fs::write(base.join("taken/keep.txt"), "mine")?;
    let mut s = session();
    s.execute("project.save_as", &json!({"path": old.display().to_string()}))?;
    assert!(s.execute("project.rename", &json!({"name": "taken"})).is_err());
    assert!(s.execute("project.rename", &json!({"name": ".."})).is_err());
    assert_eq!(std::fs::read_to_string(base.join("taken/keep.txt"))?, "mine");
    assert!(old.join("main.tex").is_file() && s.dir() == Some(&old));
    Ok(())
}

#[test]
fn an_agent_cannot_rename_a_project_folder_it_may_only_read() -> R {
    let base = temp("roots")?;
    let (read, write) = (base.join("read"), base.join("write"));
    std::fs::create_dir_all(read.join("draft"))?;
    std::fs::create_dir_all(&write)?;
    std::fs::write(read.join("draft/main.tex"), "x")?;
    let mut s = session();
    s.confine(AutomationRoots::new(std::slice::from_ref(&read), std::slice::from_ref(&write))?);
    s.execute("project.open", &json!({"path": read.join("draft").display().to_string()}))?;
    assert!(s.execute("project.rename", &json!({"name": "thesis"})).is_err());
    assert!(read.join("draft/main.tex").is_file() && !read.join("thesis").exists());
    Ok(())
}
