//! R1: comments through the engine, kept in `.texcraft/review/` (docs/collaboration-roadmap.md).

use std::path::PathBuf;

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::review::Author;

type R = Result<(), Box<dyn std::error::Error>>;

fn project(name: &str, text: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-review-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("main.tex"), text)?;
    Ok(dir)
}

fn open(dir: &std::path::Path, who: &str) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_author(Some(Author::new(who, format!("{who}-key"))));
    s.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    Ok(s)
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn review_files(dir: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
    let folder = dir.join(".texcraft/review");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    Ok(std::fs::read_dir(folder)?.filter_map(|e| e.ok().map(|e| e.path())).collect())
}

#[test]
fn comments_are_kept_in_the_project_folder_and_come_back_on_their_passage() -> R {
    let dir = project("roundtrip", "\\documentclass{article}\nThe quick brown fox.\n")?;
    let mut s = open(&dir, "Ada")?;
    let r = s.execute("review.comment", &json!({"path": "main.tex", "start": 28, "end": 33, "text": "Which fox?"}))?;
    let id = str_of(&r["id"]);
    assert_eq!(review_files(&dir)?.len(), 1, "the thread was not written to its own file");
    s.execute("review.reply", &json!({"id": id, "text": "The brown one."}))?;
    // Text typed before the passage, then saved: the comment follows it.
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": 24, "end": 24, "text": "Look: "}]}))?;
    s.execute("project.save", &json!({}))?;
    assert!(!s.project().paths().any(|p| p.as_str().starts_with(".texcraft")), "the review is listed as a project file");

    let mut again = open(&dir, "Ada")?;
    assert!(!again.project().paths().any(|p| p.as_str().starts_with(".texcraft")), "the review folder was read as sources");
    let list = again.execute("review.list", &json!({}))?;
    let thread = &list["threads"][0];
    let (start, end) = (thread["anchor"]["start"].as_u64().unwrap_or(0) as usize, thread["anchor"]["end"].as_u64().unwrap_or(0) as usize);
    let text = str_of(&again.execute("file.read", &json!({"path": "main.tex"}))?["text"]);
    assert_eq!(text.chars().skip(start).take(end - start).collect::<String>(), "quick");
    assert_eq!(thread["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(thread["line"], 2);
    Ok(())
}

#[test]
fn resolving_reopening_and_deleting_change_the_files() -> R {
    let dir = project("states", "one two three\n")?;
    let mut s = open(&dir, "Ada")?;
    let id = str_of(&s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 7, "text": "Two?"}))?["id"]);
    s.execute("review.resolve", &json!({"id": id}))?;
    assert_eq!(s.execute("review.list", &json!({"state": "open"}))?["threads"].as_array().map(Vec::len), Some(0));
    let file = std::fs::read_to_string(&review_files(&dir)?[0])?;
    assert!(file.contains("resolved"), "{file}");
    s.execute("review.reopen", &json!({"id": id}))?;
    assert_eq!(s.execute("review.list", &json!({"state": "open"}))?["open"], 1);

    // Someone else may reply, but not delete Ada's thread or edit her message.
    let mut bob = open(&dir, "Bob")?;
    let message = str_of(&bob.execute("review.list", &json!({}))?["threads"][0]["messages"][0]["id"]);
    bob.execute("review.reply", &json!({"id": id, "text": "I think so."}))?;
    assert!(bob.execute("review.delete", &json!({"id": id})).is_err());
    assert!(bob.execute("review.edit", &json!({"id": id, "message": message, "text": "x"})).is_err());

    let mut s = open(&dir, "Ada")?;
    assert_eq!(s.execute("review.list", &json!({}))?["threads"][0]["messages"].as_array().map(Vec::len), Some(2), "Bob's reply was not saved");
    s.execute("review.delete", &json!({"id": id}))?;
    assert!(review_files(&dir)?.is_empty(), "the deleted thread's file stayed");
    Ok(())
}

#[test]
fn a_newer_review_file_is_reported_not_dropped_or_misread() -> R {
    let dir = project("newer", "text\n")?;
    std::fs::create_dir_all(dir.join(".texcraft/review"))?;
    let newer = dir.join(".texcraft/review/01JZZZZZZZZZZZZZZZZZZZZZZZ.json");
    std::fs::write(&newer, "{\"version\": 7, \"kind\": \"thread\"}")?;
    let mut s = open(&dir, "Ada")?;
    let list = s.execute("review.list", &json!({}))?;
    assert_eq!(list["threads"].as_array().map(Vec::len), Some(0));
    assert!(list["problems"][0].as_str().is_some_and(|p| p.contains("newer")), "{list}");
    assert!(newer.exists());
    Ok(())
}

#[test]
fn commenting_needs_a_name_and_a_passage_in_the_text() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "abc"})).unwrap();
    let err = s.execute("review.comment", &json!({"path": "main.tex", "start": 0, "end": 1, "text": "x"})).unwrap_err();
    assert!(err.contains("name"), "{err}");
    s.set_author(Some(Author::new("Ada", "k")));
    assert!(s.execute("review.comment", &json!({"path": "main.tex", "start": 2, "end": 9, "text": "x"})).is_err());
    // Without a folder the review stays in memory.
    assert!(s.execute("review.comment", &json!({"path": "main.tex", "start": 0, "end": 1, "text": "x"})).is_ok());
    assert_eq!(s.review().threads().len(), 1);
}
