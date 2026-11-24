//! Empty folders (`file.new_folder`) and moving a file into a folder (`file.rename`).

use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::review::Author;

type R = Result<(), String>;

fn session() -> Session {
    Session::with_backend(Box::new(Unavailable("tests".into())))
}

fn empty_folders(s: &Session) -> Vec<String> {
    s.empty_folders().map(|f| f.as_str().to_owned()).collect()
}

#[test]
fn a_new_folder_is_shown_until_a_file_is_in_it_and_never_makes_the_project_unsaved() -> R {
    let mut s = session();
    let dirty = s.is_dirty();
    s.execute("file.new_folder", &json!({"path": "chapters"}))?;
    assert_eq!(empty_folders(&s), ["chapters"]);
    assert_eq!(s.is_dirty(), dirty, "an empty folder has nothing to save");
    assert!(s.execute("file.new_folder", &json!({"path": "chapters"})).is_err(), "made twice");
    assert!(s.execute("file.new_folder", &json!({"path": "main.tex"})).is_err(), "over a file");
    assert!(s.execute("file.new", &json!({"path": "chapters"})).is_err(), "a file over the folder");
    s.execute("file.new", &json!({"path": "chapters/intro.tex"}))?;
    assert!(empty_folders(&s).is_empty(), "the folder now exists through its file");
    assert!(s.execute("file.new_folder", &json!({"path": "chapters"})).is_err(), "a folder with files exists");
    Ok(())
}

#[test]
fn a_file_moved_into_a_folder_keeps_its_tab_its_main_mark_and_its_comments() -> R {
    let mut s = session();
    s.set_author(Some(Author::new("Ada", "k")));
    s.execute("file.write", &json!({"path": "main.tex", "text": "The quick fox."}))?;
    s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 9, "text": "Which?"}))?;
    s.execute("file.new_folder", &json!({"path": "src"}))?;
    s.execute("file.rename", &json!({"from": "main.tex", "to": "src/main.tex"}))?;
    let moved = |p: Option<&texcraft_engine::project::ProjectPath>| p.map(|p| p.as_str().to_owned());
    assert_eq!(moved(s.current()).as_deref(), Some("src/main.tex"), "the shown tab follows");
    assert!(s.open_files().iter().all(|p| p.as_str() != "main.tex"));
    assert_eq!(moved(s.project().main()).as_deref(), Some("src/main.tex"), "the main mark follows");
    let threads = s.execute("review.list", &json!({"path": "src/main.tex"}))?;
    assert_eq!(threads["threads"].as_array().map(Vec::len), Some(1), "the comment follows: {threads}");
    assert!(empty_folders(&s).is_empty());
    Ok(())
}

#[test]
fn a_file_is_not_moved_over_another() -> R {
    let mut s = session();
    s.execute("file.new", &json!({"path": "a/x.tex", "text": "kept"}))?;
    s.execute("file.new", &json!({"path": "x.tex", "text": "moved"}))?;
    let err = s.execute("file.rename", &json!({"from": "x.tex", "to": "a/x.tex"})).unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(s.execute("file.read", &json!({"path": "a/x.tex"}))?["text"], "kept");
    assert_eq!(s.execute("file.read", &json!({"path": "x.tex"}))?["text"], "moved");
    Ok(())
}
