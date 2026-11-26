//! Saving and versions apart (docs/ROADMAP.md, C9): writing the project's changes makes no
//! version; a version is made when asked or by itself, in any repository, and only when something
//! changed since the latest one. An automatic version amends the automatic one this session made
//! before it while that one is not pushed, so they stay few; no other commit is ever amended.

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

/// A repository made with git, not TeXCraft (its marker removed), with the project in `paper/`.
fn theirs(name: &str) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let top = temp(name)?;
    texcraft_history::History::open_or_init(&top)?;
    let config = top.join(".git/config");
    let text = std::fs::read_to_string(&config)?;
    std::fs::write(&config, text.replace("[texcraft]\n\thistory = true\n", ""))?;
    let dir = top.join("paper");
    Ok((top, dir))
}

fn log(s: &mut Session) -> Result<Vec<serde_json::Value>, String> {
    Ok(s.execute("history.list", &json!({}))?["commits"].as_array().cloned().unwrap_or_default())
}

#[test]
fn a_repository_someone_else_made_gets_automatic_versions_but_a_save_is_no_commit() -> R {
    // The operator's decision (C9): automatic versions in the user's own repositories too.
    let (top, dir) = theirs("theirs")?;
    let mut s = session();
    write(&mut s, "main.tex", "draft")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    assert_eq!(versions(&mut s)?, 0, "a save is not a commit in someone else's repository");
    write(&mut s, "main.tex", "draft 2")?;
    let automatic = s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(automatic["repository"], "theirs");
    assert!(automatic["commit"].is_string(), "{automatic}");
    assert_eq!(std::fs::read_to_string(dir.join("main.tex"))?, "draft 2");
    assert_eq!(versions(&mut s)?, 1);
    std::fs::remove_dir_all(&top)?;
    Ok(())
}

#[test]
fn automatic_versions_of_one_session_are_one_commit_that_says_all_they_changed() -> R {
    let (top, dir) = theirs("amend")?;
    let mut s = session();
    write(&mut s, "main.tex", "\\section{Intro}\nOne.\n")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    s.execute("history.version", &json!({"message": "Start"}))?;
    let start = versions(&mut s)?;

    write(&mut s, "main.tex", "\\section{Intro}\nOne, two.\n")?;
    let first = s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(first["amended"], false, "{first}");
    s.execute("file.new", &json!({"path": "notes.tex"}))?;
    write(&mut s, "notes.tex", "Notes.\n")?;
    let second = s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(second["amended"], true, "{second}");
    assert_ne!(first["commit"], second["commit"]);
    assert_eq!(versions(&mut s)?, start + 1, "two automatic versions, one commit");
    let latest = log(&mut s)?.first().cloned().unwrap_or_default();
    let message = latest["message"].as_str().unwrap_or_default();
    assert!(message.contains("Intro") && message.contains("notes.tex"), "the message says both changes: {message}");
    let changed: Vec<&str> = latest["changes"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str()).collect();
    assert!(changed.contains(&"main.tex") && changed.contains(&"notes.tex"), "{latest}");
    assert_eq!(latest["parent"], log(&mut s)?[1]["id"], "it follows the named version");

    // A named version is a point of its own: the automatic one before it is folded into it, and
    // the next automatic version is a commit after it.
    write(&mut s, "main.tex", "\\section{Intro}\nOne, two, three.\n")?;
    s.execute("history.version", &json!({"message": "Submitted"}))?;
    assert_eq!(versions(&mut s)?, start + 1, "the name takes the session's automatic version");
    write(&mut s, "main.tex", "\\section{Intro}\nFour.\n")?;
    s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(versions(&mut s)?, start + 2);
    assert_eq!(log(&mut s)?[1]["message"], "Submitted", "the named version stays as it was");
    std::fs::remove_dir_all(&top)?;
    Ok(())
}

#[test]
fn a_commit_made_outside_this_session_is_never_amended() -> R {
    let (top, dir) = theirs("outside")?;
    let mut s = session();
    write(&mut s, "main.tex", "one")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    s.execute("history.version", &json!({"automatic": true}))?;
    // git commits on its own (here through a second handle on the repository, as git would).
    std::fs::create_dir_all(top.join("other"))?;
    let git = texcraft_history::History::find(&top.join("other"))?.ok_or("no repository")?;
    let snapshot = texcraft_history::Snapshot { files: vec![("README".to_owned(), b"read me".to_vec())], owned_dirs: Vec::new() };
    let outside = git.commit(&snapshot, "Add a README", None, chrono::Utc::now())?.ok_or("no commit")?;
    let before = versions(&mut s)?;
    write(&mut s, "main.tex", "two")?;
    let made = s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(made["amended"], false, "{made}");
    assert_eq!(versions(&mut s)?, before + 1);
    // git's commit (outside the project's folder, so not in its log) is the new one's parent.
    assert_eq!(log(&mut s)?[0]["parent"], outside.as_str(), "git's commit is not kept as it was");

    // Another session (another TeXCraft) does not amend this one's automatic version either.
    let mine = s.execute("history.status", &json!({}))?["head"].clone();
    let mut other = session();
    other.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    write(&mut other, "main.tex", "three")?;
    let theirs = other.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(theirs["amended"], false, "{theirs}");
    assert_eq!(log(&mut other)?[1]["id"], mine, "the first session's version is kept");
    std::fs::remove_dir_all(&top)?;
    Ok(())
}

#[test]
fn a_version_on_a_remote_tracking_ref_is_never_amended() -> R {
    let dir = temp("tracked")?;
    let mut s = session();
    write(&mut s, "main.tex", "one")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    write(&mut s, "main.tex", "two")?;
    let made = s.execute("history.version", &json!({"automatic": true}))?;
    // As a fetch or push from git outside TeXCraft leaves it: the remote has the commit.
    let git = texcraft_history::History::find(&dir)?.ok_or("no repository")?;
    git.set_ref("refs/remotes/origin/main", made["commit"].as_str().ok_or("no commit")?, "push")?;
    let before = versions(&mut s)?;
    write(&mut s, "main.tex", "three")?;
    let next = s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(next["amended"], false, "{next}");
    assert_eq!(versions(&mut s)?, before + 1, "a pushed version stays; the next is a commit after it");
    std::fs::remove_dir_all(&dir)?;
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
fn in_the_browser_an_automatic_version_replaces_the_sessions_last_one() -> R {
    let mut s = session();
    s.use_library(Library::default());
    write(&mut s, "main.tex", "one")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    let first = versions(&mut s)?;
    write(&mut s, "main.tex", "two")?;
    let made = s.execute("history.version", &json!({"automatic": true}))?;
    s.take_library_changes();
    s.execute("file.new", &json!({"path": "refs.bib"}))?;
    write(&mut s, "refs.bib", "@misc{a, title = {A}}\n")?;
    let again = s.execute("history.version", &json!({"automatic": true}))?;
    assert_eq!(again["amended"], true, "{again}");
    assert_eq!(versions(&mut s)?, first + 1, "one version for both");
    let changes = s.take_library_changes();
    let replaced = changes.iter().any(|c| {
        matches!(c, Change::Versions { versions, removed, .. } if versions.len() == 1 && removed.first() == made["commit"].as_str().map(str::to_owned).as_ref())
    });
    assert!(replaced, "the shell is told to drop the version replaced: {changes:?}");
    let message = log(&mut s)?.first().and_then(|c| c["message"].as_str().map(str::to_owned)).unwrap_or_default();
    assert!(message.contains("main.tex") && message.contains("refs.bib"), "{message}");
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
