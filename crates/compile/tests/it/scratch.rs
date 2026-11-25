//! A scratch document after the project's preamble (AI4's preview), typeset in process.

use std::sync::Arc;

use texcraft_compile::scratch::{Scratch, typeset};
use texcraft_project::ProjectPath;

fn scratch(body: &str, files: Vec<(ProjectPath, Arc<[u8]>)>) -> Scratch {
    Scratch { files, main_dir: None, preamble: "\\documentclass{article}\n\\usepackage{mymacros}\n".to_owned(), body: body.to_owned() }
}

fn macros() -> Result<Vec<(ProjectPath, Arc<[u8]>)>, String> {
    let path = ProjectPath::new("mymacros.sty").map_err(|e| e.to_string())?;
    Ok(vec![(path, Arc::from(b"\\newcommand{\\R}{\\mathrm{R}}\n".as_slice()))])
}

#[test]
fn a_body_using_the_projects_own_package_typesets_and_an_undefined_one_says_where() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("texcraft-scratch-{}", std::process::id()));
    let table = scratch("\\begin{tabular}{cc}\n$\\R$ & b\\\\\nc & d\n\\end{tabular}", macros()?);
    let pdf = typeset(&dir, &table).map_err(|m| format!("a valid table did not typeset: {m:?}"))?;
    assert!(texcraft_preview::Document::open(Arc::new(pdf)).is_ok_and(|d| d.page_count() >= 1), "no page");
    let wrong = scratch("Fine.\n\\nosuchmacro{x}", macros()?);
    let messages = typeset(&dir, &wrong).err().ok_or("an undefined command typeset")?;
    let _ = std::fs::remove_dir_all(&dir);
    assert!(messages.iter().any(|m| m.contains("line 2")), "the message does not count lines in the body: {messages:?}");
    Ok(())
}
