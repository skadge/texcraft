//! Saving and versions apart (docs/ROADMAP.md, C9): writing the project's changes makes no
//! version; a version is made when asked, by itself only in a repository TeXCraft made, and only
//! when something changed since the latest one.

use std::path::PathBuf;

use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::library::{Change, Library};

type R = Result<(), Box<dyn std::error::Error>>;

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-versions-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn session() -> Session {
    Session::with_backend(Box::new(Unavailable("tests".into())))
}

fn write(s: &mut Session, path: &str, text: &str) -> Result<(), String> {
    s.execute("file.write", &json!({"path": path, "text": text})).map(|_| ())
}

fn versions(s: &mut Session) -> Result<usize, String> {
    Ok(s.execute("history.list", &json!({}))?["commits"].as_array().map_or(0, Vec::len))
}

#[test]
fn writing_changes_makes_no_version_and_a_version_takes_them_all_at_once() -> R {
    let dir = temp("folder")?;
    let mut s = session();
    write(&mut s, "main.tex", "one")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    let first = versions(&mut s)?;
    assert!(!s.unversioned());

    for text in ["two", "three", "four"] {
        write(&mut s, "main.tex", text)?;
        assert!(s.is_dirty() && s.unversioned());
        let wrote = s.execute("project.write", &json!({}))?;
        assert_eq!(wrote["written"], true);
        assert!(!s.is_dirty(), "written");
        assert!(s.unversioned(), "written, and in no version yet");
        assert_eq!(std::fs::read_to_string(dir.join("main.tex"))?, text);
    }
    assert_eq!(versions(&mut s)?, first, "writing makes no version");

    let made = s.execute("history.version", &json!({"automatic": true}))?;
    assert!(made["commit"].is_string(), "{made}");
    assert_eq!(versions(&mut s)?, first + 1, "the three writes are one version");
    assert!(!s.unversioned());
    let again = s.execute("history.version", &json!({"automatic": true}))?;
    assert!(again["commit"].is_null(), "nothing changed since: no version ({again})");
    assert_eq!(versions(&mut s)?, first + 1);

    // A version asked for with a name writes what is not written yet, and takes the name.
    write(&mut s, "main.tex", "five")?;
    s.execute("history.version", &json!({"message": "Submitted draft"}))?;
    assert_eq!(std::fs::read_to_string(dir.join("main.tex"))?, "five");
    let log = s.execute("history.list", &json!({}))?;
    assert_eq!(log["commits"][0]["message"], "Submitted draft");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_repository_someone_else_made_gets_versions_only_when_asked() -> R {
    let top = temp("theirs")?;
    texcraft_history::History::open_or_init(&top)?;
    let config = top.join(".git/config");
    let text = std::fs::read_to_string(&config)?;
    std::fs::write(&config, text.replace("[texcraft]\n\thistory = true\n", ""))?;
    let dir = top.join("paper");
    let mut s = session();
    write(&mut s, "main.tex", "draft")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    write(&mut s, "main.tex", "draft 2")?;
    let automatic = s.execute("history.version", &json!({"automatic": true}))?;
    assert!(automatic["commit"].is_null(), "{automatic}");
    assert_eq!(std::fs::read_to_string(dir.join("main.tex"))?, "draft 2", "written all the same");
    assert_eq!(versions(&mut s)?, 0);
    let asked = s.execute("history.version", &json!({}))?;
    assert!(asked["commit"].is_string(), "{asked}");
    assert_eq!(versions(&mut s)?, 1);
    std::fs::remove_dir_all(&top)?;
    Ok(())
}

#[test]
fn in_the_browser_writing_keeps_the_files_and_a_version_is_recorded_apart() -> R {
    let mut s = session();
    s.use_library(Library::default());
    write(&mut s, "main.tex", "one")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    let first = versions(&mut s)?;
    s.take_library_changes();

    write(&mut s, "main.tex", "two")?;
    s.execute("project.write", &json!({}))?;
    let changes = s.take_library_changes();
    assert!(changes.iter().any(|c| matches!(c, Change::Saved(name, _) if name == "Paper")), "the files are kept");
    assert!(!changes.iter().any(|c| matches!(c, Change::Versions { .. })), "and no version");
    assert_eq!(versions(&mut s)?, first);

    s.execute("history.version", &json!({"automatic": true}))?;
    assert!(s.take_library_changes().iter().any(|c| matches!(c, Change::Versions { .. })));
    assert_eq!(versions(&mut s)?, first + 1);
    Ok(())
}

#[test]
fn a_project_with_no_place_has_nothing_to_write_or_version() -> R {
    let mut s = session();
    write(&mut s, "main.tex", "x")?;
    assert!(s.execute("project.write", &json!({})).is_err(), "no folder: Save As first");
    assert!(s.execute("history.version", &json!({})).is_err());
    assert!(s.is_dirty(), "still unsaved: closing asks");
    Ok(())
}
