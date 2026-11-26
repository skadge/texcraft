//! The project's history (C8): a folder in no repository becomes one on its first save and each
//! save is a commit; a folder in someone else's repository is committed to only when asked.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::file_access::AutomationRoots;

type R = Result<(), Box<dyn std::error::Error>>;

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-history-{name}", std::process::id()));
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

/// A repository someone else made, in `dir`.
fn their_repository(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    texcraft_history::History::open_or_init(dir)?;
    let config = dir.join(".git/config");
    let text = std::fs::read_to_string(&config)?;
    std::fs::write(&config, text.replace("[texcraft]\n\thistory = true\n", ""))?;
    Ok(())
}

fn messages_and_changes(s: &mut Session) -> Result<Vec<(String, Vec<String>)>, String> {
    let list = s.execute("history.list", &json!({}))?;
    Ok(list["commits"]
        .as_array()
        .map(|c| {
            c.iter()
                .map(|e| {
                    let changes = e["changes"]
                        .as_array()
                        .map(|ch| ch.iter().map(|x| format!("{} {}", x["change"].as_str().unwrap_or(""), x["path"].as_str().unwrap_or(""))).collect())
                        .unwrap_or_default();
                    (e["message"].as_str().unwrap_or("").to_owned(), changes)
                })
                .collect()
        })
        .unwrap_or_default())
}

#[test]
fn a_new_folder_project_becomes_a_repository_and_each_save_that_changes_it_a_commit() -> R {
    let dir = temp("new")?;
    let mut s = session();
    assert!(s.execute("history.list", &json!({})).is_err(), "no history before the project has a folder");
    write(&mut s, "main.tex", "first")?;
    let saved = s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    assert_eq!(saved["history"]["repository"], "ours");
    assert!(saved["history"]["commit"].is_string(), "{saved}");
    assert!(dir.join(".git").is_dir());

    let again = s.execute("project.save", &json!({}))?;
    assert!(again["history"]["commit"].is_null(), "nothing changed: no commit ({again})");

    write(&mut s, "main.tex", "second")?;
    s.execute("file.new", &json!({"path": "intro.tex", "text": "hello"}))?;
    let third = s.execute("project.save", &json!({}))?;
    assert!(third["history"]["commit"].is_string(), "{third}");

    let log = messages_and_changes(&mut s)?;
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[0].1, ["added intro.tex", "modified main.tex"], "newest first, with what changed");
    assert!(log[1].1.contains(&"added main.tex".to_owned()));
    assert!(log[0].0.starts_with("Edited main.tex; added intro.tex"), "a save's commit says what it changed: {:?}", log[0].0);

    // Opened again, the folder is still our repository.
    let mut t = session();
    t.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    assert_eq!(t.execute("history.status", &json!({}))?["repository"], "ours");
    let first = t.execute("history.list", &json!({}))?["commits"][1]["id"].as_str().map(str::to_owned).ok_or("no id")?;
    assert_eq!(t.execute("history.show", &json!({"commit": first, "path": "main.tex"}))?["text"], "first");
    assert!(t.execute("history.show", &json!({"commit": first, "path": "intro.tex"})).is_err(), "not in that version");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_folder_in_someone_elses_repository_is_committed_to_only_when_asked() -> R {
    let top = temp("theirs")?;
    their_repository(&top)?;
    let dir = top.join("paper");
    let mut s = session();
    write(&mut s, "main.tex", "draft")?;
    let saved = s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    assert_eq!(saved["history"]["repository"], "theirs");
    assert!(saved["history"]["commit"].is_null(), "no commit in their repository on save");
    assert!(!dir.join(".git").exists(), "no repository made inside theirs");
    assert!(messages_and_changes(&mut s)?.is_empty());

    write(&mut s, "main.tex", "draft 2")?;
    assert!(s.execute("history.commit", &json!({"message": "Draft"})).is_err(), "unsaved changes are saved first");
    s.execute("project.save", &json!({}))?;
    let made = s.execute("history.commit", &json!({"message": "Draft"}))?;
    assert!(made["commit"].is_string(), "{made}");
    assert_eq!(messages_and_changes(&mut s)?, [("Draft".to_owned(), vec!["added main.tex".to_owned()])]);
    assert!(s.execute("history.commit", &json!({"message": "Again"}))?["commit"].is_null(), "nothing changed");
    std::fs::remove_dir_all(&top)?;
    Ok(())
}

#[test]
fn restoring_brings_back_an_old_version_as_unsaved_edits_and_the_next_save_commits_it() -> R {
    let dir = temp("restore")?;
    let mut s = session();
    write(&mut s, "main.tex", "old")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    write(&mut s, "main.tex", "new")?;
    s.execute("file.new", &json!({"path": "later.tex", "text": "later"}))?;
    s.execute("project.save", &json!({}))?;
    let first = s.execute("history.list", &json!({}))?["commits"][1]["id"].as_str().map(str::to_owned).ok_or("no id")?;

    let newest = s.execute("history.list", &json!({}))?["commits"][0]["id"].as_str().map(str::to_owned).ok_or("no id")?;
    let diff = s.execute("history.diff", &json!({"commit": newest, "path": "main.tex"}))?;
    let lines = &diff["hunks"][0]["lines"];
    assert_eq!((lines[0]["kind"].as_str(), lines[0]["text"].as_str()), (Some("removed"), Some("old")), "{diff}");
    assert_eq!((lines[1]["kind"].as_str(), lines[1]["text"].as_str()), (Some("added"), Some("new")), "{diff}");
    let added = s.execute("history.diff", &json!({"commit": newest, "path": "later.tex"}))?;
    assert_eq!(added["change"], "added", "against the parent, which had no such file");

    let one = s.execute("history.restore", &json!({"commit": first, "path": "main.tex"}))?;
    assert_eq!(one["restored"], json!(["main.tex"]));
    assert!(s.is_dirty(), "a restored version is an unsaved edit");
    assert_eq!(s.execute("file.read", &json!({"path": "main.tex"}))?["text"], "old");
    assert!(s.project().paths().any(|p| p.as_str() == "later.tex"), "one file: the rest stays");

    let all = s.execute("history.restore", &json!({"commit": first}))?;
    assert_eq!(all["removed"], json!(["later.tex"]), "{all}");
    let saved = s.execute("project.save", &json!({}))?;
    assert!(saved["history"]["commit"].is_string(), "the restore is a new commit: {saved}");
    let log = messages_and_changes(&mut s)?;
    assert_eq!(log.len(), 3, "the history is never rewritten");
    assert_eq!(log[0].1, ["deleted later.tex", "modified main.tex"]);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_commit_in_someone_elses_repository_leaves_what_they_staged_alone() -> R {
    let dir = temp("staged")?;
    their_repository(&dir)?;
    let mut s = session();
    write(&mut s, "main.tex", "one")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    let first = s.execute("history.commit", &json!({"message": "One"}))?["commit"].as_str().map(str::to_owned).ok_or("no commit")?;
    write(&mut s, "main.tex", "two")?;
    s.execute("project.save", &json!({}))?;
    s.execute("history.commit", &json!({"message": "Two"}))?;
    // As `git reset --soft` leaves it: HEAD back at the first commit, the second's files staged.
    std::fs::write(dir.join(".git/refs/heads/main"), format!("{first}\n"))?;
    let index = std::fs::read(dir.join(".git/index"))?;
    let refused = s.execute("history.commit", &json!({"message": "Three"}));
    assert!(refused.as_ref().is_err_and(|e| e.contains("staged")), "{refused:?}");
    assert_eq!(std::fs::read(dir.join(".git/index"))?, index, "the index is as the user left it");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn an_agent_never_reaches_a_repository_outside_its_roots() -> R {
    let top = temp("roots")?;
    their_repository(&top)?;
    let dir = top.join("paper");
    std::fs::create_dir_all(&dir)?;
    let mut s = session();
    s.confine(AutomationRoots::new(std::slice::from_ref(&dir), std::slice::from_ref(&dir))?);
    write(&mut s, "main.tex", "x")?;
    let saved: Value = s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    assert!(saved["history"]["error"].as_str().is_some_and(|e| e.contains("refused")), "{saved}");
    assert!(!dir.join(".git").exists(), "no repository made inside theirs");
    assert!(s.execute("history.list", &json!({})).is_err());
    std::fs::remove_dir_all(&top)?;
    Ok(())
}
