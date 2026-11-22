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

#[test]
fn a_reference_leads_to_its_label() {
    let mut s = session("Intro.\n\\section{A}\\label{sec:a}\nSee \\ref{sec:a}.").unwrap();
    let r = s.execute("language.definition", &json!({"path": "main.tex", "line": 3, "column": 11})).unwrap();
    assert_eq!((r["line"].as_u64(), r["path"].as_str()), (Some(2), Some("main.tex")), "{r}");
}

#[test]
fn a_hint_has_a_fix_that_agents_can_apply() {
    let mut s = session("Old style: {\\bf bold}.\n").unwrap();
    let fixes = s.execute("language.fixes", &json!({"path": "main.tex", "line": 1})).unwrap();
    assert!(fixes["fixes"].as_array().is_some_and(|f| !f.is_empty()), "no fix for \\bf: {fixes}");
    s.execute("language.fix", &json!({"path": "main.tex", "line": 1, "index": 0})).unwrap();
    let text = s.project().text(&texcraft_engine::project::ProjectPath::new("main.tex").unwrap()).unwrap().to_owned();
    assert!(!text.contains("\\bf "), "the fix did not change the text: {text}");
}

/// "Builds" by handing back a `symbols` record that knows one bibliography entry.
struct WithSymbols;

impl texcraft_compile::Backend for WithSymbols {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &texcraft_compile::Job) -> texcraft_compile::Outcome {
        let symbols = json!({"type": "symbols", "files": [], "commands": [], "environments": [], "labels": [],
            "citations": [], "places": [],
            "bibliography": [{"key": "knuth84", "type": "book", "title": "The TeXbook", "author": "Knuth, Donald", "year": "1984", "file": "refs.bib", "line": 1}]});
        texcraft_compile::Outcome {
            status: texcraft_compile::Status::Built,
            symbols: Some(symbols),
            ..texcraft_compile::Outcome::unavailable(job.id, "")
        }
    }
}

#[test]
fn a_build_of_our_own_gives_completion_its_bibliography() {
    let mut s = Session::with_backend(Box::new(WithSymbols));
    s.execute("file.new", &json!({"path": "paper/main.tex"})).unwrap();
    s.execute("file.write", &json!({"path": "paper/main.tex", "text": "\\documentclass{article}\n\\cite{kn"})).unwrap();
    s.execute("file.set_main", &json!({"path": "paper/main.tex"})).unwrap();
    let keys = |s: &mut Session| {
        let r = s.execute("language.complete", &json!({"path": "paper/main.tex", "line": 2, "column": 9})).unwrap();
        r["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str().map(str::to_owned)).collect::<Vec<_>>()
    };
    assert!(!keys(&mut s).contains(&"knuth84".to_owned()), "the entry was offered before any build");
    s.execute("build.compile", &json!({"wait": true})).unwrap();
    assert!(keys(&mut s).contains(&"knuth84".to_owned()), "the build's entry is not offered: {:?}", keys(&mut s));
}
