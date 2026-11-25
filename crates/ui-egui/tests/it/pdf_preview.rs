//! The PDF preview (B8): its zoom menu, its links.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::{Backend, Job, Outcome, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

#[path = "../../../preview/tests/it/sample.rs"]
mod sample;

/// Builds the sample PDF of two pages with links.
struct Sample;

impl Backend for Sample {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        Outcome { status: Status::Built, pdf: Some(Arc::new(sample::links_pdf())), pages: 2, ..Outcome::unavailable(job.id, "") }
    }
}

/// The app in a window, the sample PDF shown beside the editor.
fn built() -> Harness<'static, App> {
    let s = Session::with_backend(Box::new(Sample));
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    ui_cmd::run(harness.state_mut(), "build.compile");
    for _ in 0..400 {
        harness.step();
        if harness.query_by_label("1 / 2").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(harness.query_by_label("1 / 2").is_some(), "the PDF is not shown");
    harness
}

fn zoom(harness: &mut Harness<'_, App>) -> f64 {
    let inspect = harness.state_mut().control("ui.inspect", &json!({}));
    inspect["result"]["preview"]["zoom"].as_f64().unwrap_or_default()
}

#[test]
fn the_zoom_menu_sets_a_fixed_zoom() {
    let mut harness = built();
    harness.get_by_label("Fit width").click();
    harness.run();
    harness.get_by_label("200 %").click();
    harness.run();
    assert!(harness.query_by_label("200 %").is_some(), "the zoom button does not say the zoom");
    assert!((zoom(&mut harness) - 2.0).abs() < 1e-3, "the zoom is {}", zoom(&mut harness));
}

#[test]
fn the_actual_size_chord_zooms_the_pdf_not_the_window() {
    let mut harness = built();
    ui_cmd::run(harness.state_mut(), "view.fit_page");
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    harness.run();
    assert!((zoom(&mut harness) - 1.0).abs() < 1e-3, "the zoom is {}", zoom(&mut harness));
    assert!(harness.query_by_label("100 %").is_some(), "the zoom button does not say 100 %");
}
