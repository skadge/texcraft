//! The build log's raw log search, and the build's output files.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::{Backend, Job, Outcome, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

/// A build with a PDF, SyncTeX data and a log that names a package three times, in two cases.
struct Logged;

impl Backend for Logged {
    fn name(&self) -> String {
        "logged".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let mut log = String::from("(./main.tex\nLaTeX2e <2024-11-01>\n(hyperref.sty\n");
        for i in 0..200 {
            log.push_str(&format!("line {i} of the log\n"));
        }
        log.push_str("Package Hyperref Warning: the end.\n) HYPERREF done\n");
        Outcome {
            status: Status::Built,
            pdf: Some(Arc::new(b"%PDF-1.7".to_vec())),
            synctex: Some(Arc::new(b"SyncTeX Version:1".to_vec())),
            log,
            ..Outcome::unavailable(job.id, "")
        }
    }
}

fn harness() -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Logged));
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    Ok(h)
}

fn count(h: &Harness<'_, App>) -> Option<String> {
    ["1 of 3", "2 of 3", "3 of 3", "No results"].into_iter().find(|c| h.query_by_label(c).is_some()).map(str::to_owned)
}

#[test]
fn searching_the_raw_log_finds_and_counts_matches_and_steps_through_them() -> Result<(), String> {
    let mut h = harness()?;
    h.get_by_label("Raw log").click();
    h.run();
    let field = h.get_by_label("Search the raw log");
    field.focus();
    field.type_text("hyperref");
    h.run();
    assert_eq!(count(&h).as_deref(), Some("1 of 3"), "every match is counted, whatever its case");
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(count(&h).as_deref(), Some("2 of 3"), "Enter goes to the next match");
    assert!(h.get_by_label("Search the raw log").is_focused(), "the field keeps the keyboard");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run();
    assert_eq!(count(&h).as_deref(), Some("1 of 3"), "Shift+Enter goes back");
    h.get_by_label("Previous Match").click();
    h.run();
    assert_eq!(count(&h).as_deref(), Some("3 of 3"), "going back from the first wraps to the last");
    h.get_by_label("Search the raw log").focus();
    h.run();
    h.get_by_label("Search the raw log").type_text("zzz");
    h.run();
    assert_eq!(count(&h).as_deref(), Some("No results"));
    Ok(())
}

#[test]
fn the_build_log_lists_the_build_s_files_and_saves_one_where_asked() -> Result<(), String> {
    let mut h = harness()?;
    assert!(h.query_by_label("Save As… main.pdf").is_some(), "the PDF is listed");
    assert!(h.query_by_label("Save As… main.synctex").is_some(), "the SyncTeX data is listed");
    assert!(h.query_by_label("Save As… main-print.pdf").is_none(), "a screen build has no PDF/X");
    h.get_by_label("Save As… main.log").click();
    h.run();
    let dir = std::env::temp_dir().join(format!("texcraft-ui-outputs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("saved.log");
    let field = h.get_by_label("The file to write it to");
    field.focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    field.type_text(&path.display().to_string());
    h.key_press(egui::Key::Enter);
    h.run();
    let saved = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    assert!(saved.contains("HYPERREF done"), "the log is saved as the build wrote it");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
