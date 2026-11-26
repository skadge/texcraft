//! A9: the PDF in its own window. The harness cannot open a second OS window, so egui shows it as
//! a window inside the main one; what is checked is the main window's side: the editor takes the
//! PDF's room, and the PDF comes back when asked or when a layout is chosen.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

const WIDTH: f32 = 1400.0;

fn harness() -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Hello.\n"}))?;
    let prefs = Prefs { auto_compile: false, layout: "split".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(egui::vec2(WIDTH, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.run();
    Ok(h)
}

fn editor_width(h: &Harness<'_, App>) -> f32 {
    h.get_by_label("main.tex source").rect().width()
}

fn detached(h: &mut Harness<'_, App>) -> bool {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["pdf_window"].as_bool().unwrap_or_default()
}

#[test]
fn the_pdf_window_gives_the_editor_the_main_window_and_closing_it_brings_the_pdf_back() {
    let mut h = harness().unwrap();
    let split = editor_width(&h);
    assert!(split < WIDTH * 0.6, "the split layout's editor is {split} wide");

    h.get_by_label("Open the PDF in its own window").click();
    h.run();
    h.run();
    assert!(detached(&mut h));
    assert!(editor_width(&h) > split * 1.3, "the editor did not take the PDF's room: {} against {split}", editor_width(&h));
    assert!(h.query_by_label("Compile").is_some(), "the PDF pane is not shown anywhere");

    h.get_by_label("Put the PDF back in the main window").click();
    h.run();
    h.run();
    assert!(!detached(&mut h));
    assert!(editor_width(&h) < WIDTH * 0.6, "the PDF did not come back beside the editor");
}

#[test]
fn choosing_a_layout_brings_the_pdf_back_into_the_main_window() {
    let mut h = harness().unwrap();
    let r = h.state_mut().control("engine.execute", &json!({"command": "view.pdf_window"}));
    assert_eq!(r["ok"], true, "{r}");
    h.run();
    assert!(detached(&mut h));
    let r = h.state_mut().control("engine.execute", &json!({"command": "view.layout.preview"}));
    assert_eq!(r["ok"], true, "{r}");
    h.run();
    h.run();
    assert!(!detached(&mut h), "a layout chosen left the PDF in its own window");
    assert!(h.query_by_label("main.tex source").is_none(), "the PDF-only layout still shows the editor");
}

/// Issue #39: the PDF's own window has the page thumbnails: the page in view marked, a click
/// going to another.
#[test]
fn the_pdf_window_shows_thumbnails_that_go_to_their_page() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT as _;
    let s = Session::with_backend(Box::new(crate::pdf_preview::Sample));
    let prefs = Prefs { auto_compile: false, layout: "split".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(egui::vec2(WIDTH, 1000.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "build.compile");
    for _ in 0..400 {
        h.step();
        if h.query_by_label_contains("1 / 2").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.get_by_label("Open the PDF in its own window").click();
    for _ in 0..3 {
        h.step();
    }
    assert!(h.query_by_label("Page 1").is_none(), "the thumbnails are shown before they are asked for");
    h.get_by_role_and_label(Role::Button, "Page Thumbnails").click();
    for _ in 0..3 {
        h.step();
    }
    let selected = |h: &Harness<'_, App>, n: usize| h.get_by_role_and_label(Role::Button, &format!("Page {n}")).accesskit_node().is_selected();
    assert_eq!(selected(&h, 1), Some(true), "the page in view is not marked");
    assert_eq!(selected(&h, 2), Some(false));
    h.get_by_role_and_label(Role::Button, "Page 2").click();
    for _ in 0..4 {
        h.step();
    }
    let page = h.state_mut().control("ui.inspect", &json!({}))["result"]["preview"]["page"].as_u64();
    assert_eq!(page, Some(2), "the thumbnail did not go to its page");
    assert_eq!(selected(&h, 2), Some(true), "the mark did not follow");
}
