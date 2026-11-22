use texcraft_project::{Contents, Error, Project, ProjectPath};

fn path(s: &str) -> Result<ProjectPath, Error> {
    ProjectPath::new(s)
}

#[test]
fn removing_the_main_document_makes_another_tex_file_main() -> Result<(), Error> {
    let mut p = Project::blank();
    p.add(path("figure.png")?, Contents::Binary(vec![0x89, b'P']))?;
    p.add(path("other.tex")?, Contents::Text(String::new()))?;
    p.remove(&path("main.tex")?)?;
    assert_eq!(p.main(), Some(&path("other.tex")?));
    Ok(())
}

#[test]
fn renaming_the_main_document_keeps_it_main() -> Result<(), Error> {
    let mut p = Project::blank();
    p.rename(&path("main.tex")?, path("paper.tex")?)?;
    assert_eq!(p.main(), Some(&path("paper.tex")?));
    Ok(())
}

#[test]
fn adding_over_an_existing_file_is_refused_and_keeps_it() -> Result<(), Error> {
    let mut p = Project::blank();
    let before = p.text(&path("main.tex")?)?.to_owned();
    assert!(p.add(path("main.tex")?, Contents::Text("x".into())).is_err());
    assert_eq!(p.text(&path("main.tex")?)?, before);
    Ok(())
}
