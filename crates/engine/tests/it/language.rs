use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;

fn session(text: &str) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    Ok(s)
}

#[test]
fn a_label_written_in_the_file_is_offered_inside_a_reference() {
    let mut s = session("\\section{A}\\label{sec:intro}\nSee \\ref{sec").unwrap();
    let r = s.execute("language.complete", &json!({"path": "main.tex", "line": 2, "column": 13})).unwrap();
    let labels: Vec<&str> = r["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).collect();
    assert_eq!(labels, ["sec:intro"]);
    assert_eq!(r["items"][0]["replace_from_column"], 10, "the replaced range does not start after the brace");
}

#[test]
fn the_lint_sees_the_text_as_it_changes() {
    let mut s = session("Fine.\n").unwrap();
    assert_eq!(s.execute("language.lint", &json!({"path": "main.tex"})).unwrap()["hints"].as_array().map(Vec::len), Some(0));
    s.execute("file.write", &json!({"path": "main.tex", "text": "$$x$$\n"})).unwrap();
    let hints = s.execute("language.lint", &json!({"path": "main.tex"})).unwrap();
    assert!(hints["hints"].as_array().is_some_and(|h| !h.is_empty()), "display math with $$ gave no hint: {hints}");
}
