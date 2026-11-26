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
pub(crate) mod sample;

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

fn scroll(harness: &mut Harness<'_, App>) -> egui::Vec2 {
    let inspect = harness.state_mut().control("ui.inspect", &json!({}));
    let at = |i: usize| inspect["result"]["preview"]["scroll"][i].as_f64().unwrap_or_default() as f32;
    egui::vec2(at(0), at(1))
}

/// Issue #19: Ctrl (Cmd on a Mac) with the wheel zooms the PDF, keeping the point under the
/// pointer where it was.
#[test]
fn ctrl_and_the_wheel_zoom_around_the_pointer() {
    let mut harness = built();
    ui_cmd::run(harness.state_mut(), "view.actual_size");
    settle(&mut harness);
    let page = harness.get_by_label("Page 1 of 2").rect();
    // A point a third of the way into the page, and what of the page is under it.
    let pointer = page.min + page.size() / 3.0;
    let before = zoom(&mut harness) as f32;
    let under = (pointer - page.min) / before;
    harness.hover_at(pointer);
    settle(&mut harness);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 60.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::COMMAND,
    };
    harness.event(wheel);
    settle(&mut harness);
    let after = zoom(&mut harness) as f32;
    assert!(after > before * 1.05, "the zoom went from {before} to {after}");
    let page = harness.get_by_label("Page 1 of 2").rect();
    let now = (pointer - page.min) / after;
    assert!((now - under).length() < 4.0, "the point under the pointer moved from {under:?} to {now:?}");
}

/// Issue #18: the middle button dragged over pages larger than the pane moves them with it.
#[test]
fn the_middle_button_drags_the_pages() {
    let mut harness = built();
    harness.get_by_label("Fit width").click();
    settle(&mut harness);
    harness.get_by_label("400 %").click();
    settle(&mut harness);
    let page = harness.get_by_label("Page 1 of 2").rect();
    let start = harness.get_by_label("Zoom In").rect().center() + egui::vec2(0.0, 300.0);
    let start = egui::pos2(start.x.max(page.left() + 10.0), start.y);
    let before = scroll(&mut harness);
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: egui::Modifiers::NONE };
    harness.event(egui::Event::PointerMoved(start));
    harness.event(button(start, true));
    settle(&mut harness);
    let end = start - egui::vec2(120.0, 150.0);
    for step in 1..=3 {
        harness.event(egui::Event::PointerMoved(start + (end - start) * step as f32 / 3.0));
        settle(&mut harness);
    }
    harness.event(button(end, false));
    settle(&mut harness);
    let after = scroll(&mut harness);
    assert!(after.x > before.x + 60.0 && after.y > before.y + 60.0, "the pages did not follow the pointer: {before:?} to {after:?}");
}

/// Issue #41: the zoom's Fit button goes back to the last fit chosen, named after it, and has
/// nothing to do at that fit already.
#[test]
fn the_fit_button_goes_back_to_the_last_fit_chosen() {
    use egui_kittest::kittest::NodeT as _;
    let mut harness = built();
    let enabled = |h: &Harness<'_, App>, name: &str| !h.get_by_label(name).accesskit_node().is_disabled();
    assert!(!enabled(&harness, "Back to Fit Width"), "at fit width already, the button can be clicked");
    ui_cmd::run(harness.state_mut(), "view.fit_page");
    ui_cmd::run(harness.state_mut(), "view.zoom_in");
    settle(&mut harness);
    assert!(harness.query_by_label("Fit page").is_none(), "the zoom is still at fit page");
    assert!(enabled(&harness, "Back to Fit Page"), "the button does not go back to the fit chosen last");
    harness.get_by_label("Back to Fit Page").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Fit page").is_some(), "the button did not go back to fit page");
}

/// Issue #40: the button that moves the PDF to its own window ends the PDF's header, after the
/// zoom.
#[test]
fn the_own_window_button_ends_the_header_after_the_zoom() {
    let harness = built();
    let rect = |label: &str| harness.get_by_label(label).rect();
    let window = rect("Open the PDF in its own window");
    for before in ["Zoom In", "Back to Fit Width", "Fit width", "Export"] {
        let r = rect(before);
        assert!(r.right() <= window.left(), "{before} is not before the window's button: {r:?} {window:?}");
    }
    let compile = rect("Compile");
    assert!((window.center().y - compile.center().y).abs() < compile.height() / 2.0, "the window's button left the header");
}

fn preview(harness: &mut Harness<'_, App>) -> serde_json::Value {
    harness.state_mut().control("ui.inspect", &json!({}))["result"]["preview"].clone()
}

/// Issue #39: Two Pages puts the pages side by side, and zooming out puts as many across as fit.
#[test]
fn two_pages_and_zooming_out_put_the_pages_side_by_side() {
    let mut harness = built();
    let page = |h: &Harness<'_, App>, n: usize| h.get_by_label(&format!("Page {n} of 2")).rect();
    assert_eq!(preview(&mut harness)["columns"], 1, "at fit width the pages are not one under the other");
    let (one, two) = (page(&harness, 1), page(&harness, 2));
    assert!(two.top() > one.bottom(), "the second page is not under the first");

    harness.get_by_label("Fit width").click();
    settle(&mut harness);
    harness.get_by_label("Two Pages").click();
    settle(&mut harness);
    assert_eq!(preview(&mut harness)["two_pages"], true);
    let (one, two) = (page(&harness, 1), page(&harness, 2));
    assert!(two.left() > one.right() && (two.top() - one.top()).abs() < 1.0, "two pages are not side by side: {one:?} {two:?}");
    assert!(two.width() < one.width() * 1.01 && one.width() * 2.0 < 1200.0, "the fit did not make room for both");

    ui_cmd::run(harness.state_mut(), "view.two_pages");
    ui_cmd::run_with(harness.state_mut(), "view.zoom", &json!({"percent": 25}));
    settle(&mut harness);
    let (one, two) = (page(&harness, 1), page(&harness, 2));
    assert!(two.left() > one.right() && (two.top() - one.top()).abs() < 1.0, "zoomed out, the pages stay one under the other");
}
