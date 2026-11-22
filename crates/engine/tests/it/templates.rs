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
