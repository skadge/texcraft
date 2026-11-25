//! `table.*` and `figure.*`: the grid's and the figure dialog's edits, as agents make them.

use serde_json::{Value, json};
use texcraft_engine::Session;

const MAIN: &str = "\\documentclass{article}\n\\begin{document}\n\\begin{tabular}{ll}\n  a & b \\\\\n  c & d \\\\\n\\end{tabular}\n\n\\begin{figure}[h]\n  \\includegraphics{plot}\n  \\caption{Old.}\n\\end{figure}\n\\end{document}\n";

fn session() -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(texcraft_compile::Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": MAIN}))?;
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    Ok(s)
}

fn read(s: &mut Session) -> String {
    let v = s.execute("file.read", &json!({"path": "main.tex"})).unwrap_or_default();
    v["text"].as_str().unwrap_or_default().to_owned()
}

fn at(needle: &str) -> usize {
    MAIN.find(needle).map_or(0, |b| MAIN[..b].chars().count())
}

#[test]
fn a_cell_is_set_escaped_and_the_table_reads_back() -> Result<(), String> {
    let mut s = session()?;
    s.execute("table.edit", &json!({"path": "main.tex", "at": at("d \\\\"), "op": "set_cell", "text": "50% & more"}))?;
    assert_eq!(read(&mut s), MAIN.replace("c & d", "c & 50\\% \\& more"));
    let v = s.execute("table.inspect", &json!({"path": "main.tex"}))?;
    assert_eq!(v["tables"][0]["rows"][1][1]["text"], "50\\% \\& more", "{v}");
    Ok(())
}

#[test]
fn booktabs_borders_say_the_package_is_missing() -> Result<(), String> {
    let mut s = session()?;
    let v = s.execute("table.edit", &json!({"path": "main.tex", "at": at("a &"), "op": "borders", "borders": "booktabs"}))?;
    assert_eq!(v["loaded"], json!(false), "{v}");
    assert!(v["note"].as_str().is_some_and(|n| n.contains("\\usepackage{booktabs}")), "{v}");
    assert!(!read(&mut s).contains("\\usepackage{booktabs}"), "the package was added silently");
    let rows = s.execute("table.edit", &json!({"path": "main.tex", "at": at("a &"), "op": "insert_row_below", "row": 0}))?;
    assert_eq!(rows["changed"], json!(true));
    let v: Value = s.execute("table.inspect", &json!({"path": "main.tex", "at": at("a &")}))?;
    assert_eq!(v["tables"][0]["rows"].as_array().map(Vec::len), Some(3));
    assert_eq!(v["tables"][0]["borders"], "booktabs");
    Ok(())
}

#[test]
fn the_figure_changes_its_caption_and_a_new_one_finds_its_image() -> Result<(), String> {
    let mut s = session()?;
    s.execute("figure.edit", &json!({"path": "main.tex", "at": at("\\caption"), "caption": "New.", "width": 0.5}))?;
    let text = read(&mut s);
    assert!(text.contains("\\caption{New.}") && text.contains("\\includegraphics[width=0.5\\linewidth]{plot}"), "{text}");
    s.execute("file.import", &json!({"path": "figures/x.png", "data": "iVBORw0KGgo="}))?;
    s.execute("file.new", &json!({"path": "chapters/one.tex", "text": "Text.\n"}))?;
    s.execute("figure.insert", &json!({"path": "chapters/one.tex", "at": 5, "image": "figures/x.png", "label": "fig:x"}))?;
    let v = s.execute("file.read", &json!({"path": "chapters/one.tex"}))?;
    let one = v["text"].as_str().unwrap_or_default();
    assert!(one.starts_with("Text.\n\\begin{figure}") && one.contains("{figures/x.png}") && one.contains("\\label{fig:x}"), "{one}");
    Ok(())
}
