//! Projects kept by name in a library (the browser's projects, with no folders).

use std::collections::BTreeMap;

use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::history::store::{Version, Versions};
use texcraft_engine::library::{Change, Library, Stored};
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
    assert!(
        matches!(&changes[..], [Change::Saved(name, _), Change::Versions { project, versions, .. }] if name == "Thesis" && project == "Thesis" && versions.len() == 1),
        "the shell is told what to store: the project, and its first version"
    );

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
    // One the page fetches: not built into the program.
    let template = catalog.templates().iter().find(|t| !t.is_builtin()).unwrap();
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

/// The page fetches only the templates that are not built in (the gallery counts the others as
/// kept), so a built-in one makes a project from the program's own files (the browser's every
/// built-in template once failed with "not fetched yet").
#[test]
fn a_built_in_template_becomes_a_library_project_without_being_fetched() {
    let mut s = session();
    let catalog = texcraft_templates::Catalog::builtin().unwrap();
    let builtin: Vec<_> = catalog.templates().iter().filter(|t| t.is_builtin()).collect();
    assert!(!builtin.is_empty(), "the catalogue has built-in templates");
    for (i, template) in builtin.iter().enumerate() {
        let name = format!("Project {i}");
        s.execute("project.from_template", &json!({"template": template.id, "folder": name})).unwrap_or_else(|e| panic!("{}: {e}", template.id));
        let main = ProjectPath::new(&template.main).unwrap();
        assert_eq!(s.project().main(), Some(&main), "{}", template.id);
        assert!(s.project().get(&main).is_some(), "{}: its main document is in the project", template.id);
    }
    assert_eq!(s.library().unwrap().names().len(), builtin.len(), "each saved at once");
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
    assert!(matches!(&s.take_library_changes()[..], [Change::Saved(_, stored), Change::Versions { .. }] if stored.review.len() == 1));
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

#[test]
fn a_suggestion_is_kept_with_a_library_project_and_comes_back() {
    let mut s = reviewer().unwrap();
    s.execute("project.save_as", &json!({"path": "Suggested"})).unwrap();
    s.execute("review.suggest", &json!({"path": "main.tex", "start": 4, "end": 9, "text": "slow"})).unwrap();
    s.execute("project.new", &json!({})).unwrap();
    s.execute("project.open", &json!({"path": "Suggested"})).unwrap();
    let list = s.execute("review.list", &json!({"kind": "suggestion"})).unwrap();
    assert_eq!(list["openSuggestions"], 1, "{list}");
    assert_eq!(first_thread_quote(&mut s).as_deref(), Some("quick"));
    let id = list["threads"][0]["id"].clone();
    s.execute("review.accept", &json!({"id": id})).unwrap();
    assert_eq!(s.execute("file.read", &json!({"path": "main.tex"})).unwrap()["text"], "The slow brown fox.");
}

// The history of a library project (C8): a version per save, kept by the shell beside the project.

fn write(s: &mut Session, text: &str) -> Result<(), String> {
    s.execute("file.write", &json!({"path": "main.tex", "text": text})).map(drop)
}

/// The history's versions, newest first: each one's id and its changed paths with how they changed.
fn versions(s: &mut Session) -> Result<Vec<(String, Vec<String>)>, String> {
    let list = s.execute("history.list", &json!({}))?;
    let commits = list["commits"].as_array().ok_or("no commits")?;
    Ok(commits
        .iter()
        .map(|c| {
            let changes = c["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|ch| format!("{} {}", ch["change"].as_str().unwrap_or(""), ch["path"].as_str().unwrap_or("")))
                .collect();
            (c["id"].as_str().unwrap_or("").to_owned(), changes)
        })
        .collect())
}

#[test]
fn each_save_that_changes_a_library_project_is_a_version_newest_first() -> Result<(), String> {
    let mut s = session();
    assert!(s.execute("history.list", &json!({})).is_err(), "an untitled project has no history");
    write(&mut s, "first\n")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    let saved = s.execute("project.save", &json!({}))?;
    assert!(saved["history"]["commit"].is_null(), "a save that changed nothing is no version: {saved}");
    write(&mut s, "second\n")?;
    s.execute("file.new", &json!({"path": "intro.tex"}))?;
    s.execute("project.save", &json!({}))?;
    let list = versions(&mut s)?;
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].1, ["added intro.tex", "modified main.tex"], "the newest first, with what it changed");
    assert!(list[1].1.contains(&"added main.tex".to_owned()));
    let added: usize = s.take_library_changes().iter().map(|c| if let Change::Versions { versions, .. } = c { versions.len() } else { 0 }).sum();
    assert_eq!(added, 2, "the shell stores each version once");
    let shown = s.execute("history.show", &json!({"commit": list[1].0, "path": "main.tex"}))?;
    assert_eq!(shown["text"], "first\n");
    let diff = s.execute("history.diff", &json!({"commit": list[0].0, "path": "main.tex"}))?;
    assert_eq!((diff["added"].as_u64(), diff["removed"].as_u64()), (Some(1), Some(1)), "{diff}");
    assert!(s.execute("history.commit", &json!({"message": "x"})).is_err(), "saving already makes the versions");
    Ok(())
}

#[test]
fn a_library_version_says_which_sections_it_changed() -> Result<(), String> {
    let mut s = session();
    let paper = "\\section{Introduction}\nWe begin.\n\\section{Results}\nIt works.\n";
    write(&mut s, paper)?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    write(&mut s, &paper.replace("It works.", "It works well."))?;
    s.execute("project.save", &json!({}))?;
    let list = s.execute("history.list", &json!({}))?;
    assert_eq!(list["commits"][0]["message"], "Edited Results (main.tex)");
    assert!(list["commits"][1]["message"].as_str().is_some_and(|m| m.starts_with("Added main.tex")), "{list}");
    Ok(())
}

#[test]
fn restoring_a_library_version_brings_it_back_unsaved_and_the_next_save_is_a_version() -> Result<(), String> {
    let mut s = session();
    write(&mut s, "old\n")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    write(&mut s, "new\n")?;
    s.execute("file.new", &json!({"path": "later.tex"}))?;
    s.execute("project.save", &json!({}))?;
    let first = versions(&mut s)?.last().map(|v| v.0.clone()).ok_or("no version")?;
    s.execute("history.restore", &json!({"commit": first}))?;
    assert_eq!(s.project().text(&ProjectPath::new("main.tex").unwrap()), Ok("old\n"));
    assert!(s.project().get(&ProjectPath::new("later.tex").unwrap()).is_none(), "a file added since leaves");
    assert!(s.is_dirty(), "restored as unsaved edits");
    s.execute("project.save", &json!({}))?;
    assert_eq!(versions(&mut s)?.len(), 3, "the history is never rewritten: the restore is a new version");
    Ok(())
}

/// What the web shell keeps: each change applied, as it applies them to IndexedDB.
#[derive(Default)]
struct Shell {
    projects: BTreeMap<String, Stored>,
    versions: BTreeMap<String, Vec<Version>>,
    blobs: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

impl Shell {
    fn keep(&mut self, changes: Vec<Change>) {
        for change in changes {
            match change {
                Change::Saved(name, stored) => {
                    self.projects.insert(name, *stored);
                }
                Change::Deleted(name) => {
                    self.projects.remove(&name);
                    self.versions.remove(&name);
                    self.blobs.remove(&name);
                }
                Change::Thread { .. } => {}
                Change::Versions { project, versions, blobs } => {
                    self.versions.entry(project.clone()).or_default().extend(versions);
                    self.blobs.entry(project).or_default().extend(blobs);
                }
            }
        }
    }

    /// The library the shell fills at the next start.
    fn reload(&self) -> Library {
        let mut library = Library::default();
        for (name, stored) in &self.projects {
            let mut stored = stored.clone();
            // In any order, as a store's keys come back.
            let mut versions = self.versions.get(name).cloned().unwrap_or_default();
            versions.reverse();
            stored.history = Versions::from_parts(versions, self.blobs.get(name).cloned().unwrap_or_default());
            library.insert(name.clone(), stored);
        }
        library
    }
}

#[test]
fn the_history_the_shell_stores_comes_back_and_moves_with_a_renamed_project() -> Result<(), String> {
    let mut shell = Shell::default();
    let mut s = session();
    write(&mut s, "one\n")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    write(&mut s, "two\n")?;
    s.execute("project.save", &json!({}))?;
    shell.keep(s.take_library_changes());
    let before = versions(&mut s)?;

    let mut s = session();
    s.use_library(shell.reload());
    s.execute("project.open", &json!({"path": "Paper"}))?;
    assert_eq!(versions(&mut s)?, before, "the same versions after a reload");
    s.execute("project.rename", &json!({"name": "Thesis"}))?;
    write(&mut s, "three\n")?;
    s.execute("project.save", &json!({}))?;
    shell.keep(s.take_library_changes());
    assert!(!shell.versions.contains_key("Paper"), "nothing is left under the old name");

    let mut s = session();
    s.use_library(shell.reload());
    s.execute("project.open", &json!({"path": "Thesis"}))?;
    let after = versions(&mut s)?;
    assert_eq!(after.len(), 3, "the history moved with the project and goes on");
    assert_eq!(&after[1..], &before[..]);
    let shown = s.execute("history.show", &json!({"commit": before[1].0, "path": "main.tex"}))?;
    assert_eq!(shown["text"], "one\n", "the files' bytes came back too");
    Ok(())
}

/// The browser's storage, as the shell writes the library's changes to it: here one that is full
/// for the projects' files.
fn store_all(s: &mut Session, store: impl Fn(&Change) -> Result<(), String>) -> Vec<String> {
    let mut refused = Vec::new();
    for change in s.take_library_changes() {
        if let Err(e) = store(&change) {
            s.not_stored(&change);
            refused.push(e);
        }
    }
    refused
}

fn full(change: &Change) -> Result<(), String> {
    match change {
        Change::Saved(..) | Change::Versions { .. } => Err("QuotaExceededError: The quota has been exceeded.".to_owned()),
        _ => Ok(()),
    }
}

#[test]
fn a_save_the_full_storage_refuses_leaves_the_project_unsaved_and_the_next_save_stores_it_again() {
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "Kept."})).unwrap();
    s.execute("project.save_as", &json!({"path": "Thesis"})).unwrap();
    assert!(store_all(&mut s, |_| Ok(())).is_empty());
    assert!(!s.is_dirty());

    s.execute("file.write", &json!({"path": "main.tex", "text": "Lost if not kept."})).unwrap();
    s.execute("project.save", &json!({})).unwrap();
    let refused = store_all(&mut s, full);
    assert!(!refused.is_empty() && refused.iter().all(|e| texcraft_engine::library::out_of_room(e)), "the refusal is not said as lack of room");
    assert!(s.is_dirty(), "a save the storage refused counts as saved");

    // Room was made: the next save stores the whole project again.
    s.execute("project.save", &json!({})).unwrap();
    let changes = s.take_library_changes();
    assert!(
        changes.iter().any(|c| matches!(c, Change::Saved(name, p) if name == "Thesis" && p.files.iter().any(|(_, b)| b == b"Lost if not kept.")))
    );
}

#[test]
fn a_refused_write_of_another_project_leaves_the_open_one_saved() {
    let mut s = session();
    s.execute("project.save_as", &json!({"path": "Open"})).unwrap();
    let _ = s.take_library_changes();
    s.not_stored(&Change::Thread { project: "Other".into(), file: "t.md".into(), text: None });
    assert!(!s.is_dirty());
    assert!(!texcraft_engine::library::out_of_room("NotFoundError: no such store"), "any error is taken for a full disk");
}

#[test]
fn a_project_downloaded_as_a_zip_opens_again_as_the_same_project() {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-zip-round-trip", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\input{chapters/one}\n"})).unwrap();
    s.execute("file.new", &json!({"path": "chapters/one.tex", "text": "Once."})).unwrap();
    s.execute("file.import", &json!({"path": "figures/dot.png", "data": "iVBORw0KGgo="})).unwrap();
    s.execute("project.save_as", &json!({"path": "Thesis"})).unwrap();
    let zip = dir.join("Thesis.zip");
    let done = s.execute("project.download_zip", &json!({"path": zip.display().to_string()})).unwrap();
    assert_eq!(done["files"], json!(s.project().len()));
    let files =
        |s: &Session| -> Vec<(String, Vec<u8>)> { s.project().files().map(|(p, c)| (p.as_str().to_owned(), c.as_bytes().to_vec())).collect() };
    let before = files(&s);

    let mut other = session();
    other.execute("project.import", &json!({"source": zip.display().to_string()})).unwrap();
    assert_eq!(files(&other), before, "the zip does not open as the same project");
    let _ = std::fs::remove_dir_all(&dir);
}
