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

fn text_of(s: &mut Session) -> Result<String, String> {
    Ok(str_of(&s.execute("file.read", &json!({"path": "main.tex"}))?["text"]))
}

#[test]
fn a_suggestion_leaves_the_file_is_kept_in_the_folder_and_accepting_it_makes_the_edit() -> R {
    let dir = project("suggest", "The colour of the sky.\n")?;
    let mut s = open(&dir, "Bob")?;
    let r = s.execute("review.suggest", &json!({"path": "main.tex", "start": 4, "end": 10, "text": "color"}))?;
    let id = str_of(&r["id"]);
    assert_eq!(text_of(&mut s)?, "The colour of the sky.\n", "suggesting changed the file");
    assert_eq!(r["thread"]["kind"], "suggestion");
    let file = std::fs::read_to_string(&review_files(&dir)?[0])?;
    assert!(file.contains("\"suggestion\""), "{file}");
    // Typed before it, then read again from the folder: it is on its passage.
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": 0, "end": 0, "text": "See: "}]}))?;
    s.execute("project.save", &json!({}))?;
    let mut ada = open(&dir, "Ada")?;
    let list = ada.execute("review.list", &json!({"kind": "suggestion"}))?;
    assert_eq!(list["openSuggestions"], 1);
    ada.execute("review.reply", &json!({"id": id, "text": "Yes, American spelling."}))?;
    ada.execute("review.accept", &json!({"id": id}))?;
    assert_eq!(text_of(&mut ada)?, "See: The color of the sky.\n");
    let edits = ada.edits_since(&texcraft_engine::project::ProjectPath::new("main.tex")?, 0).unwrap_or_default();
    assert!(edits.last().is_some_and(|e| e.origin == texcraft_engine::Origin::Review), "the accept is not an edit with the review origin");
    assert!(ada.execute("review.accept", &json!({"id": id})).is_err(), "a suggestion was accepted twice");
    let file = std::fs::read_to_string(&review_files(&dir)?[0])?;
    assert!(file.contains("accepted"), "{file}");
    Ok(())
}

#[test]
fn rejecting_leaves_the_text_and_undoing_an_accept_reopens_the_suggestion() -> R {
    let dir = project("undo-accept", "one two three\n")?;
    let mut s = open(&dir, "Ada")?;
    let two = str_of(&s.execute("review.suggest", &json!({"path": "main.tex", "start": 4, "end": 7, "text": "2"}))?["id"]);
    let three = str_of(&s.execute("review.suggest", &json!({"path": "main.tex", "start": 8, "end": 13, "text": "3"}))?["id"]);
    s.execute("review.reject", &json!({"id": three}))?;
    assert_eq!(text_of(&mut s)?, "one two three\n");
    s.execute("review.accept", &json!({"id": two}))?;
    assert_eq!(text_of(&mut s)?, "one 2 three\n");
    // The editor's undo of it, sent back as an edit with the undo origin.
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": 4, "end": 5, "text": "two"}], "origin": "undo"}))?;
    assert_eq!(text_of(&mut s)?, "one two three\n");
    let open = s.execute("review.list", &json!({"state": "open", "kind": "suggestion"}))?;
    assert_eq!(open["threads"][0]["id"], json!(two), "the undone accept did not reopen its suggestion");
    s.execute("review.accept", &json!({"id": two}))?;
    assert_eq!(text_of(&mut s)?, "one 2 three\n");
    Ok(())
}

#[test]
fn accept_all_is_one_edit_and_reports_the_overlapping_suggestions_as_outdated() -> R {
    let dir = project("accept-all", "a quick brown fox\n")?;
    let mut s = open(&dir, "Ada")?;
    for (start, end, text) in [(2, 7, "slow"), (14, 17, "dog"), (4, 13, "ick red")] {
        s.execute("review.suggest", &json!({"path": "main.tex", "start": start, "end": end, "text": text}))?;
    }
    let before = s.revision();
    let r = s.execute("review.accept_all", &json!({"path": "main.tex"}))?;
    assert_eq!(text_of(&mut s)?, "a slow brown dog\n");
    assert_eq!(s.revision(), before + 1, "Accept All was not one edit");
    assert_eq!(r["accepted"], 2);
    assert_eq!(r["outdated"].as_array().map(Vec::len), Some(1));
    // Outdated, the last can still be rejected, and Reject All takes what is left.
    let r = s.execute("review.reject_all", &json!({}))?;
    assert_eq!(r["rejected"], 1);
    assert_eq!(s.execute("review.list", &json!({"state": "open"}))?["open"], 0);
    Ok(())
}
