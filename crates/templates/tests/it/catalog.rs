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
