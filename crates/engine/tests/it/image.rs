//! `file.insert_image`: an image of the project placed in a document as LaTeX finds it.

use serde_json::{Value, json};

fn session() -> Result<texcraft_engine::Session, String> {
    let mut s = texcraft_engine::Session::with_backend(Box::new(texcraft_compile::Unavailable("tests".into())));
    s.execute(
        "file.write",
        &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\begin{document}\n\\input{chapters/intro}\n\\end{document}\n"}),
    )?;
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    s.execute("file.new", &json!({"path": "chapters/intro.tex", "text": "Before. After.\n"}))?;
    // A PNG's first bytes.
    s.execute("file.import", &json!({"path": "figures/plot.png", "data": "iVBORw0KGgo="}))?;
    Ok(s)
}

fn read(s: &mut texcraft_engine::Session, path: &str) -> String {
    let v = s.execute("file.read", &json!({"path": path})).unwrap_or_default();
    v["text"].as_str().unwrap_or_default().to_owned()
}

fn insert(s: &mut texcraft_engine::Session, at: usize) -> Result<Value, String> {
    s.execute("file.insert_image", &json!({"path": "chapters/intro.tex", "image": "figures/plot.png", "at": at}))
}

#[test]
fn the_path_is_from_the_main_documents_folder_on_a_line_of_its_own() {
    let mut s = session().unwrap();
    insert(&mut s, "Before.".len()).unwrap();
    let text = read(&mut s, "chapters/intro.tex");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.first(), Some(&"Before."), "{text}");
    assert_eq!(lines.last(), Some(&" After."), "{text}");
    let middle = lines.get(1).copied().unwrap_or_default();
    // From main.tex's folder, not from chapters/ where the line is.
    assert!(middle.starts_with("\\includegraphics") && middle.ends_with("{figures/plot.png}"), "{text}");
}

#[test]
fn a_main_document_in_a_folder_reaches_up_to_the_image() {
    let mut s = session().unwrap();
    s.execute("file.new", &json!({"path": "paper/main.tex", "text": "\\documentclass{article}\n"})).unwrap();
    s.execute("file.set_main", &json!({"path": "paper/main.tex"})).unwrap();
    let r = insert(&mut s, 0).unwrap();
    assert_eq!(r["image"], "../figures/plot.png", "{r}");
}

#[test]
fn whether_graphicx_is_loaded_is_said() {
    let mut s = session().unwrap();
    assert_eq!(insert(&mut s, 0).unwrap()["graphicx"], false);
    let main = read(&mut s, "main.tex");
    let commented = main.replacen("\\begin{document}", "% \\usepackage{graphicx}\n\\begin{document}", 1);
    s.execute("file.write", &json!({"path": "main.tex", "text": commented})).unwrap();
    assert_eq!(insert(&mut s, 0).unwrap()["graphicx"], false, "a commented-out package counted");
    let loaded = main.replacen("\\begin{document}", "\\usepackage[final]{xcolor, graphicx}\n\\begin{document}", 1);
    s.execute("file.write", &json!({"path": "main.tex", "text": loaded})).unwrap();
    assert_eq!(insert(&mut s, 0).unwrap()["graphicx"], true);
}

#[test]
fn a_file_that_is_not_an_image_inserts_nothing() {
    let mut s = session().unwrap();
    let before = read(&mut s, "chapters/intro.tex");
    for image in ["main.tex", "figures/missing.png"] {
        let r = s.execute("file.insert_image", &json!({"path": "chapters/intro.tex", "image": image, "at": 0}));
        assert!(r.is_err(), "{image} was inserted");
    }
    assert_eq!(read(&mut s, "chapters/intro.tex"), before);
}
