//! Exports through retex itself.

use std::sync::Arc;

use texcraft_compile::{Backend, Export, Job};
use texcraft_project::ProjectPath;

#[test]
fn retex_exports_a_document_to_html_with_its_text() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("texcraft-export-{}", std::process::id()));
    let backend = texcraft_compile::retex_backend::Retex::new(Some(dir.clone()));
    let export = backend.exporter().ok_or("retex has no exporter")?;
    let source = "\\documentclass{article}\n\\begin{document}\nHello exported world.\n\\end{document}\n";
    let main = ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    let job = Job { files: vec![(main.clone(), Arc::from(source.as_bytes()))], main, id: 1, options: Default::default() };
    let html = export(&job, Export::Html, "media")?;
    let text = String::from_utf8_lossy(&html.data);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(text.contains("exported"), "the text is not in the HTML");
    Ok(())
}

#[test]
fn a_document_that_does_not_build_is_not_exported() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("texcraft-export-bad-{}", std::process::id()));
    let backend = texcraft_compile::retex_backend::Retex::new(Some(dir.clone()));
    let export = backend.exporter().ok_or("retex has no exporter")?;
    let main = ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    let job = Job {
        files: vec![(main.clone(), Arc::from(&b"\\documentclass{article}\n\\begin{document}\n\\nosuchmacro\n\\end{document}\n"[..]))],
        main,
        id: 1,
        options: Default::default(),
    };
    let result = export(&job, Export::Odt, "media");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_err(), "a failed build was exported");
    Ok(())
}
