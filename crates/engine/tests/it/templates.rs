use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_templates::{Catalog, Store};

type R = Result<(), Box<dyn std::error::Error>>;

#[test]
fn a_project_from_a_kept_template_has_its_files_its_licence_note_and_its_main_document() -> R {
    let base = std::env::temp_dir().join(format!("texcraft-it-templates-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let store = Store::new(base.join("store"));
    let catalog = Catalog::builtin()?;
    let template = catalog.find("acm-sigconf")?;
    let files: Vec<(String, Vec<u8>)> = template.project_paths().into_iter().map(|(_, p)| (p, b"\\documentclass{article}".to_vec())).collect();
    store.keep(template, &files)?;

    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(base.join("store"));
    let folder = base.join("paper");
    s.execute("project.from_template", &json!({"template": "acm-sigconf", "folder": folder.display().to_string()}))?;
    assert!(folder.join("sigconf.tex").is_file());
    assert!(folder.join("sample-base.bib").is_file(), "companion files come along");
    assert!(folder.join("TEMPLATE-LICENSE").is_file(), "the licence note is kept");
    assert_eq!(s.project().main().map(|p| p.as_str()), Some("sigconf.tex"));

    let again = s.execute("project.from_template", &json!({"template": "acm-sigconf", "folder": folder.display().to_string()}));
    assert!(again.is_err(), "a folder with files is refused");
    std::fs::remove_dir_all(&base)?;
    Ok(())
}

#[test]
fn the_example_project_is_made_in_a_folder_without_a_download_and_listed_as_the_example() -> R {
    let base = std::env::temp_dir().join(format!("texcraft-it-example-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(base.join("store"));
    let listed = s.execute("template.list", &json!({}))?;
    let examples: Vec<&str> = listed.as_array().into_iter().flatten().filter(|t| t["example"] == true).filter_map(|t| t["id"].as_str()).collect();
    assert_eq!(examples.len(), 1, "agents can tell the example from the templates");
    let folder = base.join("example");
    s.execute("project.from_template", &json!({"template": examples[0], "folder": folder.display().to_string()}))?;
    for file in ["main.tex", "sections/introduction.tex", "references.bib"] {
        assert!(folder.join(file).is_file(), "{file} is missing");
    }
    let note = std::fs::read_to_string(folder.join("TEMPLATE-LICENSE"))?;
    assert!(note.contains("CC0-1.0") && !note.contains("CTAN"), "our own example's note names CTAN: {note}");
    std::fs::remove_dir_all(&base)?;
    Ok(())
}
