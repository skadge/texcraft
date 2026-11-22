use std::sync::Arc;

use serde_json::json;
use texcraft_compile::{Backend, Job, Outcome, Status};
use texcraft_engine::Session;

/// "Builds" by handing back the fixture's SyncTeX data with a PDF.
struct Synced;

impl Backend for Synced {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let synctex = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/subfolder.synctex")).ok().map(Arc::new);
        Outcome { status: Status::Built, pdf: Some(Arc::new(b"%PDF-1.7".to_vec())), synctex, ..Outcome::unavailable(job.id, "") }
    }
}

#[test]
fn a_line_of_a_file_in_the_main_documents_folder_goes_to_the_pdf_and_back() {
    let mut s = Session::with_backend(Box::new(Synced));
    for (path, text) in [("paper/main.tex", "main"), ("paper/sections/one.tex", "one")] {
        s.execute("file.new", &json!({"path": path})).unwrap();
        s.execute("file.write", &json!({"path": path, "text": text})).unwrap();
    }
    s.execute("file.set_main", &json!({"path": "paper/main.tex"})).unwrap();
    s.execute("build.compile", &json!({"wait": true})).unwrap();

    // SyncTeX names the file `sections/one.tex`, from the main document's folder.
    let found = s.execute("pdf.locate", &json!({"path": "paper/sections/one.tex", "line": 1})).unwrap();
    let rect = &found["rects"][0];
    assert_eq!(rect["page"], 1, "{found}");
    let (x, y) = (rect["x"].as_f64().unwrap() + 5.0, rect["y"].as_f64().unwrap() + 3.0);
    let back = s.execute("pdf.source", &json!({"page": 1, "x": x, "y": y})).unwrap();
    assert_eq!((back["path"].as_str(), back["line"].as_u64()), (Some("paper/sections/one.tex"), Some(1)), "{back}");

    let main = s.execute("pdf.locate", &json!({"path": "paper/main.tex", "line": 3})).unwrap();
    assert!(main["rects"].as_array().is_some_and(|r| !r.is_empty()), "the main file's line is not found: {main}");
    assert!(s.execute("pdf.source", &json!({"page": 9, "x": 1.0, "y": 1.0})).is_err(), "a page the PDF does not have gave a place");
}
