use texcraft_templates::Catalog;

#[test]
fn the_builtin_catalogue_parses_and_every_template_has_its_main_file() {
    let catalog = Catalog::builtin().unwrap();
    assert!(!catalog.templates().is_empty());
    for t in catalog.templates() {
        assert!(t.project_paths().iter().any(|(_, project)| *project == t.main), "{}", t.id);
    }
}

#[test]
fn a_file_path_that_leaves_its_folder_is_refused() {
    let text = r#"
[[template]]
id = "x"
title = "x"
publisher = "x"
category = "journal"
source = { texlive = "x" }
files = ["../../etc/passwd", "main.tex"]
main = "main.tex"
licence = "MIT"
"#;
    assert!(Catalog::parse(text).is_err());
}

#[test]
fn files_keep_their_layout_relative_to_the_main_files_folder() {
    let text = r#"
[[template]]
id = "x"
title = "x"
publisher = "x"
category = "report"
source = { texlive = "x" }
files = ["samples/paper.tex", "samples/refs.bib", "samples/graphics/fig.png"]
main = "paper.tex"
licence = "MIT"
"#;
    let catalog = Catalog::parse(text).unwrap();
    let paths: Vec<String> = catalog.templates()[0].project_paths().into_iter().map(|(_, p)| p).collect();
    assert_eq!(paths, ["paper.tex", "refs.bib", "graphics/fig.png"]);
}

#[test]
fn every_builtin_template_is_compiled_in_and_kept_without_a_download() {
    let catalog = Catalog::builtin().unwrap();
    let builtin: Vec<_> = catalog.templates().iter().filter(|t| t.is_builtin()).collect();
    assert!(!builtin.is_empty());
    let dir = std::env::temp_dir().join(format!("texcraft-builtin-{}", std::process::id()));
    let store = texcraft_templates::Store::new(&dir);
    for t in builtin {
        let files = t.builtin_files();
        for (_, path) in t.project_paths() {
            assert!(files.iter().any(|(p, _)| *p == path), "{}: {path} is listed but not compiled in", t.id);
        }
        let kept = store.kept(t).unwrap_or_else(|| panic!("{} is not kept", t.id));
        assert!(kept.join(&t.main).is_file(), "{}: its main file was not written", t.id);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
