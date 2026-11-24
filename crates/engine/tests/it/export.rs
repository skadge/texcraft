use std::sync::Arc;

use serde_json::json;
use texcraft_compile::{Backend, Export, Exported, Exporter, Job, Outcome, Unavailable};
use texcraft_engine::Session;

type R = Result<(), Box<dyn std::error::Error>>;

fn temp(name: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn session(backend: Box<dyn Backend>, main: &str) -> Session {
    let mut s = Session::with_backend(backend);
    let _ = s.execute("file.write", &json!({"path": "main.tex", "text": main}));
    let _ = s.execute("file.set_main", &json!({"path": "main.tex"}));
    s
}

#[test]
fn an_engine_without_a_converter_refuses_to_export() -> R {
    let dir = temp("export-none")?;
    let mut s = session(Box::new(Unavailable("no engine".to_owned())), "\\documentclass{article}");
    let path = dir.join("out.docx");
    let err = s.execute("build.export", &json!({"format": "docx", "path": path.display().to_string()}));
    assert!(err.is_err(), "an export without a converter was reported as done: {err:?}");
    assert!(!path.exists(), "a file was written");
    Ok(())
}

/// Exports as Markdown with one picture, or fails with a reason.
struct Converting(bool);

impl Backend for Converting {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, "")
    }
    fn exporter(&self) -> Option<Exporter> {
        let works = self.0;
        Some(Arc::new(move |_: &Job, format: Export, media: &str| {
            if !works {
                return Err("Undefined control sequence".to_owned());
            }
            Ok(Exported { data: format!("{format:?}").into_bytes(), media: vec![(format!("{media}/a.png"), vec![1, 2, 3])] })
        }))
    }
}

#[test]
fn an_export_writes_the_document_and_its_pictures_beside_it() -> R {
    let dir = temp("export-md")?;
    let mut s = session(Box::new(Converting(true)), "\\documentclass{article}");
    let path = dir.join("paper.md");
    let done = s.execute("build.export", &json!({"format": "markdown", "path": path.display().to_string()}))?;
    assert!(path.exists(), "{done}");
    assert!(dir.join("paper_media/a.png").exists(), "the picture is not beside the document: {done}");
    assert_eq!(done["files"].as_array().map(Vec::len), Some(2), "{done}");
    Ok(())
}

#[test]
fn a_failed_export_says_why_and_writes_nothing() -> R {
    let dir = temp("export-fail")?;
    let mut s = session(Box::new(Converting(false)), "\\documentclass{article}");
    let path = dir.join("paper.odt");
    let err = s.execute("build.export", &json!({"format": "odt", "path": path.display().to_string()})).err().unwrap_or_default();
    assert!(err.contains("Undefined control sequence"), "the reason is lost: {err}");
    assert!(!path.exists());
    Ok(())
}

#[test]
fn a_beamer_main_document_is_a_presentation() {
    let beamer = session(Box::new(Converting(true)), "% \\documentclass{article}\n\\documentclass[aspectratio=169]{beamer}\n");
    assert!(beamer.is_presentation());
    let article = session(Box::new(Converting(true)), "\\documentclass{article}\n% beamer\n");
    assert!(!article.is_presentation());
}

#[test]
fn a_background_export_returns_at_once_and_reports_when_done() -> R {
    let dir = temp("export-bg")?;
    let mut s = session(Box::new(Converting(true)), "\\documentclass{article}");
    let path = dir.join("paper.docx");
    let started = s.execute("build.export", &json!({"format": "docx", "path": path.display().to_string(), "background": true}))?;
    assert_eq!(started["started"], true);
    let mut outcome = None;
    for _ in 0..200 {
        if let Some(o) = s.poll_export() {
            outcome = Some(o);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let done = outcome.ok_or("the export never ended")??;
    assert_eq!(done["path"], path.display().to_string());
    assert!(path.exists(), "the file was not written");
    assert!(!s.exporting(), "still marked as running");
    Ok(())
}

#[test]
fn a_failed_background_export_reports_its_reason() -> R {
    let dir = temp("export-bg-fail")?;
    let mut s = session(Box::new(Converting(false)), "\\documentclass{article}");
    let path = dir.join("paper.docx");
    s.execute("build.export", &json!({"format": "docx", "path": path.display().to_string(), "background": true}))?;
    let mut outcome = None;
    for _ in 0..200 {
        if let Some(o) = s.poll_export() {
            outcome = Some(o);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let err = outcome.ok_or("the export never ended")?.err().ok_or("a failed export was reported as done")?;
    assert!(err.contains("Undefined control sequence"), "{err}");
    assert!(!path.exists());
    Ok(())
}
