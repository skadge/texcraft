//! Where a real build places its errors: where they start, with where TeX noticed them.

use std::sync::Arc;

use texcraft_compile::{Backend, Job, Options, Outcome, Severity};
use texcraft_project::ProjectPath;

fn build(name: &str, files: &[(&str, &str)]) -> Result<Outcome, String> {
    let dir = std::env::temp_dir().join(format!("texcraft-places-{name}-{}", std::process::id()));
    let mut backend = texcraft_compile::retex_backend::Retex::new(Some(dir.clone()));
    let main = ProjectPath::new(files[0].0).map_err(|e| e.to_string())?;
    let files = files
        .iter()
        .map(|(p, t)| Ok((ProjectPath::new(p).map_err(|e| e.to_string())?, Arc::from(t.as_bytes()))))
        .collect::<Result<Vec<_>, String>>()?;
    let outcome = backend.compile(&Job { files, main, id: 1, options: Options::default(), project: None, packages: Default::default() });
    let _ = std::fs::remove_dir_all(&dir);
    Ok(outcome)
}

const MAIN: &str = "\\documentclass{article}\n\\begin{document}\nHello.\n\\input{sections/intro}\nMore.\n\\end{document}\n";

#[test]
fn an_environment_left_open_in_an_included_file_is_reported_there() -> Result<(), String> {
    let o = build("env", &[("main.tex", MAIN), ("sections/intro.tex", "Intro.\n\n\\begin{itemize}\n\\item One\n")])?;
    let d = o
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error && d.message.contains("itemize"))
        .ok_or(format!("no error: {:?}", o.diagnostics))?;
    assert_eq!((d.file.as_deref(), d.line), (Some("sections/intro.tex"), Some(3)), "not at the \\begin");
    let noticed = d.noticed.as_ref().ok_or("where TeX noticed it is lost")?;
    assert_eq!((noticed.file.as_str(), noticed.line), ("main.tex", Some(6)));
    Ok(())
}

#[test]
fn a_brace_left_open_in_an_included_file_is_reported_there() -> Result<(), String> {
    let o = build("brace", &[("main.tex", MAIN), ("sections/intro.tex", "Intro \\textbf{bold\n")])?;
    let d = o.diagnostics.iter().find(|d| d.severity == Severity::Error).ok_or("no error")?;
    assert_eq!((d.file.as_deref(), d.line), (Some("sections/intro.tex"), Some(1)), "{d:?}");
    assert!(d.noticed.is_some());
    Ok(())
}
