//! The PDF pane's Export menu.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::{Backend, Export, Exported, Exporter, Job, Outcome};
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

/// Builds nothing, converts to anything.
struct Converting;

impl Backend for Converting {
    fn name(&self) -> String {
        "test".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, "")
    }
    fn exporter(&self) -> Option<Exporter> {
        Some(Arc::new(|_: &Job, _: Export, _: &str| Ok(Exported::default())))
    }
}

/// The vertical position of each entry, by label.
fn top(h: &Harness<'_, App>, label: &str) -> f32 {
    h.get_by_label(label).rect().top()
}

fn menu_for(main: &str) -> Result<(f32, f32), String> {
    let mut s = Session::with_backend(Box::new(Converting));
    s.execute("file.write", &json!({"path": "main.tex", "text": main}))?;
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Export").click();
    h.run();
    Ok((top(&h, "PowerPoint Presentation (.pptx)"), top(&h, "Word Document (.docx)")))
}

#[test]
fn a_presentation_lists_its_slide_formats_before_the_text_formats() -> Result<(), String> {
    let (pptx, docx) = menu_for("\\documentclass[11pt]{beamer}\n\\begin{document}\\end{document}\n")?;
    assert!(pptx < docx, "PowerPoint is not above Word for a beamer document");
    let (pptx, docx) = menu_for("\\documentclass{article}\n\\begin{document}\\end{document}\n")?;
    assert!(docx < pptx, "Word is not above PowerPoint for an article");
    Ok(())
}

/// Builds nothing, and converts once `go` is set.
struct Slow(Arc<std::sync::atomic::AtomicBool>);

impl Backend for Slow {
    fn name(&self) -> String {
        "test".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome::unavailable(job.id, "")
    }
    fn exporter(&self) -> Option<Exporter> {
        let go = self.0.clone();
        Some(Arc::new(move |_: &Job, _: Export, _: &str| {
            while !go.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Ok(Exported::default())
        }))
    }
}

#[test]
fn the_export_pill_turns_while_a_file_is_exported_and_the_status_says_when_it_is_written() -> Result<(), String> {
    let go = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut s = Session::with_backend(Box::new(Slow(go.clone())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\begin{document}\\end{document}\n"}))?;
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    // A pill with its name, wider than the icon buttons beside it.
    let pill = h.get_by_role_and_label(egui::accesskit::Role::Button, "Export").rect();
    assert!(pill.width() > h.get_by_label("Build log").rect().width() * 2.0, "Export is not a pill with its name");
    h.get_by_label("Export").click();
    h.run();
    h.get_by_label("Word Document (.docx)").click();
    h.run();
    let out = std::env::temp_dir().join(format!("texcraft-export-pill-{}.docx", std::process::id()));
    h.get_by_label("The file to write the export to").type_text(&out.to_string_lossy());
    h.run();
    h.key_press(egui::Key::Enter);
    // The spinner repaints while the export runs: step frames, not until idle.
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_label("Exporting…").is_some(), "the pill does not say an export is running");
    assert!(h.query_all_by_label_contains("Exporting as Word Document").next().is_some(), "the status line does not say the export started");
    go.store(true, std::sync::atomic::Ordering::SeqCst);
    for _ in 0..400 {
        h.step();
        if h.query_all_by_label_contains("Exported to").next().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = std::fs::remove_file(&out);
    assert!(h.query_all_by_label_contains("Exported to").next().is_some(), "the status line does not say the file was written");
    h.step();
    assert!(h.query_by_label("Exporting…").is_none(), "the pill still turns after the export");
    Ok(())
}
