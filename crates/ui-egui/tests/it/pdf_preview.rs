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
pub(crate) struct Sample;

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
    built_in("en")
}

/// The same, the interface in `language`.
fn built_in(language: &str) -> Harness<'static, App> {
    let s = Session::with_backend(Box::new(Sample));
    let app = App::with_prefs(s, Prefs { auto_compile: false, language: language.to_owned(), ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    ui_cmd::run(harness.state_mut(), "build.compile");
    for _ in 0..400 {
        harness.step();
        if harness.query_by_label_contains("1 / 2").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(harness.query_by_label_contains("1 / 2").is_some(), "the PDF is not shown");
    harness
}

/// Runs a few frames. Not `Harness::run`: the page thread asks for a frame each time it has drawn
/// a page, whenever that is.
fn settle(harness: &mut Harness<'_, App>) {
    for _ in 0..4 {
        harness.step();
    }
}

fn zoom(harness: &mut Harness<'_, App>) -> f64 {
    let inspect = harness.state_mut().control("ui.inspect", &json!({}));
    inspect["result"]["preview"]["zoom"].as_f64().unwrap_or_default()
}

#[test]
fn the_zoom_menu_sets_a_fixed_zoom() {
    let mut harness = built();
    harness.get_by_label("Fit width").click();
    settle(&mut harness);
    harness.get_by_label("200 %").click();
    settle(&mut harness);
    assert!(harness.query_by_label("200 %").is_some(), "the zoom button does not say the zoom");
    assert!((zoom(&mut harness) - 2.0).abs() < 1e-3, "the zoom is {}", zoom(&mut harness));
}

#[test]
fn the_actual_size_chord_zooms_the_pdf_not_the_window() {
    let mut harness = built();
    ui_cmd::run(harness.state_mut(), "view.fit_page");
    settle(&mut harness);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    settle(&mut harness);
    assert!((zoom(&mut harness) - 1.0).abs() < 1e-3, "the zoom is {}", zoom(&mut harness));
    assert!(harness.query_by_label("100 %").is_some(), "the zoom button does not say 100 %");
}

#[test]
fn a_link_to_a_page_scrolls_there() {
    let mut harness = built();
    // Two links go there: the first is drawn with its destination given in place.
    harness.query_all_by_label("Link to page 2").next().expect("the link is not exposed").click();
    settle(&mut harness);
    assert!(harness.query_by_label("2 / 2").is_some(), "the link did not scroll to page 2");
}

#[test]
fn a_link_to_a_script_is_not_opened() {
    let mut harness = built();
    harness.get_by_label("Link to javascript:alert(1)").click();
    settle(&mut harness);
    let inspect = harness.state_mut().control("ui.inspect", &json!({}));
    assert_eq!(inspect["result"]["status"]["error"], json!(true), "{inspect}");
}

#[test]
fn text_dragged_over_is_copied() {
    let mut harness = built();
    ui_cmd::run(harness.state_mut(), "view.actual_size");
    settle(&mut harness);
    // At 100 %, a point of the page is a point of the window. "Hello world" is at x 72 from the
    // left and on the baseline 700 points from the bottom of the 792-point page.
    let page = harness.get_by_label("Page 1 of 2").rect();
    let at = |x: f32, y: f32| page.min + egui::vec2(x, 792.0 - y);
    harness.hover_at(at(60.0, 704.0));
    settle(&mut harness);
    harness.drag_at(at(60.0, 704.0));
    settle(&mut harness);
    for x in [90.0, 150.0, 250.0] {
        harness.hover_at(at(x, 704.0));
        settle(&mut harness);
    }
    harness.drop_at(at(250.0, 704.0));
    settle(&mut harness);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::C);
    settle(&mut harness);
    let inspect = harness.state_mut().control("ui.inspect", &json!({}));
    assert_eq!(inspect["result"]["status"]["text"], json!("Copied 11 characters from the PDF."), "{inspect}");
}

#[test]
fn a_presentation_goes_through_the_pages_and_escape_leaves_it_where_it_was() {
    let mut harness = built();
    harness.get_by_label("Fit width").click();
    settle(&mut harness);
    harness.get_by_label_contains("Present").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Page 1 of 2").is_some(), "the first page is not presented");
    assert!(harness.query_by_label("Compile").is_none(), "the window's controls show during the presentation");
    harness.key_press(egui::Key::Space);
    settle(&mut harness);
    assert!(harness.query_by_label("Page 2 of 2").is_some(), "Space did not go to the next page");
    harness.key_press(egui::Key::Escape);
    settle(&mut harness);
    assert!(harness.query_by_label("2 / 2").is_some(), "the preview is not at the page presented last");
}

#[test]
fn longer_labels_send_the_pages_and_the_zoom_to_a_row_of_their_own_rather_than_over_export() {
    // The pseudo locale makes every label 40 % longer, as German does.
    let mut harness = built_in("pseudo");
    settle(&mut harness);
    let rect = |h: &Harness<'_, App>, label: &str| h.get_by_label_contains(label).rect();
    let (export, zoom_out, pages) = (rect(&harness, "Export"), rect(&harness, "Zoom Out"), rect(&harness, "Previous page"));
    assert!(!export.intersects(zoom_out) && !export.intersects(pages), "the page tools are drawn over Export: {export:?} {zoom_out:?}");
    assert!(zoom_out.top() >= export.bottom(), "the page tools did not move under the buttons");
}
