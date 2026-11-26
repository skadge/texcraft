//! The whole document's outline (`file.document_outline`, docs/ROADMAP.md E14): through the main
//! file's includes, numbered, and made again when an included file changes.

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;

fn session(files: &[(&str, &str)]) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for (path, text) in files {
        let _ = s.execute("file.new", &json!({"path": path}));
        assert!(s.execute("file.write", &json!({"path": path, "text": text})).is_ok());
    }
    assert!(s.execute("file.set_main", &json!({"path": "main.tex"})).is_ok());
    s
}

fn titles(outline: &Value) -> Vec<(String, String)> {
    outline["entries"]
        .as_array()
        .map(|e| {
            e.iter()
                .filter(|e| e["kind"] == "heading")
                .map(|e| (e["title"].as_str().unwrap_or_default().to_owned(), e["file"].as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_outline_follows_the_includes_and_follows_their_edits() {
    let main = "\\documentclass{article}\n\\begin{document}\n\\section{Intro}\n\\input{one}\n\\include{missing}\n\\end{document}\n";
    let mut s = session(&[("main.tex", main), ("one.tex", "\\section{Method}\n")]);
    let outline = s.execute("file.document_outline", &json!({})).unwrap_or_default();
    assert_eq!(titles(&outline), [("Intro".to_owned(), "main.tex".to_owned()), ("Method".to_owned(), "one.tex".to_owned())]);
    let method = &outline["entries"][1];
    assert_eq!(method["number"], "2", "an included heading is numbered after the main file's");
    let missing: Vec<&Value> = outline["entries"].as_array().map(|e| e.iter().filter(|e| e["kind"] == "include").collect()).unwrap_or_default();
    assert!(missing.len() == 1 && missing[0]["problem"] == "missing" && missing[0]["name"] == "missing", "{missing:?}");
    assert!(s.execute("file.write", &json!({"path": "one.tex", "text": "\\section{Method}\n\\section{Results}\n"})).is_ok());
    let after = s.execute("file.document_outline", &json!({})).unwrap_or_default();
    assert!(titles(&after).len() > titles(&outline).len(), "an edit of an included file did not reach the outline");
}
