//! Projects opened from GitLab (C5), against the fake GitLab: opening a branch, its changes,
//! committing them, a token kept for the session only. No test reaches the network.

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::fake::FakeGitLab;
use texcraft_engine::library::{Change, Library};
use texcraft_engine::project::ProjectPath;

const URL: &str = "https://gitlab.example/group/paper";
const TOKEN: &str = "glpat-engine-test";

fn fake() -> FakeGitLab {
    FakeGitLab::new(
        "https://gitlab.example",
        "group/paper",
        TOKEN,
        &[
            ("main.tex", b"\\documentclass{article}\n\\begin{document}\nHi\n\\end{document}\n"),
            ("intro.tex", b"Intro\n"),
            ("a/notes.tex", b"\\documentclass{memo}\n"),
        ],
    )
}

fn session(fake: &FakeGitLab) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let fake = fake.clone();
    s.set_forge_transport(move || fake.transport());
    s
}

fn write(s: &mut Session, path: &str, text: &str) -> Result<Value, String> {
    s.execute("file.write", &json!({"path": path, "text": text}))
}

fn text(s: &Session, path: &str) -> Option<String> {
    let path = ProjectPath::new(path).ok()?;
    s.project().text(&path).ok().map(str::to_owned)
}

fn changed(s: &mut Session) -> Result<Vec<String>, String> {
    let status = s.execute("forge.status", &json!({}))?;
    Ok(status["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| format!("{} {}", c["change"].as_str().unwrap_or(""), c["path"].as_str().unwrap_or("")))
        .collect())
}

#[test]
fn a_branch_opens_in_memory_and_its_edits_are_committed_back() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    let opened = s.execute("forge.open", &json!({"url": URL, "token": TOKEN}))?;
    assert_eq!(opened["files"], 3);
    assert_eq!(s.project().main().map(ProjectPath::as_str), Some("main.tex"), "the top-level main.tex is the main document");
    assert!(s.dir().is_none() && !s.is_dirty(), "the desktop opens it in memory, unchanged");
    assert_eq!(s.forge().map(|l| l.branch.as_str()), Some("main"));
    assert!(changed(&mut s)?.is_empty());

    write(&mut s, "intro.tex", "Better intro\n")?;
    s.execute("file.new", &json!({"path": "refs.bib"}))?;
    assert_eq!(changed(&mut s)?, ["modified intro.tex", "added refs.bib"]);
    assert!(s.is_dirty());

    // The token was kept for the session: the commit needs none.
    let done = s.execute("forge.commit", &json!({"message": "Better intro"}))?;
    assert!(done["commit"].is_string(), "{done}");
    assert_eq!(fake.files("main")["intro.tex"], b"Better intro\n");
    assert!(fake.files("main").contains_key("refs.bib"));
    assert!(changed(&mut s)?.is_empty(), "the commit is the new base");
    assert!(!s.is_dirty(), "nothing is left that the branch does not have");
    Ok(())
}

#[test]
fn the_token_is_in_no_result_and_not_kept_with_the_project() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    s.use_library(Library::default());
    let mut seen = s.execute("forge.open", &json!({"url": URL, "token": TOKEN}))?.to_string();
    write(&mut s, "main.tex", "x\n")?;
    seen += &s.execute("forge.status", &json!({}))?.to_string();
    seen += &s.execute("forge.commit", &json!({"message": "x"}))?.to_string();
    seen += &s.execute("project.inspect", &json!({}))?.to_string();
    seen += &format!("{:?}", s.take_library_changes());
    assert!(!seen.contains(TOKEN), "the token showed: {seen}");
    Ok(())
}

#[test]
fn in_the_browser_the_project_is_kept_with_its_link_and_asks_for_the_token_again() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    s.use_library(Library::default());
    s.execute("forge.open", &json!({"url": URL, "token": TOKEN}))?;
    assert_eq!(s.dir().and_then(|d| d.to_str()), Some("paper"), "kept under the project's name");
    write(&mut s, "main.tex", "edited\n")?;
    s.execute("project.save", &json!({}))?;

    // A reload: the shell gives back what it stored; the token is gone.
    let mut library = Library::default();
    for change in s.take_library_changes() {
        if let Change::Saved(name, stored) = change {
            library.insert(name, *stored);
        }
    }
    let mut s = session(&fake);
    s.use_library(library);
    s.execute("project.open", &json!({"path": "paper"}))?;
    assert_eq!(changed(&mut s)?, ["modified main.tex"], "the changes are counted from the stored base");
    let refused = s.execute("forge.commit", &json!({"message": "Edit"}));
    assert!(refused.as_ref().is_err_and(|e| e.contains("access token")), "{refused:?}");
    s.execute("forge.commit", &json!({"message": "Edit", "token": TOKEN}))?;
    assert_eq!(fake.files("main")["main.tex"], b"edited\n");
    let kept = s.take_library_changes().into_iter().rev().find_map(|c| if let Change::Saved(_, stored) = c { stored.forge } else { None });
    assert_eq!(kept.map(|l| l.base), fake.head("main"), "the browser keeps the new base");
    Ok(())
}

#[test]
fn a_branch_that_changed_the_same_file_refuses_and_keeps_the_edits() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    s.execute("forge.open", &json!({"url": URL, "token": TOKEN}))?;
    fake.push("main", &[("intro.tex", Some(b"Their intro\n".as_slice()))]);
    let head = fake.head("main");
    write(&mut s, "intro.tex", "My intro\n")?;
    let refused = s.execute("forge.commit", &json!({"message": "Mine"}));
    assert!(refused.as_ref().is_err_and(|e| e.contains("intro.tex")), "{refused:?}");
    assert_eq!(fake.head("main"), head);
    assert_eq!(text(&s, "intro.tex").as_deref(), Some("My intro\n"));
    assert_eq!(changed(&mut s)?, ["modified intro.tex"]);
    Ok(())
}

#[test]
fn what_others_committed_to_other_files_comes_into_the_project() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    s.execute("forge.open", &json!({"url": URL, "token": TOKEN}))?;
    fake.push("main", &[("intro.tex", Some(b"Their intro\n".as_slice())), ("a/notes.tex", None)]);
    write(&mut s, "main.tex", "mine\n")?;
    let done = s.execute("forge.commit", &json!({"message": "Mine"}))?;
    assert_eq!(done["taken"], json!(["intro.tex"]));
    assert_eq!(done["removed"], json!(["a/notes.tex"]));
    assert_eq!(text(&s, "intro.tex").as_deref(), Some("Their intro\n"));
    assert!(text(&s, "a/notes.tex").is_none());
    assert!(changed(&mut s)?.is_empty());
    Ok(())
}

#[test]
fn in_the_background_the_outcome_comes_from_polling() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    let started = s.execute("forge.open", &json!({"url": URL, "token": TOKEN, "background": true}))?;
    assert_eq!(started["started"], true);
    let outcome = loop {
        if let Some(outcome) = s.poll_forge() {
            break outcome?;
        }
        assert_eq!(s.forge_busy(), Some("open"));
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(outcome["operation"], "open");
    assert!(s.forge_busy().is_none());
    assert_eq!(text(&s, "intro.tex").as_deref(), Some("Intro\n"));
    Ok(())
}

#[test]
fn errors_are_said_and_change_nothing() -> Result<(), String> {
    let fake = fake();
    let mut s = session(&fake);
    write(&mut s, "main.tex", "before\n")?;
    let wrong = s.execute("forge.open", &json!({"url": URL, "token": "glpat-wrong"}));
    assert!(wrong.as_ref().is_err_and(|e| e.contains("did not accept the access token")), "{wrong:?}");
    assert_eq!(text(&s, "main.tex").as_deref(), Some("before\n"), "the open project was replaced");
    assert!(s.execute("forge.open", &json!({"url": URL})).is_err_and(|e| e.contains("access token")), "no token was asked for");
    assert!(s.execute("forge.commit", &json!({"message": "x"})).is_err(), "a project from no forge was committed");
    Ok(())
}
