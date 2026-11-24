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
    h.get_by_label("Export…").click();
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
