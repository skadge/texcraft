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
    Ok((top(&h, "PPTX Presentation (.pptx)"), top(&h, "DOCX Document (.docx)")))
}

#[test]
fn a_presentation_lists_its_slide_formats_before_the_text_formats() -> Result<(), String> {
    let (pptx, docx) = menu_for("\\documentclass[11pt]{beamer}\n\\begin{document}\\end{document}\n")?;
    assert!(pptx < docx, "PPTX is not above DOCX for a beamer document");
    let (pptx, docx) = menu_for("\\documentclass{article}\n\\begin{document}\\end{document}\n")?;
    assert!(docx < pptx, "DOCX is not above PPTX for an article");
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
    h.get_by_label("DOCX Document (.docx)").click();
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
    assert!(h.query_all_by_label_contains("Exporting as DOCX Document").next().is_some(), "the status line does not say the export started");
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

/// Issue #22: in the browser the PDF is downloaded: the PDF pane's button and the Export menu's
/// first entry say so and can be chosen once there is a PDF (they were greyed out).
#[test]
fn in_the_browser_the_pdf_can_be_downloaded_from_the_pdf_pane() {
    use egui_kittest::kittest::NodeT as _;
    let mut s = Session::with_backend(Box::new(super::pdf_preview::Sample));
    s.use_library(texcraft_engine::library::Library::default());
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "build.compile");
    for _ in 0..400 {
        h.step();
        if h.query_by_label("1 / 2").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
    let enabled = |h: &Harness<'_, App>| !h.get_by_role_and_label(egui::accesskit::Role::Button, "Download PDF").accesskit_node().is_disabled();
    assert!(enabled(&h), "the PDF pane's Download PDF is greyed out");
    h.get_by_label("Export").click();
    h.run();
    // The pane's button, and the menu's entry.
    let ready = h.query_all_by_label("Download PDF").filter(|n| !n.accesskit_node().is_disabled()).count();
    assert_eq!(ready, 2, "the Export menu's PDF entry is greyed out or missing");
}
