//! The example project the start page offers (`templates/builtin/example`) is a new user's first
//! build: it has to build with no error, and its references and citation have to resolve.

use std::sync::Arc;

use texcraft_compile::{Backend, Job, Options, Severity, Status};
use texcraft_project::ProjectPath;

const FILES: &[(&str, &[u8])] = &[
    ("main.tex", include_bytes!("../../../../templates/builtin/example/main.tex")),
    ("sections/introduction.tex", include_bytes!("../../../../templates/builtin/example/sections/introduction.tex")),
    ("references.bib", include_bytes!("../../../../templates/builtin/example/references.bib")),
];

#[test]
fn the_example_project_builds_cleanly_with_its_references_resolved() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("texcraft-example-{}", std::process::id()));
    let mut backend = texcraft_compile::retex_backend::Retex::new(Some(dir.clone()));
    let files =
        FILES.iter().map(|(p, b)| Ok((ProjectPath::new(p).map_err(|e| e.to_string())?, Arc::from(*b)))).collect::<Result<Vec<_>, String>>()?;
    let main = ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    let outcome = backend.compile(&Job {
        files,
        main,
        id: 1,
        options: Options::default(),
        project: None,
        packages: Default::default(),
        missing_packages: Default::default(),
    });
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(outcome.status, Status::Built, "{:?}", outcome.diagnostics);
    let problems: Vec<_> = outcome
        .diagnostics
        .iter()
        // An unresolved \ref or \cite is a warning that says "undefined".
        .filter(|d| d.severity == Severity::Error || d.message.to_lowercase().contains("undefined"))
        .collect();
    assert!(problems.is_empty(), "a first build should be clean: {problems:?}");
    assert!(outcome.pages > 1, "the example lost its content: {} pages", outcome.pages);
    Ok(())
}
