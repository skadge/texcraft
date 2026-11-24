//! Projects kept by name in a library (the browser's projects, with no folders).

use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::library::{Change, Library};
use texcraft_engine::project::ProjectPath;

fn session() -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.use_library(Library::default());
    s
}

#[test]
fn a_project_saved_by_name_opens_again_with_its_files_and_main_document() {
    let mut s = session();
    s.execute("file.new", &json!({"path": "chapters/one.tex"})).unwrap();
    s.execute("file.write", &json!({"path": "chapters/one.tex", "text": "Once"})).unwrap();
    s.execute("project.save_as", &json!({"path": "  Thesis "})).unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.dir().and_then(|d| d.to_str()), Some("Thesis"), "the name is trimmed");
    let changes = s.take_library_changes();
    assert!(matches!(&changes[..], [Change::Saved(name, _)] if name == "Thesis"), "the shell is told what to store");

    s.execute("project.new", &json!({})).unwrap();
    assert!(s.project().get(&ProjectPath::new("chapters/one.tex").unwrap()).is_none());
    s.execute("project.open", &json!({"path": "Thesis"})).unwrap();
    assert_eq!(s.project().text(&ProjectPath::new("chapters/one.tex").unwrap()).unwrap(), "Once");
    assert_eq!(s.project().main().map(|m| m.as_str()), Some("main.tex"));
}

#[test]
fn save_as_does_not_replace_another_project_of_the_same_name() {
    let mut s = session();
    s.execute("project.save_as", &json!({"path": "Paper"})).unwrap();
    s.execute("project.save", &json!({})).unwrap();
    s.execute("project.new", &json!({})).unwrap();
    assert!(s.execute("project.save_as", &json!({"path": "Paper"})).is_err());
    assert!(s.execute("project.save_as", &json!({"path": "a/b"})).is_err());
    assert!(s.execute("project.save_as", &json!({"path": " "})).is_err());
}

#[test]
fn deleting_the_open_project_leaves_it_open_and_unsaved() {
    let mut s = session();
    s.execute("project.save_as", &json!({"path": "Notes"})).unwrap();
    s.execute("project.delete", &json!({"name": "Notes"})).unwrap();
    assert!(s.library().unwrap().names().is_empty());
    assert!(s.dir().is_none() && s.is_dirty(), "the text is not lost: it can be saved under a new name");
    assert!(matches!(s.take_library_changes().last(), Some(Change::Deleted(n)) if n == "Notes"));
}

#[test]
fn a_library_project_writes_nothing_to_disk() {
    let cwd = std::env::current_dir().unwrap();
    let mut s = session();
    s.execute("project.save_as", &json!({"path": "Local"})).unwrap();
    s.execute("project.open", &json!({"path": "Local"})).unwrap();
    assert!(!cwd.join("Local").exists(), "the name is not a folder");
}

#[test]
fn a_template_the_page_fetched_becomes_a_saved_project_of_the_library() {
    let mut s = session();
    let catalog = texcraft_templates::Catalog::builtin().unwrap();
    let template = &catalog.templates()[0];
    let params = json!({"template": template.id, "folder": "Article"});
    assert!(s.execute("project.from_template", &params).is_err(), "not fetched yet");
    let mut files: Vec<(String, Vec<u8>)> = template.project_paths().into_iter().map(|(_, p)| (p, b"\\documentclass{article}".to_vec())).collect();
    files.push(("TEMPLATE-LICENSE".into(), b"licence".to_vec()));
    s.keep_template(template.id.clone(), files);
    s.execute("project.from_template", &params).unwrap();
    assert_eq!(s.project().main().map(|m| m.as_str()), Some(template.main.as_str()));
    assert!(s.project().get(&ProjectPath::new("TEMPLATE-LICENSE").unwrap()).is_some(), "the licence note goes with the project");
    assert_eq!(s.library().unwrap().names(), ["Article"], "saved at once");
    assert!(s.execute("project.from_template", &params).is_err(), "the name is taken");
}

fn reviewer() -> Result<Session, String> {
    let mut s = session();
    s.set_author(Some(texcraft_engine::review::Author::new("Ada", "Ada-key".to_owned())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "The quick brown fox."}))?;
    Ok(s)
}

fn first_thread_quote(s: &mut Session) -> Option<String> {
    let list = s.execute("review.list", &json!({})).ok()?;
    let anchor = &list["threads"].get(0)?["anchor"];
    let (start, end) = (anchor["start"].as_u64()? as usize, anchor["end"].as_u64()? as usize);
    let text = s.execute("file.read", &json!({"path": "main.tex"})).ok()?["text"].as_str()?.to_owned();
    Some(text.chars().skip(start).take(end.checked_sub(start)?).collect())
}

#[test]
fn a_comment_is_kept_with_a_library_project_at_once_and_comes_back_on_its_passage() {
    let mut s = reviewer().unwrap();
    s.execute("project.save_as", &json!({"path": "Essay"})).unwrap();
    s.take_library_changes();
    s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 9, "text": "Which fox?"})).unwrap();
    let changes = s.take_library_changes();
    let text = changes.iter().find_map(|c| match c {
        Change::Thread { project, text: Some(text), .. } if project == "Essay" => Some(text.clone()),
        _ => None,
    });
    assert!(text.is_some_and(|t| texcraft_engine::review::format::read(&t).is_ok()), "the thread is stored at once, as a thread file's text");

    // Text typed before the passage, then saved: the comment follows it.
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": 0, "end": 0, "text": "Look: "}]})).unwrap();
    s.execute("project.save", &json!({})).unwrap();
    s.execute("project.new", &json!({})).unwrap();
    assert!(s.review().threads().is_empty());
    s.execute("project.open", &json!({"path": "Essay"})).unwrap();
    assert_eq!(first_thread_quote(&mut s).as_deref(), Some("quick"));
}

#[test]
fn a_comment_made_before_the_first_save_is_saved_with_the_project() {
    let mut s = reviewer().unwrap();
    s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 9, "text": "Which fox?"})).unwrap();
    assert!(s.take_library_changes().is_empty(), "nowhere to keep it yet");
    s.execute("project.save_as", &json!({"path": "Draft"})).unwrap();
    assert!(matches!(&s.take_library_changes()[..], [Change::Saved(_, stored)] if stored.review.len() == 1));
    s.execute("project.new", &json!({})).unwrap();
    s.execute("project.open", &json!({"path": "Draft"})).unwrap();
    assert_eq!(first_thread_quote(&mut s).as_deref(), Some("quick"));
}

#[test]
fn a_deleted_comment_is_removed_from_the_library_project() {
    let mut s = reviewer().unwrap();
    s.execute("project.save_as", &json!({"path": "Memo"})).unwrap();
    let id = s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 9, "text": "Which fox?"})).unwrap()["id"].clone();
    s.take_library_changes();
    s.execute("review.delete", &json!({"id": id})).unwrap();
    assert!(matches!(&s.take_library_changes()[..], [Change::Thread { text: None, .. }]));
    s.execute("project.open", &json!({"path": "Memo"})).unwrap();
    assert!(s.review().threads().is_empty());
}
