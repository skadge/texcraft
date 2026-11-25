//! Integration tests for texcraft-history: repositories in scratch folders.

use std::path::{Path, PathBuf};

use texcraft_history::store::{Versions, blob_id};
use texcraft_history::{ChangeKind, History, Owner, Person, Snapshot, Versioned};

type R = Result<(), String>;

/// A new empty scratch folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("texcraft-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot(files: &[(&str, &str)]) -> Snapshot {
    Snapshot { files: files.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect(), owned_dirs: vec![".texcraft/review".into()] }
}

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

fn ada() -> Person {
    Person::new("Ada", "")
}

/// A repository someone else made: one of ours with TeXCraft's mark taken out.
fn theirs(dir: &Path) -> R {
    History::open_or_init(dir)?;
    let config = dir.join(".git/config");
    let text = std::fs::read_to_string(&config).map_err(|e| e.to_string())?;
    std::fs::write(&config, text.replace("[texcraft]\n\thistory = true\n", "")).map_err(|e| e.to_string())
}

#[test]
fn a_folder_in_no_repository_becomes_ours_on_main_with_the_build_files_ignored() -> R {
    let s = Scratch::new("new")?;
    assert!(History::find(&s.0)?.is_none());
    let h = History::open_or_init(&s.0)?;
    assert_eq!(h.owner(), Owner::Ours);
    assert_eq!(h.branch().as_deref(), Some("main"));
    assert!(h.head()?.is_none(), "no commit before the first save");
    let ignore = std::fs::read_to_string(s.0.join(".gitignore")).map_err(|e| e.to_string())?;
    assert!(ignore.contains(".retex/"));
    // Found again as ours.
    assert_eq!(History::find(&s.0)?.map(|h| h.owner()), Some(Owner::Ours));
    Ok(())
}

#[test]
fn an_unchanged_project_is_not_committed_again_and_changes_are_listed_newest_first() -> R {
    let s = Scratch::new("log")?;
    let h = History::open_or_init(&s.0)?;
    let first = h.commit(&snapshot(&[("main.tex", "one"), ("a.tex", "a")]), "first", Some(&ada()), now())?;
    assert!(first.is_some());
    assert_eq!(h.commit(&snapshot(&[("main.tex", "one"), ("a.tex", "a")]), "again", Some(&ada()), now())?, None);
    let second = h.commit(&snapshot(&[("main.tex", "two"), ("b/c.tex", "c")]), "second", None, now())?;
    assert!(second.is_some() && second != first);
    let log = h.log(10)?;
    assert_eq!(log.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["second", "first"]);
    assert_eq!(log[1].author, "Ada");
    assert_eq!(
        log[0].changes,
        [("a.tex".to_owned(), ChangeKind::Deleted), ("b/c.tex".to_owned(), ChangeKind::Added), ("main.tex".to_owned(), ChangeKind::Modified)]
    );
    assert!(log[1].changes.contains(&(".gitignore".to_owned(), ChangeKind::Added)), "the first commit has the ignore file: {:?}", log[1].changes);
    assert_eq!(h.log(1)?.len(), 1);
    let first = first.unwrap_or_default();
    assert_eq!(h.file_at(&first, "main.tex")?.as_deref(), Some(&b"one"[..]));
    assert_eq!(h.file_at(&first, "b/c.tex")?, None);
    let files = h.files_at(&first)?;
    assert_eq!(files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), ["a.tex", "main.tex"], "hidden files are not project files");
    assert!(h.file_at("not an id", "main.tex").is_err());
    Ok(())
}

#[test]
fn a_project_inside_someone_elses_repository_keeps_their_files() -> R {
    let s = Scratch::new("theirs")?;
    theirs(&s.0)?;
    let top = History::find(&s.0)?.ok_or("no repository")?;
    assert_eq!(top.owner(), Owner::Theirs);
    std::fs::create_dir_all(s.0.join("paper/.github")).map_err(|e| e.to_string())?;
    top.commit(&snapshot(&[("README", "theirs"), ("paper/.github/ci.yml", "ci"), ("paper/main.tex", "v1")]), "theirs", None, now())?;

    let h = History::find(&s.0.join("paper"))?.ok_or("no repository")?;
    assert_eq!(h.owner(), Owner::Theirs, "a folder above it has the repository");
    assert_eq!(h.prefix(), "paper");
    h.commit(&snapshot(&[("main.tex", "v2")]), "edit", Some(&ada()), now())?;
    top.commit(&snapshot(&[("README", "theirs, changed"), ("paper/main.tex", "v2")]), "readme", None, now())?;

    let head = h.head()?.ok_or("no head")?;
    let all = top.files_at(&head)?;
    let names: Vec<&str> = all.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["README", "paper/main.tex"], "their file outside the project stays");
    assert!(top.file_at(&head, "paper/.github/ci.yml")?.is_some(), "a hidden file the project does not load stays");
    let log = h.log(10)?;
    assert_eq!(log.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["edit", "theirs"], "only commits that touched the project");
    assert_eq!(log[0].changes, [("main.tex".to_owned(), ChangeKind::Modified)], "paths from the project's folder");
    Ok(())
}

#[test]
fn ignored_files_stay_out_unless_already_tracked() -> R {
    let s = Scratch::new("ignore")?;
    let h = History::open_or_init(&s.0)?;
    std::fs::write(s.0.join(".gitignore"), ".retex/\n*.aux\nbuild/\n").map_err(|e| e.to_string())?;
    h.commit(&snapshot(&[("main.tex", "x"), ("main.aux", "aux"), ("build/out.log", "log")]), "first", None, now())?;
    let head = h.head()?.ok_or("no head")?;
    let names: Vec<String> = h.files_at(&head)?.into_iter().map(|(p, _)| p).collect();
    assert_eq!(names, ["main.tex"]);
    Ok(())
}

#[test]
fn review_files_follow_the_snapshot_other_hidden_files_keep_their_version() -> R {
    let s = Scratch::new("review")?;
    let h = History::open_or_init(&s.0)?;
    h.commit(&snapshot(&[("main.tex", "x"), (".texcraft/review/a.json", "{}"), (".latexmkrc", "x\n")]), "first", None, now())?;
    h.commit(&snapshot(&[("main.tex", "y")]), "second", None, now())?;
    let head = h.head()?.ok_or("no head")?;
    assert_eq!(h.file_at(&head, ".texcraft/review/a.json")?, None, "a thread gone from the snapshot is gone");
    assert_eq!(h.file_at(&head, ".latexmkrc")?.as_deref(), Some(&b"x\n"[..]), "another hidden file keeps its version");
    Ok(())
}

// The history kept as values (the browser).

#[test]
fn kept_files_have_the_ids_git_gives_them() -> R {
    // So the history can be pushed to a git forge as it is (C5): what `git hash-object` says.
    assert_eq!(blob_id(b"")?, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    assert_eq!(blob_id(b"hello\n")?, "ce013625030ba8dba906f756967f9e9ca394464a");
    Ok(())
}

#[test]
fn kept_versions_share_unchanged_files_and_come_back_in_order() -> R {
    let mut h = Versions::default();
    h.record(&snapshot(&[("main.tex", "a"), ("big.png", "picture")]), "1", "Ada", now())?;
    let second = h.record(&snapshot(&[("main.tex", "b"), ("big.png", "picture")]), "2", "Ada", now())?.ok_or("no version")?;
    assert_eq!(second.blobs.len(), 1, "only the changed file's bytes are new");
    h.record(&snapshot(&[("main.tex", "c"), ("big.png", "picture")]), "3", "Ada", now())?;
    let mut shuffled = h.versions().to_vec();
    shuffled.swap(0, 2);
    let back = Versions::from_parts(shuffled, h.blobs().clone());
    assert_eq!(back, h, "the order comes from the parents, not from how they were stored");
    let messages: Vec<String> = back.log(10)?.into_iter().map(|e| e.message).collect();
    assert_eq!(messages, ["3", "2", "1"]);
    Ok(())
}
