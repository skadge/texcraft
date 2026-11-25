//! Finding and replacing across the project's files (`project.find`, `project.replace`).

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;

fn session() -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for (path, text) in [("main.tex", "Cat\n\\input{b}\n"), ("chapters/b.tex", "no match\nA cat, a cat."), ("notes.md", "nothing here")] {
        let id = if path == "main.tex" { "file.write" } else { "file.new" };
        s.execute(id, &json!({"path": path, "text": text}))?;
    }
    // A binary file that holds the pattern's bytes is not searched.
    s.execute("file.import", &json!({"path": "fig.png", "data": "Y2F0AA=="}))?;
    Ok(s)
}

fn text(s: &Session, path: &str) -> String {
    ProjectPath::new(path).ok().and_then(|p| s.project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn finding_in_the_project_groups_matches_by_text_file() {
    let mut s = session().unwrap();
    let found = s.execute("project.find", &json!({"pattern": "cat"})).unwrap();
    let files: Vec<&str> = found["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(files, ["chapters/b.tex", "main.tex"], "only the text files with matches are listed");
    let b = &found["files"][0];
    assert_eq!(b["count"], 2);
    let second = &b["matches"][1];
    assert_eq!((second["line"].as_u64(), second["column"].as_u64()), (Some(2), Some(10)));
    assert_eq!(
        format!("{}{}{}", second["before"].as_str().unwrap(), second["text"].as_str().unwrap(), second["after"].as_str().unwrap()),
        "A cat, a cat."
    );
    assert_eq!(found["count"], 3);
    let cased = s.execute("project.find", &json!({"pattern": "cat", "case": true})).unwrap();
    assert!(cased["count"].as_u64() < found["count"].as_u64(), "Match Case found as many");
}

#[test]
fn a_long_line_is_clipped_around_the_match() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let line = format!("{}needle{}", "a".repeat(5000), "b".repeat(5000));
    s.execute("file.write", &json!({"path": "main.tex", "text": line})).unwrap();
    let found = s.execute("project.find", &json!({"pattern": "needle"})).unwrap();
    let m = &found["files"][0]["matches"][0];
    let shown = m["before"].as_str().unwrap().len() + m["after"].as_str().unwrap().len();
    assert!(shown < line.len() / 10, "the whole line came back");
    assert_eq!(m["column"], 5001);
}

#[test]
fn replacing_in_the_project_makes_one_edit_per_file() {
    let mut s = session().unwrap();
    let before = s.revision();
    let r = s.execute("project.replace", &json!({"pattern": "cat", "replacement": "dog"})).unwrap();
    assert_eq!(r["replaced"], 3);
    assert_eq!(text(&s, "main.tex"), "dog\n\\input{b}\n");
    assert_eq!(text(&s, "chapters/b.tex"), "no match\nA dog, a dog.");
    for path in ["main.tex", "chapters/b.tex"] {
        let edits = s.edits_since(&ProjectPath::new(path).unwrap(), before).unwrap();
        assert_eq!(edits.len(), 1, "{path}'s replacements are not one edit");
    }
    assert!(s.edits_since(&ProjectPath::new("notes.md").unwrap(), before).unwrap_or_default().is_empty(), "a file without matches was edited");
}

#[test]
fn replacing_in_listed_files_leaves_the_others() {
    let mut s = session().unwrap();
    let r = s.execute("project.replace", &json!({"pattern": "cat", "replacement": "dog", "files": ["chapters/b.tex"]})).unwrap();
    assert_eq!(r["files"].as_array().map(Vec::len), Some(1));
    assert_eq!(text(&s, "main.tex"), "Cat\n\\input{b}\n");
    assert!(text(&s, "chapters/b.tex").contains("dog"));
}

#[test]
fn a_failing_replacement_changes_no_file() {
    let mut s = session().unwrap();
    let before = s.revision();
    assert!(s.execute("project.replace", &json!({"pattern": "(", "regex": true, "replacement": "x"})).is_err());
    // A listed file that is not text fails the whole replacement, even after one that matched.
    let r: Result<Value, _> = s.execute("project.replace", &json!({"pattern": "cat", "replacement": "dog", "files": ["main.tex", "fig.png"]}));
    assert!(r.is_err());
    assert_eq!(s.revision(), before);
    assert_eq!(text(&s, "main.tex"), "Cat\n\\input{b}\n");
}
