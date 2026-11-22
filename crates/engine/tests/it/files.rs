use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;

fn session() -> Session {
    Session::with_backend(Box::new(Unavailable("tests".into())))
}

#[test]
fn a_build_without_an_engine_reports_it_instead_of_succeeding() {
    let mut s = session();
    let r = s.execute("build.compile", &json!({"wait": true})).unwrap();
    assert_eq!(r["result"]["status"], "unavailable");
    assert!(s.pdf().is_none());
}

#[test]
fn closing_the_shown_tab_shows_a_neighbour() {
    let mut s = session();
    s.execute("file.new", &json!({"path": "a.tex"})).unwrap();
    s.execute("file.new", &json!({"path": "b.tex"})).unwrap();
    s.execute("file.open", &json!({"path": "a.tex"})).unwrap();
    s.execute("file.close", &json!({"path": "a.tex"})).unwrap();
    assert!(s.current().is_some());
    assert!(!s.open_files().iter().any(|p| p.as_str() == "a.tex"));
}

#[test]
fn an_edit_makes_the_last_build_stale() {
    let mut s = session();
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    assert!(!s.is_stale());
    s.execute("file.write", &json!({"path": "main.tex", "text": "x"})).unwrap();
    assert!(s.is_stale());
}

#[test]
fn an_edit_replaces_exactly_its_range_and_refuses_split_characters() {
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "é and more"})).unwrap();
    assert!(s.execute("file.edit", &json!({"path": "main.tex", "start": 1, "end": 2, "text": "x"})).is_err());
    s.execute("file.edit", &json!({"path": "main.tex", "start": 3, "end": 6, "text": "\\textbf{and}"})).unwrap();
    let text = s.execute("file.read", &json!({"path": "main.tex"})).unwrap();
    assert_eq!(text["text"], "é \\textbf{and} more");
}

#[test]
fn a_file_given_by_its_contents_is_added_without_a_disk() {
    let mut s = session();
    // "fig" in base64, then a PNG's first bytes: one text file and one binary file.
    s.execute("file.import", &json!({"path": "notes.txt", "data": "Zmln"})).unwrap();
    s.execute("file.import", &json!({"path": "figure.png", "data": "iVBORw0KGgo="})).unwrap();
    let text = s.execute("file.read", &json!({"path": "notes.txt"})).unwrap();
    assert_eq!(text["text"], "fig", "{text}");
    let listed = s.execute("project.inspect", &json!({})).unwrap().to_string();
    assert!(listed.contains("figure.png"), "the binary file is not in the project: {listed}");
    assert!(s.execute("file.import", &json!({"path": "bad.txt", "data": "not base64!"})).is_err(), "bad base64 was taken");
    assert!(s.execute("file.import", &json!({"data": "Zmln"})).is_err(), "contents without a path were taken");
}
