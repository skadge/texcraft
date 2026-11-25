//! Issue #2: a file deleted, renamed or moved in the project leaves its folder at the next save,
//! and only those files do.

use std::path::PathBuf;

use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;

type R = Result<(), Box<dyn std::error::Error>>;

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-deleted-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn session() -> Session {
    Session::with_backend(Box::new(Unavailable("tests".into())))
}

fn paths(s: &Session) -> Vec<String> {
    s.project().paths().map(|p| p.as_str().to_owned()).collect()
}

#[test]
fn a_deleted_file_is_gone_from_the_folder_after_a_save_and_other_files_stay() -> R {
    let dir = temp("delete")?;
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "main"}))?;
    s.execute("file.new", &json!({"path": "old.tex", "text": "old"}))?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    // Put there by someone else after the project was saved: not the project's.
    std::fs::write(dir.join("notes.txt"), "mine")?;

    s.execute("file.delete", &json!({"path": "old.tex"}))?;
    let saved = s.execute("project.save", &json!({}))?;
    assert_eq!(saved["removed"]["deleted"], json!(["old.tex"]), "{saved}");
    assert!(!dir.join("old.tex").exists());
    assert!(dir.join("notes.txt").exists(), "a file the project did not load stays");
    assert!(dir.join(".git").is_dir() && dir.join(".gitignore").exists(), "hidden entries stay");

    let mut t = session();
    t.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    assert_eq!(paths(&t), ["main.tex", "notes.txt"], "the deleted file does not come back");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_moved_file_leaves_its_old_place_and_the_folder_it_empties() -> R {
    let dir = temp("move")?;
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "main"}))?;
    s.execute("file.new", &json!({"path": "chapters/intro.tex", "text": "intro"}))?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;

    s.execute("file.rename", &json!({"from": "chapters/intro.tex", "to": "intro.tex"}))?;
    s.execute("project.save", &json!({}))?;
    assert!(!dir.join("chapters").exists(), "the emptied folder is gone");

    let mut t = session();
    t.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    assert_eq!(paths(&t), ["intro.tex", "main.tex"]);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn saving_to_another_folder_removes_nothing_there() -> R {
    let (one, two) = (temp("first")?, temp("second")?);
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "main"}))?;
    s.execute("file.new", &json!({"path": "old.tex", "text": "old"}))?;
    s.execute("project.save_as", &json!({"path": one.display().to_string()}))?;
    std::fs::write(two.join("old.tex"), "theirs")?;
    s.execute("file.delete", &json!({"path": "old.tex"}))?;
    s.execute("project.save_as", &json!({"path": two.display().to_string()}))?;
    assert!(two.join("old.tex").exists(), "the other folder's file is not the project's");
    assert!(one.join("old.tex").exists(), "the folder left behind is not touched");
    std::fs::remove_dir_all(&one)?;
    std::fs::remove_dir_all(&two)?;
    Ok(())
}
